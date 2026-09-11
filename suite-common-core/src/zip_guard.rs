// zip_guard.rs — Bounded reads of untrusted ZIP document packages.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Every format this suite opens — docx, odt, xlsx, ods, pptx, odp — is a ZIP
// package, and every one of them can arrive from a download, an email
// attachment or a USB stick. The readers used to call `read_to_end` on each
// entry with no bound, so a 40-kilobyte file declaring a gigabyte of
// uncompressed content allocated until the process died. `docs/readiness-
// 2026-09/interoperability.md` asks for size, count and decompression
// limits; this is where they live, once, rather than six times with six
// different numbers.

use std::io::{Read, Seek};
use zip::ZipArchive;

/// What a document package is allowed to be.
///
/// These are not tuned to the largest real document anyone has ever made —
/// they are the point past which a file is better explained as hostile or
/// broken than as someone's presentation. Each is cheap to raise if a real
/// document ever trips it, and a reader that refuses to open a file is a
/// far better outcome than one that exhausts memory trying.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZipLimits {
    /// Members in the archive. A real package has tens; a few hundred with
    /// one image per slide. Tens of thousands is a zip quine or a generator
    /// gone wrong.
    pub max_entries: usize,
    /// Uncompressed bytes of any single member. Comfortably past a
    /// high-resolution image or an embedded font, and far short of what it
    /// takes to exhaust a desktop.
    pub max_entry_bytes: u64,
    /// Uncompressed bytes of everything read from the archive together, so
    /// many medium entries cannot do what one huge entry cannot.
    pub max_total_bytes: u64,
}

impl Default for ZipLimits {
    fn default() -> Self {
        Self {
            max_entries: 16_384,
            max_entry_bytes: 128 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Tracks the running total across the entries of one archive.
#[derive(Debug)]
pub struct ZipBudget {
    limits: ZipLimits,
    spent: u64,
}

impl ZipBudget {
    pub fn new(limits: ZipLimits) -> Self {
        Self { limits, spent: 0 }
    }

    /// Reject an archive with an implausible number of members before
    /// reading any of them.
    pub fn check_entry_count(&self, count: usize) -> Result<(), String> {
        if count > self.limits.max_entries {
            return Err(format!(
                "package has {count} members, more than the {} this reader will open",
                self.limits.max_entries
            ));
        }
        Ok(())
    }

    /// Read one member, refusing to grow past the per-entry or total limit.
    ///
    /// The archive's declared uncompressed size is attacker-controlled, so
    /// it is not consulted: a bomb simply lies about it. The bound comes
    /// from `Read::take`, which stops the decompressor regardless of what
    /// the header claimed, and the buffer is grown incrementally rather
    /// than preallocated from a declared size — preallocating is itself the
    /// allocation the bomb is asking for.
    pub fn read_entry(&mut self, reader: &mut impl Read, name: &str) -> Result<Vec<u8>, PartError> {
        let remaining_total = self.limits.max_total_bytes.saturating_sub(self.spent);
        let cap = self.limits.max_entry_bytes.min(remaining_total);

        let mut bytes = Vec::new();
        // One byte past the cap, so going over is observable rather than a
        // silent truncation that a reader would parse as a valid document.
        reader
            .take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| PartError::Unreadable(format!("read {name}: {e}")))?;

        if bytes.len() as u64 > cap {
            return Err(PartError::TooLarge(if cap < self.limits.max_entry_bytes {
                format!(
                    "package expands past the {} byte total this reader will decompress \
                     (while reading {name})",
                    self.limits.max_total_bytes
                )
            } else {
                format!(
                    "{name} expands past the {} byte limit for one package member",
                    self.limits.max_entry_bytes
                )
            }));
        }

        self.spent += bytes.len() as u64;
        Ok(bytes)
    }

    /// Read one member as UTF-8 text, for the XML parts.
    pub fn read_entry_to_string(
        &mut self,
        reader: &mut impl Read,
        name: &str,
    ) -> Result<String, PartError> {
        let bytes = self.read_entry(reader, name)?;
        String::from_utf8(bytes)
            .map_err(|_| PartError::Unreadable(format!("{name} is not valid UTF-8")))
    }

    pub fn spent(&self) -> u64 {
        self.spent
    }
}

impl Default for ZipBudget {
    fn default() -> Self {
        Self::new(ZipLimits::default())
    }
}

/// Why a part could not be produced.
///
/// Readers need to tell these apart, and a string they have to sniff for a
/// prefix is not a way to do it: "there is no content.xml, so this is not
/// an ODT" and "content.xml is a decompression bomb" call for different
/// words to the user, and the difference must survive refactoring.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartError {
    /// No member by that name. For a required part this usually means the
    /// file is not the format the reader was expecting.
    Missing,
    /// Present, and past what this reader will decompress.
    TooLarge(String),
    /// Present and within budget, but unreadable — truncated stream, bad
    /// encoding, corrupt compression.
    Unreadable(String),
}

impl PartError {
    pub fn is_missing(&self) -> bool {
        matches!(self, PartError::Missing)
    }
}

/// Readers whose public error type is a plain string — which is all of them
/// today — convert with `?` and keep the wording.
impl From<PartError> for String {
    fn from(e: PartError) -> Self {
        e.to_string()
    }
}

impl std::fmt::Display for PartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartError::Missing => write!(f, "missing from package"),
            PartError::TooLarge(why) | PartError::Unreadable(why) => write!(f, "{why}"),
        }
    }
}

