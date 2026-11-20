// picture_source.rs — Which file a picture run's path may be read from.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A picture run holds a path, and the writers read that path when the
// document is saved. A path a reader put there names a file in the media
// cache, which this process wrote. Two kinds of path come from outside it:
// the file the user chose in Insert Image, and the destination of a
// Markdown image, which is whatever the Markdown file says. A Markdown file
// from someone else can name any file the app can read. Saved as .odt or
// .docx, that file's bytes went into the package whether or not it was a
// picture, where the person who wrote the Markdown could read them once the
// document came back; and a device such as /dev/zero was read until memory
// ran out.
//
// So a writer takes a picture from the media cache as it is, and from
// anywhere else only when it is a regular file that starts the way a
// picture does. Anything else is written as its alt text, as a picture
// that could not be read always was.

use std::io::Read;
use std::path::Path;

/// How much of a file is read to tell whether it is a picture. An SVG may
/// open with an XML declaration, a doctype and a comment before its root.
const HEAD_BYTES: u64 = 4096;

/// Whether `src` names something that is there and is not a regular file:
/// a device, a FIFO, a socket or a directory. Reading one as a picture
/// never ends, or waits for ever. A missing file is not one of them: it
/// may yet arrive, and reading it fails at once.
pub fn is_not_a_file(src: &str) -> bool {
    std::fs::metadata(src).is_ok_and(|meta| !meta.is_file())
}

