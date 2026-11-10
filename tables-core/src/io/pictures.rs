// pictures.rs — read anchored worksheet pictures back from xlsx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pictures resolve through the same workbook→worksheet→drawing→media
// relationship chain as charts (TBL-002): never ZIP order or part
// filenames. Each admitted picture lands in the owning sheet's `pictures`
// as bytes plus anchor and size, so the grid draws the file's own image
// and the writer keeps it. Anything else is reported, not silently
// dropped.

use super::charts::{
    attr, drawing_part_for_sheet, part_dir, read_part, rel_targets, resolve_anchor,
    resolve_sheet_parts, unprefix, Anchor,
};
use crate::sheet::{PictureFormat, PictureSpec};

/// What one picture's read could not carry, for the caller to report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureReadIssue {
    pub location: String,
    pub detail: String,
}

/// The text after an open tag `<name ...>`: the character after the name
/// must end it (space, `>` or `/`), so `<blipFill>` never matches a search
/// for `<blip>`, the way `<sheets>` would match `<sheet`.
fn open_tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = xml;
    loop {
        let i = rest.find(&format!("<{name}"))?;
        let after = rest[i + name.len() + 1..].chars().next()?;
        rest = &rest[i + name.len() + 1..];
        if matches!(after, ' ' | '>' | '/') {
            return Some(rest);
        }
    }
}

/// The `<pic>` blip's image relationship ID: `<blipFill><blip
/// r:embed="…">`. Openpyxl writes `r:embed`; a default-namespace writer
/// would write a bare `embed`, so both are accepted.
fn anchor_blip_rid(anchor_xml: &str) -> Option<String> {
    let pic = open_tag(anchor_xml, "pic")?;
    let pic = pic.split("</pic>").next().unwrap_or(pic);
    let blip = open_tag(pic, "blip")?;
    let blip = blip.split('>').next().unwrap_or("");
    attr(blip, "r:embed").or_else(|| attr(blip, "embed"))
}

/// The admitted image formats: exactly what the xlsx writer round-trips,
/// sniffed from magic bytes rather than the media part's extension (a
/// producer can name a JPEG `image1.png`).
fn sniff_format(bytes: &[u8]) -> Option<PictureFormat> {
    if bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        Some(PictureFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(PictureFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(PictureFormat::Gif)
    } else if bytes.starts_with(b"BM") {
        Some(PictureFormat::Bmp)
    } else {
        None
    }
}

/// Read one bounded media part, or `None` where it is absent, unreadable,
/// or past the decompression budget: a missing image means "carry
/// nothing", not failure.
fn read_media(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    budget: &mut suite_common_core::zip_guard::ZipBudget,
) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    budget.read_entry(&mut entry, name).ok()
}