/// Bounded access to an archive's named parts.
///
/// Every reader in the suite wants the same two things — "give me this part,
/// it must be there" and "give me this part if it happens to be there" — and
/// each used to spell them out in three or four lines of `by_name` plus
/// `read_to_string`, with the bound missing from all of them. As methods the
/// bound is not something a call site can forget.
pub trait BoundedArchive {
    /// A part the format requires. A `Missing` error is the reader's cue to
    /// say "not an ODT?"; anything else is a real failure to report as-is.
    fn part_to_string(&mut self, name: &str, budget: &mut ZipBudget) -> Result<String, PartError>;

    /// A part the format allows to be missing, unreadable or oversized —
    /// styles, notes, relationships. Absent and broken are the same thing to
    /// a caller that has a default, and neither should fail the document.
    fn optional_part_to_string(&mut self, name: &str, budget: &mut ZipBudget) -> String {
        self.part_to_string(name, budget).unwrap_or_default()
    }

    /// A binary part: an image, a font, an embedded object.
    fn part_to_bytes(&mut self, name: &str, budget: &mut ZipBudget) -> Result<Vec<u8>, PartError>;
}

impl<R: Read + Seek> BoundedArchive for ZipArchive<R> {
    fn part_to_string(&mut self, name: &str, budget: &mut ZipBudget) -> Result<String, PartError> {
        let mut entry = self.by_name(name).map_err(|_| PartError::Missing)?;
        budget.read_entry_to_string(&mut entry, name)
    }

