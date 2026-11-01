//! Picture tools: rotate, make smaller and crop. Each builds the edited file in memory from
//! the original and hands the bytes back; writing them (as a copy, or in place of the
//! original) is the caller's job, so nothing here touches a file on disk but to read it.
//!
//! JPEG is turned without re-encoding when it can be: the turn is written into the file's
//! EXIF orientation, which every viewer applies, and the picture data stays byte for byte
//! the same. Everything else is decoded, edited and encoded again.

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::fs;
use std::io::{self, Cursor, ErrorKind};
use std::path::Path;

use super::progress;

/// The largest picture file that is opened.
pub const MAX_FILE_BYTES: u64 = 256 << 20;
/// Pixels along either side beyond which a picture is not decoded.
const MAX_SIDE: u32 = 20_000;
/// Most pixels in a picture that is edited.
const MAX_PIXELS: u64 = 100_000_000;
/// JPEG quality of a cropped or turned-by-pixels picture.
const KEEP_QUALITY: u8 = 92;

/// The formats that can be edited. (WebP is not: it can only be written back losslessly,
/// which makes a lossy file several times bigger.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    Bmp,
}

impl Format {
    /// The format a file name says, if it is one of those that can be edited. (GIF would
    /// lose its animation, and the rest are not pictures to edit this way.)
    pub fn of(path: &Path) -> Option<Self> {
        match path
            .extension()?
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "bmp" => Some(Self::Bmp),
            _ => None,
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Png => ImageFormat::Png,
            Self::Bmp => ImageFormat::Bmp,
        }
    }
}

/// How far to shrink a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    High,
    Balanced,
    Small,
    Smallest,
}

impl Preset {
    /// JPEG quality (1 to 100) and the scale the sides are shrunk to. Formats without a
    /// quality setting shrink only by the scale.
    fn params(self) -> (u8, f64) {
        match self {
            Self::High => (85, 1.0),
            Self::Balanced => (70, 1.0),
            Self::Small => (55, 0.75),
            Self::Smallest => (40, 0.5),
        }
    }
}

/// One edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Edit {
    /// Quarter turns clockwise; negative for counter-clockwise.
    Rotate {
        turns: i8,
    },
    Compress {
        preset: Preset,
    },
    /// A rectangle of the picture as it is shown (EXIF orientation applied), in pixels.
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

fn invalid(what: impl std::fmt::Display) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, what.to_string())
}

/// The edited picture as bytes of the same format.
pub fn apply(path: &Path, edit: Edit) -> io::Result<Vec<u8>> {
    let format = Format::of(path).ok_or_else(|| invalid("This kind of picture can't be edited"))?;
    if fs::metadata(path)?.len() > MAX_FILE_BYTES {
        return Err(invalid("This picture is too big to edit (over 256 MB)"));
    }
    // Reading a placeholder would download it from the cloud.
    if crate::cleanup::is_cloud_only(path) {
        return Err(invalid(
            "This picture is only in the cloud. Make it available on this computer first.",
        ));
    }
    let bytes = fs::read(path)?;
    progress::checkpoint()?;
    if is_animated(&bytes, format) {
        return Err(invalid(
            "This picture is animated; editing it would keep only one frame",
        ));
    }
    if let (Format::Jpeg, Edit::Rotate { turns }) = (format, edit)
        && let Some(turned) = jpeg_turned(&bytes, turns)
    {
        return Ok(turned);
    }
    let mut picture = decode(&bytes, format)?;
    progress::checkpoint()?;
    let (quality, must_shrink_from) = match edit {
        Edit::Rotate { turns } => {
            picture = match turns.rem_euclid(4) {
                1 => picture.rotate90(),
                2 => picture.rotate180(),
                3 => picture.rotate270(),
                _ => picture,
            };
            (KEEP_QUALITY, None)
        }
        Edit::Crop {
            x,
            y,
            width,
            height,
        } => {
            let inside = width > 0
                && height > 0
                && x.checked_add(width).is_some_and(|r| r <= picture.width())
                && y.checked_add(height).is_some_and(|b| b <= picture.height());
            if !inside {
                return Err(invalid("The crop is outside the picture"));
            }
            picture = picture.crop_imm(x, y, width, height);
            (KEEP_QUALITY, None)
        }
        Edit::Compress { preset } => {
            let (quality, scale) = preset.params();
            if scale < 1.0 {
                picture = picture.resize_exact(
                    scaled(picture.width(), scale),
                    scaled(picture.height(), scale),
                    FilterType::Triangle,
                );
            }
            (quality, Some(bytes.len()))
        }
    };
    progress::checkpoint()?;
    let encoded = encode(&picture, format, quality)?;
    if let Some(before) = must_shrink_from
        && encoded.len() >= before
    {
        return Err(invalid(
            "This picture can't be made smaller at this setting. Try a stronger one.",
        ));
    }
    Ok(encoded)
}

