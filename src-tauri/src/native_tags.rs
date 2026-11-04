//! The tags the operating system keeps on a file itself, which Folio mirrors (ADR 0002):
//! Finder tags on macOS (the `com.apple.metadata:_kMDItemUserTags` attribute, a property list)
//! and `user.xdg.tags` on Linux (a comma-separated list). Windows has none, and Folio's tags
//! there stay in its own index.
//!
//! This module only reads and writes the tags. Which side is right when they differ is
//! decided in [`crate::tag_sync`].

use std::path::Path;

/// Where native tags are kept. Reading and writing are separate from the sync rules so those
/// can be tried without a file system.
pub trait Store {
    /// The tags on `path`: empty if it has none. `Err` when they can't be read, which leaves
    /// the file out of the sync.
    fn read(&self, path: &Path) -> Result<Vec<Item>, String>;
    /// Replaces the tags on `path` with these `raw` items; an empty list removes them.
    fn write(&self, path: &Path, raw: &[String]) -> Result<(), String>;
    /// How a tag with this name is written when the file doesn't have it yet.
    fn item(&self, name: &str) -> String;
}

/// One tag as the system keeps it: its name, and the text exactly as stored. The text carries
/// what a name can't, such as the colour Finder keeps beside a tag, so it is written back
/// unchanged for a tag that stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub raw: String,
}

impl Item {
    #[cfg(any(target_os = "linux", test))]
    pub fn plain(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            raw: name.to_owned(),
        }
    }
}

/// Finder tags as they are kept in the attribute: `Name`, or `Name`, a newline and the colour
/// number.
#[cfg(any(target_os = "macos", test))]
pub mod finder {
    use super::Item;

    /// The names Finder gives its seven colours, by the number it keeps beside a tag's name.
    const FINDER_COLORS: [(&str, u8); 7] = [
        ("Gray", 1),
        ("Green", 2),
        ("Purple", 3),
        ("Blue", 4),
        ("Yellow", 5),
        ("Red", 6),
        ("Orange", 7),
    ];

    /// The Finder number for a tag name that is one of the seven colours, ignoring letter case.
    fn finder_color(name: &str) -> Option<u8> {
        FINDER_COLORS
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, c)| c)
    }

    /// The tags in a Finder tags property list. The seven colour labels are the colour tags
    /// (`Grey` too, for `Gray`); a label with another name is a tag of its own. Each keeps the
    /// text it was stored as, colour number and all.
    pub fn decode(bytes: &[u8]) -> Result<Vec<Item>, String> {
        let raw: Vec<String> = plist::from_bytes(bytes).map_err(|e| e.to_string())?;
        Ok(raw
            .into_iter()
            .filter_map(|item| {
                let (name, color) = item.split_once('\n').unwrap_or((&item, "0"));
                let name = name.trim();
                if name.is_empty() {
                    return None;
                }
                let number: u8 = color.trim().parse().unwrap_or(0);
                let label = FINDER_COLORS.iter().find(|&&(_, c)| c == number);
                // Only a label that is called after its colour (or Grey for Gray) is the
                // colour tag; "Work" in red is a tag of its own.
                let name = match label {
                    Some(&(own, _)) if name.eq_ignore_ascii_case(own) || is_grey(name, number) => {
                        own.to_owned()
                    }
                    _ => name.to_owned(),
                };
                Some(Item { name, raw: item })
            })
            .collect())
    }

    fn is_grey(name: &str, number: u8) -> bool {
        number == 1 && name.eq_ignore_ascii_case("Grey")
    }

    /// A new tag as Finder keeps it: a colour's name with its number, any other name as it is.
    pub fn item(name: &str) -> String {
        match finder_color(name) {
            Some(c) => format!("{name}\n{c}"),
            None => name.to_owned(),
        }
    }

    /// The property list for these stored items.
    pub fn encode(raw: &[String]) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        plist::to_writer_binary(&mut out, &raw).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

/// The freedesktop `user.xdg.tags` attribute: UTF-8 names separated by commas.
#[cfg(any(target_os = "linux", test))]
pub mod xdg {
    use super::Item;

