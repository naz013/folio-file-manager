//! What a picture says about itself: its size and the common EXIF fields, for Quick Look and the
//! inspector. Only headers are read (never the whole picture), and a file that can't be read
//! gives nothing rather than an error: the details are a courtesy, not something to fail on.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use exif::{In, Reader, Tag, Value};
use serde::Serialize;

/// The longest text kept from a field: a picture's own words are untrusted and may be huge.
const FIELD_LIMIT: usize = 100;

/// How much of the start of a picture is looked at for EXIF. The readers walk the chunks of a
/// PNG or WebP by reading through them, and load a whole TIFF, so they are given only this much:
/// EXIF further in is not found, but no picture costs more than this to look at.
const EXIF_PREFIX: u64 = 4 << 20;

/// A place on Earth, in decimal degrees.
#[derive(Debug, Serialize, PartialEq)]
pub struct Place {
    pub lat: f64,
    pub lon: f64,
}

/// What was found in a picture. Every field is optional: cameras and editors write what they
/// like, and many pictures have no EXIF at all.
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    /// Pixels across and down, as the picture is shown (turned if EXIF says it is rotated).
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The make and model of the camera.
    pub camera: Option<String>,
    pub lens: Option<String>,
    /// When it was taken, as the camera wrote it, as `YYYY-MM-DD HH:MM:SS`.
    pub taken: Option<String>,
    /// Shutter, aperture, ISO and focal length, as one line.
    pub exposure: Option<String>,
    /// The picture says where it was taken. The place itself is only given when asked for.
    pub has_location: bool,
    pub location: Option<Place>,
}

impl ImageInfo {
    fn is_empty(&self) -> bool {
        self.width.is_none()
            && self.camera.is_none()
            && self.lens.is_none()
            && self.taken.is_none()
            && self.exposure.is_none()
            && !self.has_location
    }
}

/// The details of the picture at `path`, or `None` if it is not one Folio can read (not a
/// picture, damaged, or with nothing to say). `with_location` asks for the place too; without it
/// the answer only says whether there is one.
pub fn read(path: &Path, with_location: bool) -> Option<ImageInfo> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut info = ImageInfo::default();
    let size = imagesize::size(path).ok();
    if let Some(s) = &size {
        info.width = u32::try_from(s.width).ok();
        info.height = u32::try_from(s.height).ok();
    }
    if let Ok(file) = File::open(path) {
        let mut prefix = Vec::new();
        if file.take(EXIF_PREFIX).read_to_end(&mut prefix).is_ok()
            && let Ok(exif) = Reader::new().read_from_container(&mut Cursor::new(prefix))
        {
            fill(&mut info, &exif, with_location);
        }
    }
    (!info.is_empty()).then_some(info)
}

fn fill(info: &mut ImageInfo, exif: &exif::Exif, with_location: bool) {
    let text = |tag| {
        let field = exif.get_field(tag, In::PRIMARY)?;
        match &field.value {
            Value::Ascii(parts) => clean(&String::from_utf8_lossy(parts.first()?)),
            _ => None,
        }
    };
    let make = text(Tag::Make);
    let model = text(Tag::Model);
    info.camera = match (make, model) {
        // Many models already start with the make ("Canon EOS R5" under "Canon").
        (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
            Some(model)
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (one, other) => one.or(other),
    };
    info.lens = text(Tag::LensModel);
    info.taken = text(Tag::DateTimeOriginal)
        .or_else(|| text(Tag::DateTime))
        .map(|t| date(&t));
    info.exposure = exposure(exif);

    // A picture stored on its side is shown upright, so its size is the other way round.
    let turned = exif
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .is_some_and(|o| (5..=8).contains(&o));
    if turned {
        std::mem::swap(&mut info.width, &mut info.height);
    }
    // EXIF's own pixel size serves a format whose header gave none.
    if info.width.is_none() {
        let dim = |tag| {
            exif.get_field(tag, In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
        };
        info.width = dim(Tag::PixelXDimension);
        info.height = dim(Tag::PixelYDimension);
    }

    if let Some(place) = location(exif) {
        info.has_location = true;
        if with_location {
            info.location = Some(place);
        }
    }
}

/// Text from a picture: control characters out, one line, not too long; `None` if nothing is left.
fn clean(raw: &str) -> Option<String> {
    let text: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then(|| text.chars().take(FIELD_LIMIT).collect())
}

/// EXIF writes `2026:11:04 09:12:00`; people read `2026-11-04 09:12:00`.
fn date(raw: &str) -> String {
    // Only a plain ASCII start is cut up: the text is the picture's own, and may be anything.
    match raw.get(..10) {
        Some(head)
            if head.is_ascii() && head.as_bytes()[4] == b':' && head.as_bytes()[7] == b':' =>
        {
            format!(
                "{}-{}-{}{}",
                &head[..4],
                &head[5..7],
                &head[8..10],
                &raw[10..]
            )
        }
        _ => raw.to_owned(),
    }
}

/// A rational field as a number, if it has one and its denominator is not zero.
fn rational(exif: &exif::Exif, tag: Tag) -> Option<f64> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(v) => v
            .first()
            .filter(|r| r.denom != 0)
            .map(exif::Rational::to_f64),
        _ => None,
    }
}