/// Whether a PNG holds more than one frame (APNG). JPEG and BMP never do.
fn is_animated(bytes: &[u8], format: Format) -> bool {
    let head = &bytes[..bytes.len().min(1 << 16)];
    let has = |tag: &[u8; 4]| head.windows(4).any(|w| w == tag);
    match format {
        // The animation control chunk comes before the image data.
        Format::Png => has(b"acTL"),
        Format::Jpeg | Format::Bmp => false,
    }
}

/// `n` pixels at `scale`, at least one.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a pixel count times a factor between 0 and 1 fits u32 and is not negative"
)]
fn scaled(n: u32, scale: f64) -> u32 {
    ((f64::from(n) * scale).round() as u32).max(1)
}

/// The picture as it is shown: decoded, with its EXIF orientation applied.
fn decode(bytes: &[u8], format: Format) -> io::Result<DynamicImage> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format.image_format());
    let mut limits = Limits::default();
    limits.max_alloc = Some(1 << 30);
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| invalid(format!("This picture can't be read: {e}")))?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(invalid(
            "This picture is too large to edit (over 100 megapixels)",
        ));
    }
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder)
        .map_err(|e| invalid(format!("This picture can't be read: {e}")))?;
    if let Some(orientation) = orientation {
        picture.apply_orientation(orientation);
    }
    Ok(picture)
}

fn encode(picture: &DynamicImage, format: Format, quality: u8) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let result = match format {
        Format::Jpeg => {
            let encoder = JpegEncoder::new_with_quality(&mut out, quality);
            if picture.color().has_color() {
                picture.to_rgb8().write_with_encoder(encoder)
            } else {
                picture.to_luma8().write_with_encoder(encoder)
            }
        }
        Format::Png => picture.write_with_encoder(PngEncoder::new_with_quality(
            &mut out,
            CompressionType::Best,
            PngFilter::Adaptive,
        )),
        Format::Bmp => picture.write_to(&mut Cursor::new(&mut out), ImageFormat::Bmp),
    };
    result.map_err(|e| io::Error::other(e.to_string()))?;
    Ok(out)
}

// ---- Lossless JPEG turns ----

/// The EXIF orientation after `turns` more quarter turns clockwise, for each value 1 to 8
/// (index 0 is unused). One turn clockwise: 1→6→3→8→1, and mirrored 2→7→4→5→2.
const ONE_TURN: [u8; 9] = [0, 6, 7, 8, 5, 2, 3, 4, 1];

fn turned_orientation(mut current: u8, turns: i8) -> u8 {
    for _ in 0..turns.rem_euclid(4) {
        current = ONE_TURN[usize::from(current)];
    }
    current
}

/// Where a JPEG's EXIF orientation value is stored.
struct Slot {
    /// Offset of the two bytes of the value.
    at: usize,
    big_endian: bool,
    value: u8,
}

/// What a JPEG says about its orientation.
enum Exif {
    /// An orientation tag that can be rewritten in place.
    Slot(Slot),
    /// No EXIF segment at all: one can be added.
    None,
    /// EXIF that has no orientation tag, or that isn't understood: left alone.
    Other,
}

fn u16_at(b: &[u8], at: usize, big_endian: bool) -> Option<u16> {
    let pair: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
    Some(if big_endian {
        u16::from_be_bytes(pair)
    } else {
        u16::from_le_bytes(pair)
    })
}

fn u32_at(b: &[u8], at: usize, big_endian: bool) -> Option<u32> {
    let quad: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    Some(if big_endian {
        u32::from_be_bytes(quad)
    } else {
        u32::from_le_bytes(quad)
    })
}