/// Read anchored pictures back from an xlsx (ours or a foreign writer's),
/// with the owning sheet of each picture and what could not be modeled.
///
/// `sheets` supplies the owning sheets' real column/row geometry so
/// two-cell anchors resolve to the pixel size the producer laid out.
pub fn read_pictures_detailed(
    path: &str,
    sheets: &[crate::sheet::SheetModel],
) -> (Vec<(String, PictureSpec)>, Vec<PictureReadIssue>) {
    let mut out = Vec::new();
    let mut issues = Vec::new();
    let Ok(f) = std::fs::File::open(path) else {
        return (out, issues);
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return (out, issues);
    };
    // Bounded reads: best-effort must not extend to decompressing whatever
    // an untrusted package asks for (#442).
    use suite_common_core::zip_guard::ZipBudget;
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return (out, issues);
    }

    // The owning sheet's real geometry, mirroring the writer: hidden
    // rows/columns occupy zero pixels when the object moves and sizes
    // with its cells, so a picture laid out over them inverts to the
    // written size. Shared with the chart reader's geometry.
    let geometry = |name: &str| -> (Vec<f64>, Vec<f64>) {
        sheets
            .iter()
            .find(|s| s.name == name)
            .map(|s| {
                (
                    (0..s.cols.max(s.col_widths.len()))
                        .map(|c| if s.is_col_hidden(c) { 0.0 } else { s.col_width(c) })
                        .collect(),
                    (0..s.rows.max(s.row_heights.len()))
                        .map(|r| if s.is_row_hidden(r) { 0.0 } else { s.row_height(r) })
                        .collect(),
                )
            })
            .unwrap_or_default()
    };

    for (sheet_name, sheet_part) in resolve_sheet_parts(&mut zip, &mut budget) {
        let Some(drawing_part) = drawing_part_for_sheet(&mut zip, &sheet_part, &mut budget) else {
            continue;
        };
        let Some(drawing_xml) = read_part(&mut zip, &drawing_part, &mut budget) else {
            continue;
        };
        let drawing_xml = unprefix(&drawing_xml);

        let dir = part_dir(&drawing_part).to_string();
        let base = drawing_part.rsplit('/').next().unwrap_or(&drawing_part).to_string();
        let rels_name = format!("{dir}/_rels/{base}.rels");
        let drawing_rels = read_part(&mut zip, &rels_name, &mut budget)
            .map(|xml| rel_targets(&xml, &dir))
            .unwrap_or_default();
        let (widths, heights) = geometry(&sheet_name);
        for anchor_kind in ["oneCellAnchor", "twoCellAnchor", "absoluteAnchor"] {
            // Open-tag match: a producer may hang attributes on the anchor
            // itself (openpyxl's `<twoCellAnchor editAs="oneCell">`), so an
            // exact `<kind>` match would miss the whole anchor.
            for anchor_xml in drawing_xml.split(&format!("<{anchor_kind}")).skip(1) {
                let anchor_xml = anchor_xml.split(&format!("</{anchor_kind}>")).next().unwrap_or(anchor_xml);
                let location = format!("Sheet '{sheet_name}', drawing '{drawing_part}'");
                let Some(rid) = anchor_blip_rid(anchor_xml) else {
                    continue;
                };
                let Some(media_part) = drawing_rels.get(&rid) else {
                    issues.push(PictureReadIssue {
                        location,
                        detail: format!("an anchor names picture relationship '{rid}' with no target; its image is not carried"),
                    });
                    continue;
                };
                let Some(bytes) = read_media(&mut zip, media_part, &mut budget) else {
                    issues.push(PictureReadIssue {
                        location,
                        detail: format!("picture part '{media_part}' cannot be read; its image is not carried"),
                    });
                    continue;
                };
                let Some(format) = sniff_format(&bytes) else {
                    issues.push(PictureReadIssue {
                        location,
                        detail: format!("picture part '{media_part}' is not PNG, JPEG, GIF or BMP; its image is not carried"),
                    });
                    continue;
                };
                // The anchor machinery is the charts' (TBL-002): same
                // elements, same EMU geometry. Its chart-worded notes are
                // discarded; pictures report in their own words below.
                let mut anchor_issues = Vec::new();
                let anchor = resolve_anchor(anchor_xml, anchor_kind, &widths, &heights, &location, &mut anchor_issues);
                let Some(anchor) = anchor else {
                    issues.push(PictureReadIssue {
                        location,
                        detail: "the picture's anchor states no starting cell; its image is not carried".into(),
                    });
                    continue;
                };
                let Anchor::Placed { cell, size_px } = anchor else {
                    issues.push(PictureReadIssue {
                        location,
                        detail: "the picture uses an absolute anchor with no cell; its image is not carried".into(),
                    });
                    continue;
                };
                out.push((
                    sheet_name.clone(),
                    PictureSpec {
                        anchor: cell,
                        width_px: size_px.0,
                        height_px: size_px.1,
                        format,
                        bytes,
                    },
                ));
            }
        }
    }
    (out, issues)
}