    /// The tags in the attribute. Names are trimmed; empty ones are dropped.
    pub fn decode(bytes: &[u8]) -> Vec<Item> {
        String::from_utf8_lossy(bytes)
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(Item::plain)
            .collect()
    }

    /// The attribute for these items. A name with a comma can't be told from two, so none is
    /// written with one; see [`can_store`].
    pub fn encode(raw: &[String]) -> Vec<u8> {
        raw.iter()
            .filter(|n| can_store(n))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(",")
            .into_bytes()
    }

    /// Whether the attribute can hold the name.
    pub fn can_store(name: &str) -> bool {
        !name.contains(',') && !name.trim().is_empty()
    }
}

/// The attribute that holds the tags on this system, if it has one.
#[cfg(target_os = "macos")]
const ATTRIBUTE: &str = "com.apple.metadata:_kMDItemUserTags";
#[cfg(target_os = "linux")]
const ATTRIBUTE: &str = "user.xdg.tags";

/// The system's own store: the extended attributes of the file.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub struct System;

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Store for System {
    fn read(&self, path: &Path) -> Result<Vec<Item>, String> {
        match xattr::get(path, ATTRIBUTE) {
            Ok(None) => Ok(Vec::new()),
            Ok(Some(bytes)) => {
                #[cfg(target_os = "macos")]
                {
                    finder::decode(&bytes)
                }
                #[cfg(target_os = "linux")]
                {
                    Ok(xdg::decode(&bytes))
                }
            }
            // A file with no such attribute is a file with no tags.
            Err(e) if e.raw_os_error() == Some(no_attribute()) => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn write(&self, path: &Path, raw: &[String]) -> Result<(), String> {
        if raw.is_empty() {
            return match xattr::remove(path, ATTRIBUTE) {
                Ok(()) => Ok(()),
                // Nothing to take away.
                Err(e) if e.raw_os_error() == Some(no_attribute()) => Ok(()),
                Err(e) => Err(e.to_string()),
            };
        }
        #[cfg(target_os = "macos")]
        let value = finder::encode(raw)?;
        #[cfg(target_os = "linux")]
        let value = xdg::encode(raw);
        xattr::set(path, ATTRIBUTE, &value).map_err(|e| e.to_string())
    }

    fn item(&self, name: &str) -> String {
        #[cfg(target_os = "macos")]
        {
            finder::item(name)
        }
        #[cfg(target_os = "linux")]
        {
            name.to_owned()
        }
    }
}

/// `ENODATA` on Linux, `ENOATTR` on macOS: the file has no such attribute.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn no_attribute() -> i32 {
    #[cfg(target_os = "macos")]
    {
        93
    }
    #[cfg(target_os = "linux")]
    {
        61
    }
}

/// The system's store, or `None` where the system has no tags of its own (Windows).
#[cfg_attr(
    any(target_os = "macos", target_os = "linux"),
    expect(clippy::unnecessary_wraps, reason = "Windows has no store of its own")
)]
pub fn system() -> Option<&'static dyn Store> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        Some(&System)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Whether the system can hold a tag with this name, as written by [`Store::write`]. A line
/// break is never allowed: Finder keeps a tag's colour after one. Linux can't keep a comma.
pub fn can_store(name: &str) -> bool {
    if name.chars().any(char::is_control) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        xdg::can_store(name)
    }
    #[cfg(not(target_os = "linux"))]
    {
        !name.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raws(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names_of(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn finder_tags_survive_a_round_trip_with_their_colours() {
        let written = finder::encode(&raws(&["Red\n6", "Work", "Gray\n1"])).unwrap();
        assert_eq!(&written[..8], b"bplist00");
        let read = finder::decode(&written).unwrap();
        assert_eq!(names_of(&read), ["Red", "Work", "Gray"]);
    }

    #[test]
    fn a_custom_tags_colour_is_kept_as_it_was_stored() {
        let mut stored = Vec::new();
        plist::to_writer_binary(&mut stored, &["Work\n6", "Red\n4", "Grey\n1"]).unwrap();
        let read = finder::decode(&stored).unwrap();
        // The names are what Folio sees; the stored text is what is written back.
        assert_eq!(names_of(&read), ["Work", "Red", "Gray"]);
        let back: Vec<String> = read.iter().map(|i| i.raw.clone()).collect();
        assert_eq!(back, ["Work\n6", "Red\n4", "Grey\n1"]);
        let again = finder::decode(&finder::encode(&back).unwrap()).unwrap();
        assert_eq!(again, read);
    }

    #[test]
    fn a_new_tag_gets_the_number_beside_a_colour_and_none_beside_a_plain_name() {
        assert_eq!(finder::item("Red"), "Red\n6");
        assert_eq!(finder::item("orange"), "orange\n7");
        assert_eq!(finder::item("Work"), "Work");
    }

    #[test]
    fn finder_colour_labels_read_as_the_colour_in_any_spelling() {
        let mut out = Vec::new();
        plist::to_writer_binary(&mut out, &["Grey\n1", "Orange\n7", "Work\n6", "Plain\n0"])
            .unwrap();
        // A label that isn't called after its colour is a tag of its own, with that name.
        let read = finder::decode(&out).unwrap();
        assert_eq!(names_of(&read), ["Gray", "Orange", "Work", "Plain"]);
    }

    #[test]
    fn a_property_list_that_is_not_names_is_refused() {
        assert!(finder::decode(b"not a plist").is_err());
        let mut out = Vec::new();
        plist::to_writer_binary(&mut out, &42_u32).unwrap();
        assert!(finder::decode(&out).is_err());
    }

    #[test]
    fn xdg_tags_are_a_comma_list_without_blanks() {
        let read = xdg::decode(b"work, home,,  taxes ,");
        assert_eq!(names_of(&read), ["work", "home", "taxes"]);
        assert_eq!(xdg::decode(b"").len(), 0);
        assert_eq!(xdg::encode(&raws(&["work", "home"])), b"work,home");
    }

    #[test]
    fn xdg_leaves_out_a_name_it_cannot_hold() {
        assert!(!xdg::can_store("a,b"));
        assert!(xdg::can_store("Café 2024"));
        assert_eq!(xdg::encode(&raws(&["a,b", "c"])), b"c");
        let read = xdg::decode(&xdg::encode(&raws(&["ünï", "日本"])));
        assert_eq!(names_of(&read), ["ünï", "日本"]);
    }

    #[test]
    fn a_name_with_a_line_break_is_never_stored() {
        // Finder would read "a\nb" as the tag "a" in colour "b".
        assert!(!can_store("a\nb"));
        assert!(!can_store("tab\there"));
        assert!(can_store("Café"));
    }

    /// The real attribute on a real file. Skipped where the file system keeps no attributes.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reads_back_what_it_wrote_in_the_xdg_attribute() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        if System.write(&file, &raws(&["work", "home"])).is_err() {
            eprintln!("no user attributes on this file system; skipped");
            return;
        }
        let read = System.read(&file).unwrap();
        assert_eq!(names_of(&read), ["work", "home"]);
        assert_eq!(
            xattr::get(&file, "user.xdg.tags").unwrap().unwrap(),
            b"work,home"
        );
        System.write(&file, &[]).unwrap();
        assert_eq!(System.read(&file).unwrap().len(), 0);
        // Taking away what isn't there is not an error.
        System.write(&file, &[]).unwrap();
    }

    /// The whole sync against the real attribute.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_tag_given_in_folio_shows_in_the_xdg_attribute_and_one_set_outside_comes_in() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        if xattr::set(&file, "user.xdg.tags", b"seed").is_err() {
            eprintln!("no user attributes on this file system; skipped");
            return;
        }
        xattr::remove(&file, "user.xdg.tags").unwrap();
        let tags = crate::tags::Tags::open(None).unwrap();
        tags.toggle(std::slice::from_ref(&file), 1).unwrap();
        tags.sync(&System, std::slice::from_ref(&file));
        assert_eq!(names_of(&System.read(&file).unwrap()), ["Red"]);

        xattr::set(&file, "user.xdg.tags", b"Red,Invoices").unwrap();
        tags.sync(&System, std::slice::from_ref(&file));
        let have: Vec<String> = tags.list().into_iter().map(|t| t.name).collect();
        assert!(have.contains(&"Invoices".to_owned()));
    }
}
