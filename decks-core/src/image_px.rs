// image_px.rs — a picture's pixel size from its file header.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// ODF states a picture's crop (`fo:clip`) as lengths against its natural
// size, so reading and writing one needs the size: PNG, JPEG or GIF, at
// 96 dpi, which is what Impress takes for a picture that states none.

/// Width and height in pixels, for PNG, JPEG and GIF.
pub fn pixel_size(b: &[u8]) -> Option<(u32, u32)> {
    let be32 = |i: usize| b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    let be16 = |i: usize| b.get(i..i + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as u32);
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.get(12..16) == Some(b"IHDR") {
        return Some((be32(16)?, be32(20)?));
    }
    if b.starts_with(b"GIF8") {
        let le16 = |i: usize| b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as u32);
        return Some((le16(6)?, le16(8)?));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 4 <= b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            let len = be16(i + 2)? as usize;
            // SOF0..SOF15, except DHT (C4), JPG (C8) and DAC (CC).
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            i += 2 + len;
        }
    }
    None
}

/// The natural size of the picture at `path`, in points at 96 dpi.
pub fn natural_size_pt(path: &str) -> Option<(f64, f64)> {
    let mut head = vec![0u8; 64 * 1024];
    use std::io::Read;
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    let (w, h) = pixel_size(&head[..n])?;
    Some((f64::from(w) * 0.75, f64::from(h) * 0.75))
}

#[cfg(test)]
mod tests {
    use super::pixel_size;

    #[test]
    fn png_gif_and_jpeg_headers_give_their_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(pixel_size(&png), Some((640, 480)));
        let gif = [b'G', b'I', b'F', b'8', b'9', b'a', 0x20, 0x03, 0x58, 0x02];
        assert_eq!(pixel_size(&gif), Some((800, 600)));
        // SOI, an APP0 of length 4, then SOF0 with height 300, width 400.
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 17, 8, 0x01, 0x2C, 0x01, 0x90];
        assert_eq!(pixel_size(&jpeg), Some((400, 300)));
        assert_eq!(pixel_size(b"not an image"), None);
    }
}
