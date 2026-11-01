//! PDF tools: join PDFs into one and make a PDF smaller. Both write a new file next to the
//! source and never touch the originals.
//!
//! The result is built in memory, written under a temporary name and only then given its
//! real one, so a stop half-way (cancel, crash, full disk) leaves no broken PDF behind. Both
//! check for pause and cancel between files and images.

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use super::{PARTIAL, progress};

/// Page attributes a page may inherit from the page tree above it. The tree is rebuilt when
/// merging, so they are copied onto each page first.
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Most bytes of PDF that may be read for one job, and the most a stream may inflate to.
pub const MAX_INPUT_BYTES: u64 = 1 << 30;
const MAX_INFLATED: usize = 256 << 20;

/// How far to shrink a PDF. Only JPEG images are touched; text and drawings stay as they
/// are, so a PDF without such images shrinks only by repacking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Repack only: nothing about how it looks changes.
    Lossless,
    High,
    Balanced,
    Small,
    Smallest,
}

impl Preset {
    /// JPEG quality (1 to 100) and the scale images are shrunk to; `None` for no change to
    /// images.
    fn images(self) -> Option<(u8, f32)> {
        match self {
            Self::Lossless => None,
            Self::High => Some((85, 1.0)),
            Self::Balanced => Some((60, 1.0)),
            Self::Small => Some((40, 0.7)),
            Self::Smallest => Some((25, 0.35)),
        }
    }
}

fn invalid(what: impl std::fmt::Display) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, what.to_string())
}

fn load(path: &Path) -> io::Result<Document> {
    let doc =
        Document::load_with_options(path, LoadOptions::with_max_decompressed_size(MAX_INFLATED))
            .map_err(|e| invalid(format!("\"{}\" can't be read: {e}", file_name(path))))?;
    if doc.is_encrypted() {
        return Err(invalid(format!(
            "\"{}\" is password protected; remove the password first",
            file_name(path)
        )));
    }
    Ok(doc)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// Copies the attributes `page` inherits onto the page itself.
fn flatten(doc: &mut Document, page: ObjectId) {
    let mut found: Vec<(&[u8], Object)> = Vec::new();
    let mut at = Some(page);
    // A bound, so a loop in a damaged page tree ends.
    for _ in 0..64 {
        let Some(id) = at else { break };
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        for key in INHERITED {
            if !found.iter().any(|(k, _)| *k == key)
                && let Ok(value) = dict.get(key)
            {
                found.push((key, value.clone()));
            }
        }
        at = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    if let Ok(dict) = doc.get_object_mut(page).and_then(Object::as_dict_mut) {
        for (key, value) in found {
            if !dict.has(key) {
                dict.set(key, value);
            }
        }
    }
}

/// Joins `inputs`, in order, into one PDF.
pub fn merge(inputs: &[PathBuf]) -> io::Result<Vec<u8>> {
    if inputs.len() < 2 {
        return Err(invalid("Choose at least two PDFs to merge"));
    }
    let mut next_id = 1;
    let mut pages: Vec<(ObjectId, Object)> = Vec::new();
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    for path in inputs {
        progress::checkpoint()?;
        let mut doc = load(path)?;
        if has_document_level_content(&doc) {
            return Err(invalid(format!(
                "\"{}\" has attachments, form fields or layers that merging would lose",
                file_name(path)
            )));
        }
        doc.renumber_objects_with(next_id);
        next_id = doc.max_id + 1;
        let ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
        for id in &ids {
            flatten(&mut doc, *id);
        }
        for id in ids {
            let page = doc.get_object(id).map_err(invalid)?.clone();
            pages.push((id, page));
        }
        // Page trees and catalogs of the parts are replaced by one of our own below.
        objects.extend(doc.objects.into_iter().filter(|(_, o)| {
            !matches!(
                o.type_name(),
                Ok(b"Catalog" | b"Pages" | b"Outlines" | b"Outline")
            )
        }));
        if let Some(p) = progress::current() {
            p.add_item();
            p.add_bytes(fs::metadata(path).map_or(0, |m| m.len()));
        }
    }
    progress::checkpoint()?;

    let mut out = Document::with_version("1.7");
    out.objects = objects;
    out.max_id = next_id;
    let pages_id = out.new_object_id();
    let catalog_id = out.new_object_id();
    let mut kids = Vec::with_capacity(pages.len());
    for (id, mut page) in pages {
        if let Ok(dict) = page.as_dict_mut() {
            dict.set("Parent", pages_id);
        }
        kids.push(Object::Reference(id));
        out.objects.insert(id, page);
    }
    let mut tree = Dictionary::new();
    tree.set("Type", "Pages");
    tree.set("Count", i64::try_from(kids.len()).unwrap_or(i64::MAX));
    tree.set("Kids", kids);
    out.objects.insert(pages_id, Object::Dictionary(tree));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", pages_id);
    out.objects.insert(catalog_id, Object::Dictionary(catalog));
    out.trailer.set("Root", catalog_id);
    // Objects of the parts that nothing reaches any more (their outlines' children, say).
    out.prune_objects();
    out.renumber_objects();
    progress::checkpoint()?;
    out.compress();
    progress::checkpoint()?;
    serialize(&mut out)
}

/// `input` made smaller as `preset` says. Fails if the result would not be smaller.
pub fn compress(input: &Path, preset: Preset) -> io::Result<Vec<u8>> {
    let before = fs::metadata(input)?.len();
    let mut doc = load(input)?;
    if let Some(p) = progress::current() {
        // The file is read; the pictures and the rewrite are the rest of the work.
        p.add_bytes(before / 4);
    }
    if let Some((quality, scale)) = preset.images() {
        let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
        for id in ids {
            progress::checkpoint()?;
            if let Some(Object::Stream(stream)) = doc.objects.get_mut(&id) {
                shrink_image(stream, quality, scale);
            }
        }
    }
    progress::checkpoint()?;
    if let Some(p) = progress::current() {
        p.add_bytes(before / 2);
    }
    doc.delete_zero_length_streams();
    doc.prune_objects();
    doc.renumber_objects();
    progress::checkpoint()?;
    doc.compress();
    progress::checkpoint()?;
    let bytes = serialize(&mut doc)?;
    progress::checkpoint()?;
    if (bytes.len() as u64) >= before {
        return Err(invalid(
            "This PDF can't be made smaller at this setting. Try a stronger setting.",
        ));
    }
    Ok(bytes)
}

fn serialize(doc: &mut Document) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    doc.save_modern(&mut bytes)
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes)
}