/// Read anchored pictures back from an xlsx without a model at hand
/// (tests, the oracle), on this app's own default geometry.
pub fn read_pictures_from_xlsx(path: &str) -> Vec<crate::sheet::PictureSpec> {
    read_pictures_detailed(path, &[]).0.into_iter().map(|(_, spec)| spec).collect()
}

#[cfg(test)]
mod picture_tests {
    use super::*;
    use crate::sheet::{PictureFormat, SheetModel, COL_WIDTH, ROW_HEIGHT};

    const QUAD_PNG: &[u8] = include_bytes!("../../tests/fixtures/picture-quad.png");
    const RED_JPEG: &[u8] = include_bytes!("../../tests/fixtures/picture-red.jpg");

    /// One hand-built package part; binary parts (the media) ride a byte
    /// vector. Parts list in ZIP order, so media-first proves ZIP order is
    /// not identity.
    fn package_bin(path: &std::path::Path, parts: &[(&str, &[u8])]) {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in parts {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    fn package(path: &std::path::Path, parts: &[(&str, &str)]) {
        package_bin(path, &parts.iter().map(|(n, d)| (*n, d.as_bytes())).collect::<Vec<_>>());
    }

    fn workbook_xml() -> String {
        r#"<?xml version="1.0"?><workbook xmlns:r="r"><sheets><sheet name="Sheet1" r:id="rId1"/><sheet name="Sheet2" r:id="rId2"/></sheets></workbook>"#.into()
    }

    fn workbook_rels() -> String {
        r#"<?xml version="1.0"?><Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#.into()
    }

    fn drawing_rels() -> String {
        r#"<?xml version="1.0"?><Relationships><Relationship Id="rId1" Target="../media/image1.png"/></Relationships>"#.into()
    }

    /// A two-cell picture anchor from B2 to D7, like the render lab's
    /// `tables/worksheet-image` fixture (openpyxl, editAs=oneCell).
    fn two_cell_picture_drawing() -> String {
        r#"<xdr:wsDr xmlns:xdr="xdr" xmlns:a="a" xmlns:r="r"><xdr:twoCellAnchor editAs="oneCell"><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>6</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="1" name="Picture 1"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></a:xfrm></xdr:spPr></xdr:pic><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#.into()
    }

    /// The picture package: the drawing belongs to the second sheet, so a
    /// reader resolving by ZIP order or filename would land it wrong.
    fn picture_package(path: &std::path::Path) {
        package_bin(path, &[
            ("xl/media/image1.png", QUAD_PNG),
            ("xl/workbook.xml", workbook_xml().as_bytes()),
            ("xl/_rels/workbook.xml.rels", workbook_rels().as_bytes()),
            ("xl/worksheets/sheet1.xml", b"<worksheet><sheetData/></worksheet>"),
            ("xl/worksheets/sheet2.xml", b"<worksheet><sheetData/><drawing r:id=\"rId1\"/></worksheet>"),
            ("xl/worksheets/_rels/sheet2.xml.rels", b"<?xml version=\"1.0\"?><Relationships><Relationship Id=\"rId1\" Target=\"../drawings/drawing1.xml\"/></Relationships>"),
            ("xl/drawings/drawing1.xml", two_cell_picture_drawing().as_bytes()),
            ("xl/drawings/_rels/drawing1.xml.rels", drawing_rels().as_bytes()),
        ]);
    }

    #[test]
    fn two_cell_picture_lands_on_its_owning_sheet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("picture.xlsx");
        picture_package(&path);
        let (pics, issues) = read_pictures_detailed(path.to_str().unwrap(), &[]);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
        assert_eq!(pics.len(), 1);
        let (owner, pic) = &pics[0];
        assert_eq!(owner, "Sheet2");
        assert_eq!(pic.anchor, (1, 1));
        assert_eq!(pic.format, PictureFormat::Png);
        assert_eq!(pic.bytes, QUAD_PNG);
        // Two default columns wide, five default rows tall: the anchor
        // inverts over this app's own default geometry.
        assert_eq!((pic.width_px, pic.height_px), (2.0 * COL_WIDTH, 5.0 * ROW_HEIGHT));
    }

    #[test]
    fn one_cell_anchor_uses_its_extent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("onecell.xlsx");
        let drawing = "<xdr:wsDr><xdr:oneCellAnchor><xdr:from><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>3</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx=\"952500\" cy=\"476250\"/><xdr:pic><xdr:blipFill><a:blip r:embed=\"rId1\"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:oneCellAnchor></xdr:wsDr>".to_string();
        package_bin(path.as_path(), &[
            ("xl/media/image1.png", QUAD_PNG),
            ("xl/workbook.xml", workbook_xml().as_bytes()),
            ("xl/_rels/workbook.xml.rels", workbook_rels().as_bytes()),
            ("xl/worksheets/sheet1.xml", b"<worksheet><sheetData/><drawing r:id=\"rId1\"/></worksheet>"),
            ("xl/worksheets/_rels/sheet1.xml.rels", b"<?xml version=\"1.0\"?><Relationships><Relationship Id=\"rId1\" Target=\"../drawings/drawing1.xml\"/></Relationships>"),
            ("xl/drawings/drawing1.xml", drawing.as_bytes()),
            ("xl/drawings/_rels/drawing1.xml.rels", drawing_rels().as_bytes()),
        ]);
        let (pics, issues) = read_pictures_detailed(path.to_str().unwrap(), &[]);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
        assert_eq!(pics.len(), 1);
        assert_eq!(pics[0].1.anchor, (3, 2));
        // 952500 EMU = 100 px, 476250 EMU = 50 px.
        assert_eq!((pics[0].1.width_px, pics[0].1.height_px), (100.0, 50.0));
    }

