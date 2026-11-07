//! A minimal but real ODT package, and a reader for its `content.xml`. Shared by
//! dcp-wopi's tests and dcp-interop's Collabora-over-Matrix test.
#![allow(dead_code)]

use std::io::Write;

pub const ODT_MIME: &str = "application/vnd.oasis.opendocument.text";

/// A minimal but real ODT package (mimetype first and stored, manifest,
/// content.xml) holding `text` in one paragraph.
pub fn make_odt(text: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).unwrap();
        z.write_all(ODT_MIME.as_bytes()).unwrap();
        z.start_file("META-INF/manifest.xml", deflated).unwrap();
        z.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
 <manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.text"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>"#,
        )
        .unwrap();
        z.start_file("content.xml", deflated).unwrap();
        write!(
            z,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">
 <office:body><office:text><text:p>{text}</text:p></office:text></office:body>
</office:document-content>"#
        )
        .unwrap();
        z.finish().unwrap();
    }
    buf.into_inner()
}

pub fn content_xml(odt: &[u8]) -> String {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(odt)).expect("ODT is a zip");
    let mut s = String::new();
    z.by_name("content.xml").expect("content.xml").read_to_string(&mut s).unwrap();
    s
}