/// The bytes of the picture at `src`, for a writer to put in a package.
/// None when `src` is not a regular file, cannot be read, or is outside
/// the media cache and does not start the way a picture does.
pub fn read(src: &str) -> Option<Vec<u8>> {
    // Asked before opening: opening a FIFO waits for its other end.
    if !std::fs::metadata(src).ok()?.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(src).ok()?;
    let mut bytes = Vec::new();
    file.by_ref().take(HEAD_BYTES).read_to_end(&mut bytes).ok()?;
    if !(is_unpacked(Path::new(src)) || is_picture(&bytes)) {
        return None;
    }
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Whether `path` is a file in this process's media cache, where the
/// readers unpack a package's pictures: those are written back whatever
/// their format. Both sides are resolved first, so a path that climbs out
/// of the cache with `..`, or a link, is not in it.
fn is_unpacked(path: &Path) -> bool {
    let Some(cache) = suite_common_core::media_cache::process_dir() else { return false };
    match (path.canonicalize(), cache.canonicalize()) {
        (Ok(path), Ok(cache)) => path.starts_with(cache),
        _ => false,
    }
}

/// Whether `head`, the start of a file, is the start of a picture in a
/// format GTK draws: PNG, JPEG, GIF, TIFF, WebP, BMP, AVIF, HEIC, JPEG XL
/// or SVG.
pub fn is_picture(head: &[u8]) -> bool {
    let at = |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
    let brands: [&[u8]; 6] = [b"avif", b"avis", b"heic", b"heix", b"mif1", b"msf1"];
    at(0, b"\x89PNG\r\n\x1a\n")
        || at(0, b"\xff\xd8\xff")
        || at(0, b"GIF87a")
        || at(0, b"GIF89a")
        || at(0, b"II*\0")
        || at(0, b"MM\0*")
        || (at(0, b"RIFF") && at(8, b"WEBP"))
        // A BMP's two letters are followed by its size and four reserved
        // bytes, which are zero; two letters alone would let text through.
        || (at(0, b"BM") && at(6, &[0; 4]))
        || (at(4, b"ftyp") && brands.iter().any(|brand| at(8, brand)))
        || at(0, b"\xff\x0a")
        || at(0, b"\0\0\0\x0cJXL \r\n\x87\n")
        || head.windows(4).any(|tag| tag == b"<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";

    #[test]
    fn a_picture_is_known_by_how_it_starts() {
        let pictures: &[&[u8]] = &[
            PNG,
            b"\xff\xd8\xff\xe0\0\x10JFIF",
            b"GIF87a",
            b"GIF89a",
            b"II*\0",
            b"MM\0*",
            b"RIFF\x24\0\0\0WEBPVP8 ",
            b"BM\x46\0\0\0\0\0\0\0\x36\0\0\0",
            b"\0\0\0\x1cftypavif",
            b"\0\0\0\x18ftypheic",
            b"\xff\x0a",
            b"\0\0\0\x0cJXL \r\n\x87\n",
            b"<?xml version=\"1.0\"?>\n<!-- drawn by hand -->\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        ];
        for head in pictures {
            assert!(is_picture(head), "{:?}", String::from_utf8_lossy(head));
        }
        let others: &[&[u8]] = &[
            b"",
            b"-----BEGIN OPENSSH PRIVATE KEY-----\n",
            b"PK\x03\x04",
            b"%PDF-1.7",
            b"root:x:0:0:root:/root:/bin/bash\n",
            b"BM",
            b"BMW owners, 2026\n",
            b"\0\0\0\x18ftypmp42",
            b"RIFF\x24\0\0\0WAVEfmt ",
            b"<?xml version=\"1.0\"?>\n<settings/>",
        ];
        for head in others {
            assert!(!is_picture(head), "{:?}", String::from_utf8_lossy(head));
        }
    }

    /// A Markdown image can name any file. A writer reads the ones that are
    /// pictures, and not a key, a text file under a picture's name, a
    /// directory or a device.
    #[test]
    fn a_writer_reads_a_picture_and_not_any_other_file() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path.to_string_lossy().into_owned()
        };
        // Whatever it is called: the bytes decide, not the name.
        assert_eq!(read(&at("logo.png", PNG)).as_deref(), Some(PNG));
        assert_eq!(read(&at("logo", PNG)).as_deref(), Some(PNG));
        assert_eq!(read(&at("id_ed25519", b"-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaA==\n")), None);
        assert_eq!(read(&at("notes.png", b"the quarter's figures, not for sharing\n")), None);
        assert_eq!(read(&dir.path().to_string_lossy()), None, "a directory");
        assert_eq!(read(&dir.path().join("missing.png").to_string_lossy()), None);
        // A picture longer than the part read to recognise it comes whole.
        let mut long = PNG.to_vec();
        long.resize(3 * HEAD_BYTES as usize + 7, 0x5a);
        assert_eq!(read(&at("long.png", &long)), Some(long));
    }

    /// Reading /dev/zero as a picture never ends: `std::fs::read` on it
    /// grows until the process is out of memory.
    #[cfg(unix)]
    #[test]
    fn a_device_is_not_read() {
        assert!(is_not_a_file("/dev/zero"));
        assert!(is_not_a_file("/"));
        assert_eq!(read("/dev/zero"), None);
        // A file that is not there yet may arrive; one that is there reads.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("later.png");
        assert!(!is_not_a_file(&path.to_string_lossy()));
        std::fs::write(&path, PNG).unwrap();
        assert!(!is_not_a_file(&path.to_string_lossy()));
    }

    /// A picture a reader unpacked from a package is written back whatever
    /// its format: the package it came from called it a picture. A path
    /// that only passes through the cache's directory is not one of them.
    #[test]
    fn an_unpacked_picture_is_read_whatever_its_format() {
        // An enhanced metafile: Word writes them, GTK does not draw them.
        let emf = b"\x01\0\0\0\x6c\0\0\0not a format this module knows";
        assert!(!is_picture(emf));
        let unpacked = suite_common_core::media_cache::persist(emf).unwrap();
        assert_eq!(read(&unpacked.to_string_lossy()).as_deref(), Some(emf.as_slice()));

        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret");
        std::fs::write(&secret, b"not a picture").unwrap();
        // Resolved, so each ".." below goes up the directory it names.
        let secret = secret.canonicalize().unwrap();
        let cache = suite_common_core::media_cache::process_dir().unwrap().canonicalize().unwrap();
        // "<cache>/../../<outside>/secret", spelled so it starts in the cache.
        let mut climbing = cache.clone();
        for _ in cache.components().skip(1) {
            climbing.push("..");
        }
        climbing.push(secret.strip_prefix("/").unwrap());
        assert_eq!(std::fs::read(&climbing).unwrap(), b"not a picture", "the path does reach the file");
        assert_eq!(read(&climbing.to_string_lossy()), None);
    }
}