    fn part_to_bytes(&mut self, name: &str, budget: &mut ZipBudget) -> Result<Vec<u8>, PartError> {
        let mut entry = self.by_name(name).map_err(|_| PartError::Missing)?;
        budget.read_entry(&mut entry, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> ZipLimits {
        ZipLimits { max_entries: 3, max_entry_bytes: 16, max_total_bytes: 24 }
    }

    #[test]
    fn an_entry_within_the_limit_reads_whole() {
        let mut budget = ZipBudget::new(tiny());
        let got = budget.read_entry(&mut &b"hello"[..], "content.xml").unwrap();
        assert_eq!(got, b"hello");
        assert_eq!(budget.spent(), 5);
    }

    /// The important property: the reader stops at the bound instead of
    /// allocating whatever the entry offers. A real bomb's stream is
    /// effectively endless, which `repeat` models exactly.
    #[test]
    fn an_endless_entry_is_refused_rather_than_read() {
        let mut budget = ZipBudget::new(tiny());
        let err = budget
            .read_entry(&mut std::io::repeat(b'A').take(1 << 20), "bomb.bin")
            .unwrap_err();
        assert!(
            matches!(&err, PartError::TooLarge(why) if why.contains("one package member")),
            "got: {err:?}"
        );
    }

    /// Many medium entries cannot do what one huge entry cannot.
    #[test]
    fn entries_share_one_total_budget() {
        let mut budget = ZipBudget::new(tiny());
        budget.read_entry(&mut &[b'a'; 16][..], "one").unwrap();
        let err = budget.read_entry(&mut &[b'b'; 16][..], "two").unwrap_err();
        assert!(
            matches!(&err, PartError::TooLarge(why) if why.contains("total this reader will decompress")),
            "got: {err:?}"
        );
    }

    /// Exhausting the budget exactly must not then report a false error on
    /// an empty entry, and must not panic on the saturating arithmetic.
    #[test]
    fn a_budget_spent_to_the_byte_still_reads_an_empty_entry() {
        let mut budget = ZipBudget::new(ZipLimits { max_entries: 3, max_entry_bytes: 8, max_total_bytes: 8 });
        budget.read_entry(&mut &[b'a'; 8][..], "one").unwrap();
        assert_eq!(budget.read_entry(&mut &b""[..], "two").unwrap(), Vec::<u8>::new());
        let err = budget.read_entry(&mut &b"x"[..], "three").unwrap_err();
        assert!(matches!(&err, PartError::TooLarge(_)), "got: {err:?}");
    }

    #[test]
    fn an_implausible_member_count_is_refused_before_reading_anything() {
        let budget = ZipBudget::new(tiny());
        budget.check_entry_count(3).expect("at the limit is allowed");
        let err = budget.check_entry_count(4).unwrap_err();
        assert!(err.contains("more than the 3"), "got: {err}");
    }

    #[test]
    fn a_non_utf8_xml_part_is_an_error_not_a_panic() {
        let mut budget = ZipBudget::new(tiny());
        let err = budget
            .read_entry_to_string(&mut &b"\xff\xfe"[..], "content.xml")
            .unwrap_err();
        assert!(
            matches!(&err, PartError::Unreadable(why) if why.contains("not valid UTF-8")),
            "got: {err:?}"
        );
    }

    /// A real archive, through the trait, with a member that lies about its
    /// size the way a bomb does. Built in memory rather than committed as a
    /// binary fixture, so what is being tested is visible in the test.
    #[test]
    fn a_bomb_in_a_real_archive_is_refused_and_an_ordinary_part_is_not() {
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;

        let mut buffer = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("content.xml", options).unwrap();
            writer.write_all(b"<office:text/>").unwrap();
            // Highly compressible: a few hundred bytes on disk, 8 MiB out.
            writer.start_file("bomb.bin", options).unwrap();
            writer.write_all(&vec![0u8; 8 * 1024 * 1024]).unwrap();
            writer.finish().unwrap();
        }

        let mut archive = ZipArchive::new(Cursor::new(&buffer)).unwrap();
        let mut budget = ZipBudget::new(ZipLimits {
            max_entries: 16,
            max_entry_bytes: 1024,
            max_total_bytes: 4096,
        });

        assert_eq!(
            archive.part_to_string("content.xml", &mut budget).unwrap(),
            "<office:text/>"
        );
        let err = archive.part_to_bytes("bomb.bin", &mut budget).unwrap_err();
        assert!(matches!(&err, PartError::TooLarge(_)), "got: {err:?}");

        // The same archive under the shipped defaults opens fine: the limit
        // is what makes the bomb a bomb, not the archive.
        let mut budget = ZipBudget::default();
        assert_eq!(archive.part_to_bytes("bomb.bin", &mut budget).unwrap().len(), 8 * 1024 * 1024);
    }

    #[test]
    fn a_missing_optional_part_is_empty_not_an_error() {
        use std::io::Cursor;
        let mut buffer = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
            writer.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
            writer.finish().unwrap();
        }
        let mut archive = ZipArchive::new(Cursor::new(&buffer)).unwrap();
        let mut budget = ZipBudget::default();
        assert_eq!(archive.optional_part_to_string("styles.xml", &mut budget), "");
        // Missing is its own variant, so a reader can say "not an ODT?"
        // without matching on words.
        assert!(archive.part_to_string("styles.xml", &mut budget).unwrap_err().is_missing());
    }

    /// The shipped defaults must accept an ordinary document: a limit that
    /// rejects real files would be discovered by users, not by this suite.
    #[test]
    fn the_default_limits_accept_a_realistic_package() {
        let mut budget = ZipBudget::default();
        budget.check_entry_count(400).expect("a slide deck with an image per slide");
        let megabyte = vec![0u8; 1024 * 1024];
        for index in 0..32 {
            budget
                .read_entry(&mut &megabyte[..], &format!("media/image{index}.png"))
                .expect("32 MiB of images is an ordinary deck");
        }
    }
}