    #[test]
    fn unresolvable_blip_is_reported_not_placed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("norel.xlsx");
        let drawing = "<xdr:wsDr><xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:pic><xdr:blipFill><a:blip r:embed=\"rId9\"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>";
        package(path.as_path(), &[
            ("xl/workbook.xml", &workbook_xml()),
            ("xl/_rels/workbook.xml.rels", &workbook_rels()),
            ("xl/worksheets/sheet1.xml", "<worksheet><sheetData/><drawing r:id=\"rId1\"/></worksheet>"),
            ("xl/worksheets/_rels/sheet1.xml.rels", "<?xml version=\"1.0\"?><Relationships><Relationship Id=\"rId1\" Target=\"../drawings/drawing1.xml\"/></Relationships>"),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", &drawing_rels()),
        ]);
        let (pics, issues) = read_pictures_detailed(path.to_str().unwrap(), &[]);
        assert!(pics.is_empty());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].detail.contains("rId9"), "{}", issues[0].detail);
    }

    #[test]
    fn non_image_media_is_reported_not_carried() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vector.xlsx");
        package(path.as_path(), &[
            ("xl/workbook.xml", &workbook_xml()),
            ("xl/_rels/workbook.xml.rels", &workbook_rels()),
            ("xl/worksheets/sheet1.xml", "<worksheet><sheetData/><drawing r:id=\"rId1\"/></worksheet>"),
            ("xl/worksheets/_rels/sheet1.xml.rels", "<?xml version=\"1.0\"?><Relationships><Relationship Id=\"rId1\" Target=\"../drawings/drawing1.xml\"/></Relationships>"),
            ("xl/drawings/drawing1.xml", "<xdr:wsDr><xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:pic><xdr:blipFill><a:blip r:embed=\"rId1\"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"),
            ("xl/drawings/_rels/drawing1.xml.rels", "<?xml version=\"1.0\"?><Relationships><Relationship Id=\"rId1\" Target=\"../media/image1.wmf\"/></Relationships>"),
            ("xl/media/image1.wmf", "not an image at all"),
        ]);
        let (pics, issues) = read_pictures_detailed(path.to_str().unwrap(), &[]);
        assert!(pics.is_empty());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].detail.contains("image1.wmf"), "{}", issues[0].detail);
    }

    #[test]
    fn absolute_anchor_is_reported_not_placed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absolute.xlsx");
        package_bin(path.as_path(), &[
            ("xl/media/image1.png", QUAD_PNG),
            ("xl/workbook.xml", workbook_xml().as_bytes()),
            ("xl/_rels/workbook.xml.rels", workbook_rels().as_bytes()),
            ("xl/worksheets/sheet1.xml", b"<worksheet><sheetData/><drawing r:id=\"rId1\"/></worksheet>"),
            ("xl/worksheets/_rels/sheet1.xml.rels", b"<?xml version=\"1.0\"?><Relationships><Relationship Id=\"rId1\" Target=\"../drawings/drawing1.xml\"/></Relationships>"),
            ("xl/drawings/drawing1.xml", b"<xdr:wsDr><xdr:absoluteAnchor><xdr:pos x=\"0\" y=\"0\"/><xdr:ext cx=\"952500\" cy=\"952500\"/><xdr:pic><xdr:blipFill><a:blip r:embed=\"rId1\"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>"),
            ("xl/drawings/_rels/drawing1.xml.rels", drawing_rels().as_bytes()),
        ]);
        let (pics, issues) = read_pictures_detailed(path.to_str().unwrap(), &[]);
        assert!(pics.is_empty());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].detail.contains("absolute anchor"), "{}", issues[0].detail);
    }

    #[test]
    fn blip_fill_does_not_shadow_blip() {
        // `<blipFill>` starts with `<blip`: a naive split lands inside the
        // fill element and reads no relationship.
        let anchor = " editAs=\"oneCell\"><from></from><pic><blipFill><blip r:embed=\"rId1\"/></blipFill></pic>";
        assert_eq!(anchor_blip_rid(anchor).as_deref(), Some("rId1"));
    }

    #[test]
    fn format_sniffing_reads_magic_not_names() {
        assert_eq!(sniff_format(QUAD_PNG), Some(PictureFormat::Png));
        assert_eq!(sniff_format(RED_JPEG), Some(PictureFormat::Jpeg));
        assert_eq!(sniff_format(b"GIF89a..."), Some(PictureFormat::Gif));
        assert_eq!(sniff_format(b"BM..."), Some(PictureFormat::Bmp));
        assert_eq!(sniff_format(b"definitely not an image"), None);
        assert_eq!(sniff_format(b""), None);
    }

    /// The save keeps the picture: anchor, size, format and bytes survive
    /// a write and a re-read.
    #[test]
    fn pictures_round_trip_through_save() {
        for (format, bytes) in [(PictureFormat::Png, QUAD_PNG), (PictureFormat::Jpeg, RED_JPEG)] {
            let mut sheet = SheetModel::new("Sheet1", 30, 10, 0);
            sheet.pictures.push(PictureSpec {
                anchor: (1, 1),
                width_px: 128.0,
                height_px: 100.0,
                format,
                bytes: bytes.to_vec(),
            });
            let out = crate::io::save_sheets_to_xlsx_bytes(&[sheet], None).expect("save keeps pictures");
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("roundtrip.xlsx");
            std::fs::write(&path, out).unwrap();
            let (pics, issues) = read_pictures_detailed(path.to_str().unwrap(), &[]);
            assert!(issues.is_empty(), "unexpected issues: {issues:?}");
            assert_eq!(pics.len(), 1);
            assert_eq!(pics[0].1.anchor, (1, 1));
            assert_eq!(pics[0].1.format, format);
            assert_eq!(pics[0].1.bytes, bytes);
            // The writer lays the picture out over the default geometry the
            // reader inverts with, so the logical size survives.
            assert!((pics[0].1.width_px - 128.0).abs() < 1.0, "{}", pics[0].1.width_px);
            assert!((pics[0].1.height_px - 100.0).abs() < 1.0, "{}", pics[0].1.height_px);
        }
    }
}