/// `1/250 s · f/2.8 · ISO 100 · 35 mm`, with what the picture has.
fn exposure(exif: &exif::Exif) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(t) = rational(exif, Tag::ExposureTime).filter(|t| *t > 0.0) {
        parts.push(if t < 1.0 {
            format!("1/{} s", (1.0 / t).round())
        } else {
            format!("{} s", trim(t))
        });
    }
    if let Some(f) = rational(exif, Tag::FNumber).filter(|f| *f > 0.0) {
        parts.push(format!("f/{}", trim(f)));
    }
    if let Some(iso) = exif
        .get_field(Tag::PhotographicSensitivity, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .filter(|iso| *iso > 0)
    {
        parts.push(format!("ISO {iso}"));
    }
    if let Some(mm) = rational(exif, Tag::FocalLength).filter(|mm| *mm > 0.0) {
        parts.push(format!("{} mm", trim(mm)));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// A number with at most one decimal, and none if it is whole.
fn trim(n: f64) -> String {
    let one = (n * 10.0).round() / 10.0;
    if (one - one.round()).abs() < f64::EPSILON {
        format!("{one:.0}")
    } else {
        format!("{one:.1}")
    }
}

/// Degrees, minutes and seconds with their hemisphere, as decimal degrees.
fn coordinate(exif: &exif::Exif, value: Tag, reference: Tag, negative: u8) -> Option<f64> {
    let Value::Rational(parts) = &exif.get_field(value, In::PRIMARY)?.value else {
        return None;
    };
    if parts.len() < 3 || parts.iter().any(|r| r.denom == 0) {
        return None;
    }
    let degrees = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
    let south_or_west = match &exif.get_field(reference, In::PRIMARY)?.value {
        Value::Ascii(v) => v.first().and_then(|s| s.first()).copied() == Some(negative),
        _ => return None,
    };
    Some(if south_or_west { -degrees } else { degrees })
}

/// Where the picture was taken, if it says so and the numbers make sense.
fn location(exif: &exif::Exif) -> Option<Place> {
    let lat = coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S')?;
    let lon = coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W')?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some(Place { lat, lon })
}

#[cfg(test)]
mod tests {
    use super::*;
    use exif::experimental::Writer;
    use exif::{Field, Rational};
    use std::io::Cursor;

    fn ascii(tag: Tag, text: &str) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![text.as_bytes().to_vec()]),
        }
    }

    fn rationals(tag: Tag, values: &[(u32, u32)]) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(
                values
                    .iter()
                    .map(|&(num, denom)| Rational { num, denom })
                    .collect(),
            ),
        }
    }

    fn short(tag: Tag, n: u16) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Short(vec![n]),
        }
    }

    /// A JPEG of `w` x `h` with these EXIF fields in an APP1 segment; no picture data, which
    /// only the header readers need.
    fn jpeg(w: u16, h: u16, fields: &[Field]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        if !fields.is_empty() {
            let mut writer = Writer::new();
            for f in fields {
                writer.push_field(f);
            }
            let mut tiff = Cursor::new(Vec::new());
            writer.write(&mut tiff, false).unwrap();
            let tiff = tiff.into_inner();
            let len = u16::try_from(2 + 6 + tiff.len()).unwrap();
            out.extend([0xFF, 0xE1]);
            out.extend(len.to_be_bytes());
            out.extend(b"Exif\0\0");
            out.extend(tiff);
        }
        // Start of frame: 8 bits, h x w, three components.
        out.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        out.extend(h.to_be_bytes());
        out.extend(w.to_be_bytes());
        out.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        out.extend([0xFF, 0xD9]);
        out
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn camera_fields() -> Vec<Field> {
        vec![
            ascii(Tag::Make, "Canon"),
            ascii(Tag::Model, "Canon EOS R5"),
            ascii(Tag::LensModel, "RF24-70mm F2.8 L IS USM"),
            ascii(Tag::DateTimeOriginal, "2026:11:04 09:12:30"),
            rationals(Tag::ExposureTime, &[(1, 250)]),
            rationals(Tag::FNumber, &[(28, 10)]),
            short(Tag::PhotographicSensitivity, 100),
            rationals(Tag::FocalLength, &[(35, 1)]),
        ]
    }

    #[test]
    fn a_jpeg_with_exif_gives_its_size_camera_lens_date_and_exposure() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "a.jpg", &jpeg(6000, 4000, &camera_fields()));
        let info = read(&p, false).unwrap();
        assert_eq!((info.width, info.height), (Some(6000), Some(4000)));
        assert_eq!(info.camera.as_deref(), Some("Canon EOS R5"));
        assert_eq!(info.lens.as_deref(), Some("RF24-70mm F2.8 L IS USM"));
        assert_eq!(info.taken.as_deref(), Some("2026-11-04 09:12:30"));
        assert_eq!(
            info.exposure.as_deref(),
            Some("1/250 s · f/2.8 · ISO 100 · 35 mm")
        );
        assert!(!info.has_location && info.location.is_none());
    }

    #[test]
    fn make_and_model_are_joined_unless_the_model_already_names_the_make() {
        let dir = tempfile::tempdir().unwrap();
        let fields = [ascii(Tag::Make, "FUJIFILM"), ascii(Tag::Model, "X-T5")];
        let p = write(dir.path(), "f.jpg", &jpeg(10, 10, &fields));
        assert_eq!(
            read(&p, false).unwrap().camera.as_deref(),
            Some("FUJIFILM X-T5")
        );
        let only = [ascii(Tag::Model, "Pixel 8")];
        let p = write(dir.path(), "g.jpg", &jpeg(10, 10, &only));
        assert_eq!(read(&p, false).unwrap().camera.as_deref(), Some("Pixel 8"));
    }

    #[test]
    fn a_jpeg_without_exif_gives_only_its_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "plain.jpg", &jpeg(640, 480, &[]));
        let info = read(&p, true).unwrap();
        assert_eq!(
            info,
            ImageInfo {
                width: Some(640),
                height: Some(480),
                ..ImageInfo::default()
            }
        );
    }

    #[test]
    fn a_png_gives_its_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("p.png");
        image::RgbImage::new(5, 3).save(&p).unwrap();
        let info = read(&p, false).unwrap();
        assert_eq!((info.width, info.height), (Some(5), Some(3)));
        assert!(info.camera.is_none() && info.taken.is_none());
    }

    #[test]
    fn a_picture_stored_on_its_side_is_sized_as_it_is_shown() {
        let dir = tempfile::tempdir().unwrap();
        let on_side = [short(Tag::Orientation, 6)];
        let p = write(dir.path(), "r.jpg", &jpeg(4000, 3000, &on_side));
        let info = read(&p, false).unwrap();
        assert_eq!((info.width, info.height), (Some(3000), Some(4000)));
        let upright = [short(Tag::Orientation, 1)];
        let p = write(dir.path(), "u.jpg", &jpeg(4000, 3000, &upright));
        let info = read(&p, false).unwrap();
        assert_eq!((info.width, info.height), (Some(4000), Some(3000)));
    }

    fn gps_fields() -> Vec<Field> {
        vec![
            ascii(Tag::GPSLatitudeRef, "S"),
            rationals(Tag::GPSLatitude, &[(33, 1), (51, 1), (0, 1)]),
            ascii(Tag::GPSLongitudeRef, "E"),
            rationals(Tag::GPSLongitude, &[(151, 1), (12, 1), (36, 1)]),
        ]
    }

    #[test]
    fn the_place_is_only_given_when_asked_for_but_its_existence_always_is() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "gps.jpg", &jpeg(100, 100, &gps_fields()));
        let hidden = read(&p, false).unwrap();
        assert!(hidden.has_location);
        assert!(hidden.location.is_none(), "not unless asked");
        let shown = read(&p, true).unwrap();
        let place = shown.location.unwrap();
        assert!((place.lat - -33.85).abs() < 1e-6, "{}", place.lat);
        assert!((place.lon - 151.21).abs() < 1e-6, "{}", place.lon);
        // The serialized answer of the unasked-for case carries no coordinates at all.
        let json = serde_json::to_string(&hidden).unwrap();
        assert!(
            !json.contains("151") && json.contains("\"hasLocation\":true"),
            "{json}"
        );
    }

    #[test]
    fn a_place_that_makes_no_sense_is_not_a_place() {
        let dir = tempfile::tempdir().unwrap();
        let bad = [
            ascii(Tag::GPSLatitudeRef, "N"),
            rationals(Tag::GPSLatitude, &[(200, 1), (0, 1), (0, 1)]),
            ascii(Tag::GPSLongitudeRef, "E"),
            rationals(Tag::GPSLongitude, &[(10, 1), (0, 1), (0, 1)]),
        ];
        let p = write(dir.path(), "bad.jpg", &jpeg(10, 10, &bad));
        let info = read(&p, true).unwrap();
        assert!(!info.has_location && info.location.is_none());
        // A zero denominator is not a number.
        let zero = [
            ascii(Tag::GPSLatitudeRef, "N"),
            rationals(Tag::GPSLatitude, &[(1, 0), (0, 1), (0, 1)]),
            ascii(Tag::GPSLongitudeRef, "E"),
            rationals(Tag::GPSLongitude, &[(10, 1), (0, 1), (0, 1)]),
        ];
        let p = write(dir.path(), "zero.jpg", &jpeg(10, 10, &zero));
        assert!(!read(&p, true).unwrap().has_location);
    }

    #[test]
    fn exposure_is_written_the_way_photographers_write_it() {
        let dir = tempfile::tempdir().unwrap();
        let long = [
            rationals(Tag::ExposureTime, &[(5, 2)]),
            rationals(Tag::FNumber, &[(4, 1)]),
            rationals(Tag::FocalLength, &[(185, 10)]),
        ];
        let p = write(dir.path(), "l.jpg", &jpeg(10, 10, &long));
        assert_eq!(
            read(&p, false).unwrap().exposure.as_deref(),
            Some("2.5 s · f/4 · 18.5 mm")
        );
        // A zero denominator or a zero value says nothing.
        let none = [
            rationals(Tag::ExposureTime, &[(1, 0)]),
            rationals(Tag::FNumber, &[(0, 1)]),
        ];
        let p = write(dir.path(), "n.jpg", &jpeg(10, 10, &none));
        assert!(read(&p, false).unwrap().exposure.is_none());
    }

    #[test]
    fn text_from_a_picture_is_one_short_line() {
        let dir = tempfile::tempdir().unwrap();
        let wild = [
            ascii(
                Tag::Make,
                &format!("Acme\n\u{7}\u{1b}[31m {}", "x".repeat(500)),
            ),
            ascii(Tag::Model, "   "),
        ];
        let p = write(dir.path(), "w.jpg", &jpeg(10, 10, &wild));
        let camera = read(&p, false).unwrap().camera.unwrap();
        assert!(camera.chars().count() <= FIELD_LIMIT);
        assert!(!camera.chars().any(char::is_control), "{camera:?}");
        assert!(camera.starts_with("Acme"));
    }

    #[test]
    fn a_file_that_cannot_be_read_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        // Not a picture at all, with a picture's name.
        let text = write(dir.path(), "fake.jpg", b"this is not a jpeg, just words");
        assert_eq!(read(&text, true), None);
        // Empty.
        assert_eq!(read(&write(dir.path(), "empty.png", b""), true), None);
        // Damaged: the start of a real picture, cut inside the EXIF segment.
        let whole = jpeg(6000, 4000, &camera_fields());
        let cut = write(dir.path(), "cut.jpg", &whole[..30]);
        // Whatever survives is fine, and nothing may panic or error.
        let _ = read(&cut, true);
        // Random bytes after a valid signature.
        let mut junk = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
        junk.extend((0..200u8).rev());
        let _ = read(&write(dir.path(), "junk.jpg", &junk), true);
        // A folder and a missing file.
        assert_eq!(read(dir.path(), true), None);
        assert_eq!(read(&dir.path().join("nope.jpg"), true), None);
    }

    #[test]
    fn a_damaged_exif_keeps_the_size_the_header_gave() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = jpeg(800, 600, &camera_fields());
        // Break the TIFF header inside the APP1 segment, leaving the frame header whole.
        let at = bytes.windows(6).position(|w| w == b"Exif\0\0").unwrap() + 6;
        bytes[at] = 0;
        bytes[at + 1] = 0;
        let info = read(&write(dir.path(), "d.jpg", &bytes), true).unwrap();
        assert_eq!((info.width, info.height), (Some(800), Some(600)));
        assert!(info.camera.is_none() && info.taken.is_none());
    }

    /// A WebP of `w` x `h` (extended format) holding these EXIF fields; no picture data.
    fn webp(w: u32, h: u32, fields: &[Field]) -> Vec<u8> {
        let mut tiff = Vec::new();
        if !fields.is_empty() {
            let mut writer = Writer::new();
            for f in fields {
                writer.push_field(f);
            }
            let mut out = Cursor::new(Vec::new());
            writer.write(&mut out, false).unwrap();
            tiff = out.into_inner();
        }
        let exif_len = u32::try_from(tiff.len()).unwrap();
        if tiff.len() % 2 == 1 {
            tiff.push(0);
        }
        let mut body = b"WEBP".to_vec();
        // VP8X: the EXIF flag (0x08) if there is any, reserved, then width - 1 and height - 1.
        body.extend(b"VP8X");
        body.extend(10u32.to_le_bytes());
        body.extend([if fields.is_empty() { 0 } else { 0x08 }, 0, 0, 0]);
        body.extend((w - 1).to_le_bytes().iter().take(3));
        body.extend((h - 1).to_le_bytes().iter().take(3));
        if !fields.is_empty() {
            body.extend(b"EXIF");
            body.extend(exif_len.to_le_bytes());
            body.extend(tiff);
        }
        let mut out = b"RIFF".to_vec();
        out.extend(u32::try_from(body.len()).unwrap().to_le_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn a_webp_gives_its_size_and_exif_too() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "a.webp", &webp(1200, 800, &camera_fields()));
        let info = read(&p, false).unwrap();
        assert_eq!((info.width, info.height), (Some(1200), Some(800)));
        assert_eq!(info.camera.as_deref(), Some("Canon EOS R5"));
        assert_eq!(info.taken.as_deref(), Some("2026-11-04 09:12:30"));
        let plain = write(dir.path(), "b.webp", &webp(30, 20, &[]));
        // No EXIF chunk content: the size alone, from the header.
        let info = read(&plain, false).unwrap();
        assert_eq!((info.width, info.height), (Some(30), Some(20)));
    }

    #[test]
    fn a_date_that_is_not_plain_text_is_kept_as_it_is_and_costs_nothing_else() {
        assert_eq!(date("2026:11:04 09:12:30"), "2026-11-04 09:12:30");
        // A multibyte character where the cut would fall must not panic.
        assert_eq!(date("2026:11:0\u{e9} 09:00:00"), "2026:11:0\u{e9} 09:00:00");
        assert_eq!(
            date("\u{20ac}\u{20ac}\u{20ac}\u{20ac}"),
            "\u{20ac}\u{20ac}\u{20ac}\u{20ac}"
        );
        assert_eq!(date("short"), "short");
        let dir = tempfile::tempdir().unwrap();
        let fields = [
            ascii(Tag::DateTimeOriginal, "2026:11:0\u{e9} 09:00:00"),
            ascii(Tag::Make, "Canon"),
        ];
        let p = write(dir.path(), "d.jpg", &jpeg(10, 10, &fields));
        let info = read(&p, false).unwrap();
        assert_eq!(
            info.camera.as_deref(),
            Some("Canon"),
            "the rest is still read"
        );
        assert_eq!(info.width, Some(10));
    }
}