fn find_exif(jpeg: &[u8]) -> Exif {
    if jpeg.get(..2) != Some(&[0xFF, 0xD8]) {
        return Exif::Other;
    }
    let mut i = 2;
    while jpeg.get(i) == Some(&0xFF) {
        let Some(&marker) = jpeg.get(i + 1) else {
            return Exif::Other;
        };
        // Fill bytes may come before a marker.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        // Start of scan or end of image: the headers are over, and there was no EXIF.
        if marker == 0xDA || marker == 0xD9 {
            return Exif::None;
        }
        // Markers without a length.
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            i += 2;
            continue;
        }
        let Some(len) = u16_at(jpeg, i + 2, true).map(usize::from) else {
            return Exif::Other;
        };
        if len < 2 {
            return Exif::Other;
        }
        let body = i + 4;
        let end = i + 2 + len;
        if marker == 0xE1 && jpeg.get(body..body + 6) == Some(b"Exif\0\0") {
            return exif_slot(jpeg, body + 6, end).map_or(Exif::Other, Exif::Slot);
        }
        i = end;
    }
    // Anything that stopped the walk short of the picture data is not understood.
    Exif::Other
}

/// The orientation tag in the TIFF block at `tiff..end`.
fn exif_slot(jpeg: &[u8], tiff: usize, end: usize) -> Option<Slot> {
    let big_endian = match jpeg.get(tiff..tiff + 2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    if u16_at(jpeg, tiff + 2, big_endian)? != 42 {
        return None;
    }
    let ifd = tiff.checked_add(usize::try_from(u32_at(jpeg, tiff + 4, big_endian)?).ok()?)?;
    let count = usize::from(u16_at(jpeg, ifd, big_endian)?);
    for n in 0..count {
        let entry = ifd.checked_add(2 + 12 * n)?;
        if entry.checked_add(12)? > end {
            return None;
        }
        if u16_at(jpeg, entry, big_endian)? == 0x0112 {
            // A SHORT, once.
            if u16_at(jpeg, entry + 2, big_endian)? != 3
                || u32_at(jpeg, entry + 4, big_endian)? != 1
            {
                return None;
            }
            let value = u8::try_from(u16_at(jpeg, entry + 8, big_endian)?).ok()?;
            if !(1..=8).contains(&value) {
                return None;
            }
            return Some(Slot {
                at: entry + 8,
                big_endian,
                value,
            });
        }
    }
    None
}

/// The JPEG with its orientation turned by `turns` quarter turns clockwise and nothing else
/// changed, or `None` when that can't be done without touching the picture data.
fn jpeg_turned(jpeg: &[u8], turns: i8) -> Option<Vec<u8>> {
    if turns.rem_euclid(4) == 0 {
        return Some(jpeg.to_vec());
    }
    match find_exif(jpeg) {
        Exif::Slot(slot) => {
            let new = turned_orientation(slot.value, turns);
            let mut out = jpeg.to_vec();
            let pair = if slot.big_endian {
                u16::from(new).to_be_bytes()
            } else {
                u16::from(new).to_le_bytes()
            };
            out[slot.at..slot.at + 2].copy_from_slice(&pair);
            Some(out)
        }
        Exif::None => {
            let new = turned_orientation(1, turns);
            // APP1, 34 bytes: length, "Exif\0\0", a little-endian TIFF header whose one
            // entry is the orientation.
            let mut segment = vec![0xFF, 0xE1, 0x00, 0x22];
            segment.extend_from_slice(b"Exif\0\0II*\0\x08\0\0\0\x01\0");
            segment.extend_from_slice(&[0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00]);
            segment.extend_from_slice(&[new, 0x00, 0x00, 0x00]);
            segment.extend_from_slice(&[0x00; 4]);
            let mut out = Vec::with_capacity(jpeg.len() + segment.len());
            out.extend_from_slice(&jpeg[..2]);
            out.extend_from_slice(&segment);
            out.extend_from_slice(&jpeg[2..]);
            Some(out)
        }
        Exif::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32) -> DynamicImage {
        let mut seed = 7u32;
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |_, _| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let b = seed.to_le_bytes();
            image::Rgb([b[1], b[2], b[3]])
        }))
    }

    fn save(dir: &Path, name: &str, format: Format, w: u32, h: u32) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, encode(&picture(w, h), format, 90).unwrap()).unwrap();
        path
    }

    fn size_of(bytes: &[u8], format: Format) -> (u32, u32) {
        let p = decode(bytes, format).unwrap();
        (p.width(), p.height())
    }

    #[test]
    fn a_jpeg_is_turned_by_its_orientation_and_its_picture_data_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = save(dir.path(), "a.jpg", Format::Jpeg, 40, 20);
        let original = fs::read(&path).unwrap();
        let once = apply(&path, Edit::Rotate { turns: 1 }).unwrap();
        // Shown 20 wide and 40 tall, with every byte of the original still in the file.
        assert_eq!(size_of(&once, Format::Jpeg), (20, 40));
        assert!(once.ends_with(&original[2..]));
        // Turning again edits the tag in place: the file does not grow.
        fs::write(&path, &once).unwrap();
        let twice = apply(&path, Edit::Rotate { turns: 1 }).unwrap();
        assert_eq!(twice.len(), once.len());
        assert_eq!(size_of(&twice, Format::Jpeg), (40, 20));
        // Four turns bring it back to how it was drawn.
        fs::write(&path, &twice).unwrap();
        let three = apply(&path, Edit::Rotate { turns: -1 }).unwrap();
        assert_eq!(size_of(&three, Format::Jpeg), (20, 40));
        fs::write(&path, &three).unwrap();
        let back = apply(&path, Edit::Rotate { turns: -1 }).unwrap();
        assert_eq!(size_of(&back, Format::Jpeg), (40, 20));
    }

    /// A JPEG with the bytes of an EXIF segment already in it, with or without orientation.
    fn jpeg_with_exif(with_orientation: bool) -> Vec<u8> {
        let plain = encode(&picture(16, 8), Format::Jpeg, 90).unwrap();
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        // One entry: orientation 1, or the image width (tag 0x0100) instead.
        tiff.extend_from_slice(&[1, 0]);
        let tag: u16 = if with_orientation { 0x0112 } else { 0x0100 };
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&[3, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        let mut segment = vec![0xFF, 0xE1];
        let len = u16::try_from(2 + 6 + tiff.len()).unwrap();
        segment.extend_from_slice(&len.to_be_bytes());
        segment.extend_from_slice(b"Exif\0\0");
        segment.extend_from_slice(&tiff);
        let mut out = plain[..2].to_vec();
        out.extend_from_slice(&segment);
        out.extend_from_slice(&plain[2..]);
        out
    }

    #[test]
    fn exif_without_an_orientation_is_not_given_a_second_block() {
        let jpeg = jpeg_with_exif(false);
        // Left to the slower way, which the caller then takes; never a second APP1.
        assert!(jpeg_turned(&jpeg, 1).is_none());
        // With an orientation it is edited in place and the file keeps its size.
        let with = jpeg_with_exif(true);
        let turned = jpeg_turned(&with, 1).unwrap();
        assert_eq!(turned.len(), with.len());
        assert_ne!(turned, with);
    }

    #[test]
    fn fill_bytes_before_a_marker_do_not_hide_the_exif() {
        let mut jpeg = jpeg_with_exif(true);
        jpeg.insert(2, 0xFF);
        assert!(matches!(find_exif(&jpeg), Exif::Slot(_)));
    }

    #[test]
    fn damaged_headers_never_panic_and_never_gain_a_second_exif_block() {
        let good = jpeg_with_exif(true);
        for cut in 0..good.len().min(200) {
            let _ = jpeg_turned(&good[..cut], 1);
        }
        for at in 0..good.len().min(200) {
            let mut bent = good.clone();
            bent[at] ^= 0xFF;
            if let Some(out) = jpeg_turned(&bent, 1) {
                assert!(out.len() == bent.len() || out.len() == bent.len() + 36);
            }
        }
    }

    #[test]
    fn an_animated_png_is_refused() {
        assert!(is_animated(b"\x89PNG....acTL....", Format::Png));
        assert!(!is_animated(b"\x89PNG....IDAT....", Format::Png));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        let mut bytes = encode(&picture(8, 8), Format::Png, 90).unwrap();
        // Chunk lengths and checksums are not checked by the tag test.
        bytes.splice(33..33, *b"acTL");
        fs::write(&path, bytes).unwrap();
        let e = apply(&path, Edit::Rotate { turns: 1 }).unwrap_err();
        assert!(e.to_string().contains("animated"), "{e}");
    }

    #[test]
    fn orientation_turns_follow_the_exif_table() {
        assert_eq!(turned_orientation(1, 1), 6);
        assert_eq!(turned_orientation(1, -1), 8);
        assert_eq!(turned_orientation(1, 2), 3);
        assert_eq!(turned_orientation(6, 1), 3);
        assert_eq!(turned_orientation(2, 1), 7);
        assert_eq!(turned_orientation(5, 4), 5);
        assert_eq!(turned_orientation(3, -2), 1);
    }

    #[test]
    fn a_png_is_turned_by_pixels_and_keeps_its_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = save(dir.path(), "a.png", Format::Png, 30, 10);
        let turned = apply(&path, Edit::Rotate { turns: 1 }).unwrap();
        assert_eq!(size_of(&turned, Format::Png), (10, 30));
        assert!(turned.starts_with(b"\x89PNG"));
    }

    #[test]
    fn a_crop_keeps_exactly_the_rectangle_and_one_outside_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = save(dir.path(), "a.png", Format::Png, 50, 40);
        let crop = |x, y, width, height| {
            apply(
                &path,
                Edit::Crop {
                    x,
                    y,
                    width,
                    height,
                },
            )
        };
        let cut = crop(10, 5, 20, 15).unwrap();
        assert_eq!(size_of(&cut, Format::Png), (20, 15));
        // The pixels are the ones that were there.
        let (whole, part) = (
            decode(&fs::read(&path).unwrap(), Format::Png)
                .unwrap()
                .to_rgb8(),
            decode(&cut, Format::Png).unwrap().to_rgb8(),
        );
        assert_eq!(part.get_pixel(0, 0), whole.get_pixel(10, 5));
        assert_eq!(part.get_pixel(19, 14), whole.get_pixel(29, 19));
        assert!(crop(40, 0, 20, 10).is_err());
        assert!(crop(0, 0, 0, 10).is_err());
        assert!(crop(u32::MAX, 0, 2, 2).is_err());
    }

    #[test]
    fn a_crop_is_taken_from_the_picture_as_shown() {
        let dir = tempfile::tempdir().unwrap();
        let path = save(dir.path(), "a.jpg", Format::Jpeg, 40, 20);
        // Shown as 20 wide and 40 tall.
        fs::write(&path, apply(&path, Edit::Rotate { turns: 1 }).unwrap()).unwrap();
        let cut = apply(
            &path,
            Edit::Crop {
                x: 0,
                y: 20,
                width: 20,
                height: 20,
            },
        )
        .unwrap();
        assert_eq!(size_of(&cut, Format::Jpeg), (20, 20));
        assert!(
            apply(
                &path,
                Edit::Crop {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 20
                }
            )
            .is_err()
        );
    }

    #[test]
    fn stronger_presets_make_a_jpeg_smaller_and_one_that_cannot_shrink_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = save(dir.path(), "a.jpg", Format::Jpeg, 200, 200);
        let size = |preset| {
            apply(&path, Edit::Compress { preset })
                .map(|b| b.len())
                .unwrap()
        };
        let original = usize::try_from(fs::metadata(&path).unwrap().len()).unwrap();
        assert!(size(Preset::Balanced) < original);
        assert!(size(Preset::Smallest) < size(Preset::Balanced));
        // Saved at the lowest quality already: re-saving at a higher one only grows it.
        let tiny = dir.path().join("t.jpg");
        fs::write(&tiny, encode(&picture(16, 16), Format::Jpeg, 1).unwrap()).unwrap();
        let e = apply(
            &tiny,
            Edit::Compress {
                preset: Preset::High,
            },
        )
        .unwrap_err();
        assert!(e.to_string().contains("can't be made smaller"), "{e}");
    }

    #[test]
    fn a_picture_that_is_not_one_is_refused_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake.jpg");
        fs::write(&path, "not a picture").unwrap();
        assert!(apply(&path, Edit::Rotate { turns: 1 }).is_err());
        let e = apply(
            &path,
            Edit::Crop {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        )
        .unwrap_err();
        assert!(e.to_string().contains("can't be read"), "{e}");
        assert!(apply(&dir.path().join("x.gif"), Edit::Rotate { turns: 1 }).is_err());
    }
}