/// `n` pixels at `scale`, at least one.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a pixel count times a factor between 0 and 1 fits u32 and is not negative"
)]
fn scaled(n: u32, scale: f32) -> u32 {
    ((f64::from(n) * f64::from(scale)).round() as u32).max(1)
}

/// Re-encodes a JPEG image smaller, if it is a plain one and the result is smaller.
/// Anything else (other filters, masks, odd colour spaces) is left exactly as it is.
fn shrink_image(stream: &mut lopdf::Stream, quality: u8, scale: f32) {
    use image::{DynamicImage, ImageFormat, codecs::jpeg::JpegEncoder, imageops::FilterType};

    let dict = &stream.dict;
    let name = |key: &[u8]| dict.get(key).ok().and_then(|o| o.as_name().ok());
    if name(b"Subtype") != Some(b"Image") {
        return;
    }
    let single_filter = match dict.get(b"Filter") {
        Ok(Object::Name(n)) => Some(n.as_slice()),
        Ok(Object::Array(a)) if a.len() == 1 => a[0].as_name().ok(),
        _ => None,
    };
    if single_filter != Some(b"DCTDecode") {
        return;
    }
    let gray = match name(b"ColorSpace") {
        Some(b"DeviceRGB") => false,
        Some(b"DeviceGray") => true,
        _ => return,
    };
    // A decode array flips or remaps values; an image mask is not a picture.
    if dict.has(b"Decode") || dict.has(b"ImageMask") || dict.has(b"Mask") {
        return;
    }
    let Ok(decoded) = image::load_from_memory_with_format(&stream.content, ImageFormat::Jpeg)
    else {
        return;
    };
    let (w, h) = (decoded.width(), decoded.height());
    let (nw, nh) = if scale < 1.0 {
        (scaled(w, scale), scaled(h, scale))
    } else {
        (w, h)
    };
    let resized: DynamicImage = if (nw, nh) == (w, h) {
        decoded
    } else {
        decoded.resize_exact(nw, nh, FilterType::Triangle)
    };
    let mut encoded = Vec::new();
    let result = if gray {
        JpegEncoder::new_with_quality(&mut encoded, quality).encode_image(&resized.to_luma8())
    } else {
        JpegEncoder::new_with_quality(&mut encoded, quality).encode_image(&resized.to_rgb8())
    };
    if result.is_err() || encoded.len() >= stream.content.len() {
        return;
    }
    stream.set_content(encoded);
    stream.dict.set("Width", i64::from(nw));
    stream.dict.set("Height", i64::from(nh));
}

/// Writes `bytes` as `dir/name` (or the next free `name (2).pdf`…) and returns the path.
/// Built under a temporary name of its own first, so a failure leaves nothing at the final
/// name, and only a temporary file this call made is ever removed.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let (temp, mut file) = (0..100)
        .find_map(|n| {
            let temp = dir.join(format!("{name}.{}-{n}{PARTIAL}", std::process::id()));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
            {
                Ok(file) => Some(Ok((temp, file))),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| Err(io::Error::from(ErrorKind::AlreadyExists)))?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    drop(file);
    super::finish_copy(&temp, dir, name)
}

/// Whether a catalog holds things that merging would drop: attachments, form fields and
/// layers (optional content).
fn has_document_level_content(doc: &Document) -> bool {
    let Ok(root) = doc.catalog() else {
        return false;
    };
    root.has(b"AcroForm")
        || root.has(b"OCProperties")
        || root
            .get(b"Names")
            .and_then(|names| doc.dereference(names))
            .ok()
            .and_then(|(_, o)| o.as_dict().ok())
            .is_some_and(|d| d.has(b"EmbeddedFiles"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    /// A PDF of `pages` pages whose first page says `label`; the page size is on the page
    /// tree, not the pages, as many real files have it.
    fn pdf(label: &str, pages: usize) -> Document {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
        });
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let mut kids = Vec::new();
        for n in 0..pages {
            let text = format!("BT /F1 24 Tf 50 700 Td ({label} {n}) Tj ET");
            let content = doc.add_object(Stream::new(dictionary! {}, text.into_bytes()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            });
            kids.push(Object::Reference(page));
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => i64::try_from(pages).unwrap(),
                "Kids" => kids,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc
    }

    fn save(doc: &mut Document, dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        doc.save(&path).unwrap();
        path
    }

    #[test]
    fn merging_keeps_every_page_in_the_order_given_and_what_they_inherit() {
        let dir = tempfile::tempdir().unwrap();
        let a = save(&mut pdf("A", 2), dir.path(), "a.pdf");
        let b = save(&mut pdf("B", 1), dir.path(), "b.pdf");
        let before = (fs::read(&a).unwrap(), fs::read(&b).unwrap());
        let bytes = merge(&[b.clone(), a.clone()]).unwrap();
        let merged = Document::load_mem(&bytes).unwrap();
        let ids: Vec<ObjectId> = merged.get_pages().into_values().collect();
        assert_eq!(ids.len(), 3);
        let text: Vec<String> = merged
            .get_pages()
            .keys()
            .map(|n| merged.extract_text(&[*n]).unwrap_or_default())
            .collect();
        assert!(text[0].contains("B 0"), "{text:?}");
        assert!(
            text[1].contains("A 0") && text[2].contains("A 1"),
            "{text:?}"
        );
        // The size and fonts came from the old page trees, which are gone.
        for id in ids {
            let page = merged.get_dictionary(id).unwrap();
            assert!(page.has(b"MediaBox") && page.has(b"Resources"));
        }
        // The originals are exactly as they were.
        assert_eq!(before, (fs::read(&a).unwrap(), fs::read(&b).unwrap()));
    }

    #[test]
    fn merging_one_file_or_a_broken_one_fails_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let a = save(&mut pdf("A", 1), dir.path(), "a.pdf");
        assert!(merge(std::slice::from_ref(&a)).is_err());
        let bad = dir.path().join("bad.pdf");
        fs::write(&bad, "not a pdf").unwrap();
        let e = merge(&[a, bad]).unwrap_err();
        assert!(e.to_string().contains("bad.pdf"), "{e}");
    }

    /// A PDF with one photo-like JPEG (noise compresses badly, so there is room to shrink).
    fn with_image(dir: &Path) -> PathBuf {
        let mut pixels = image::RgbImage::new(400, 400);
        let mut seed = 12345u32;
        for p in pixels.pixels_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let b = seed.to_le_bytes();
            *p = image::Rgb([b[1], b[2], b[3]]);
        }
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode_image(&pixels)
            .unwrap();
        let mut doc = pdf("Photo", 1);
        let img = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 400, "Height" => 400,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
            },
            jpeg,
        ));
        // Reachable from the page's resources, or pruning would drop it.
        for object in doc.objects.values_mut() {
            if let Object::Dictionary(d) = object
                && d.has(b"Font")
            {
                d.set("XObject", dictionary! { "Im1" => img });
            }
        }
        save(&mut doc, dir, "photo.pdf")
    }

    #[test]
    fn stronger_presets_make_a_picture_smaller_and_the_original_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = with_image(dir.path());
        let original = fs::read(&path).unwrap();
        let balanced = compress(&path, Preset::Balanced).unwrap();
        let smallest = compress(&path, Preset::Smallest).unwrap();
        assert!(balanced.len() < original.len());
        assert!(smallest.len() < balanced.len());
        assert!(Document::load_mem(&smallest).is_ok());
        assert_eq!(original, fs::read(&path).unwrap());
    }

    #[test]
    fn a_result_that_is_not_smaller_is_refused_instead_of_written() {
        let dir = tempfile::tempdir().unwrap();
        // Already repacked once: a second repack has nothing left to save.
        let path = save(&mut pdf("A", 1), dir.path(), "a.pdf");
        let once = compress(&path, Preset::Lossless).unwrap_or_else(|_| fs::read(&path).unwrap());
        fs::write(&path, once).unwrap();
        let e = compress(&path, Preset::Lossless).unwrap_err();
        assert!(e.to_string().contains("can't be made smaller"), "{e}");
    }

    #[test]
    fn a_partial_that_was_there_before_is_never_removed() {
        let dir = tempfile::tempdir().unwrap();
        // A finished copy left at a partial name (see `finish_copy`) is somebody else's file.
        let theirs = dir.path().join(format!("out.pdf{PARTIAL}"));
        fs::write(&theirs, "mine").unwrap();
        let made = write_new(dir.path(), "out.pdf", b"new").unwrap();
        assert_eq!(made, dir.path().join("out.pdf"));
        assert_eq!(fs::read(&theirs).unwrap(), b"mine");
        assert_eq!(fs::read(&made).unwrap(), b"new");
    }

    #[test]
    fn a_pdf_with_form_fields_is_not_merged() {
        let dir = tempfile::tempdir().unwrap();
        let mut with_form = pdf("F", 1);
        let root = with_form
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        with_form
            .get_object_mut(root)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("AcroForm", dictionary! { "Fields" => Vec::<Object>::new() });
        let a = save(&mut with_form, dir.path(), "form.pdf");
        let b = save(&mut pdf("B", 1), dir.path(), "b.pdf");
        let e = merge(&[a, b]).unwrap_err();
        assert!(e.to_string().contains("would lose"), "{e}");
    }

    #[test]
    fn a_new_file_never_replaces_one_and_leaves_no_temporary_behind() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("out.pdf"), "mine").unwrap();
        let made = write_new(dir.path(), "out.pdf", b"new").unwrap();
        assert_eq!(made, dir.path().join("out (2).pdf"));
        assert_eq!(fs::read(dir.path().join("out.pdf")).unwrap(), b"mine");
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(PARTIAL)), "{names:?}");
    }
}
