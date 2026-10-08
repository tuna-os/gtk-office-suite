// docx.rs — Document ⇄ DOCX via rdocx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Model-level DOCX I/O: paragraphs, styled runs (incl. highlight and inline
// code via SourceText), headings, alignment, lists, hyperlinks. Tables are
// flattened (see read()). Fidelity is measured by tests/docx.rs and the
// LO-authored corpus in tests/lo_parity.rs.

use crate::model::{Alignment, Document, ListKind, ListLabel, NumberFormat, PageGeometry, Paragraph, ParaStyle, Run, RunStyle};
use rdocx_oxml::shared::ST_Jc;

/// The body-paragraph properties rdocx does not hand back, in twips.
///
/// Two unrelated gaps share one scan of `word/document.xml`: the
/// strict-OOXML indents rdocx never parses, and the tab stops it parses
/// but exposes only as a count (`tab_stop_count`), with no positions.
#[derive(Clone, Default)]
struct RawPara {
    start_twips: Option<f64>,
    end_twips: Option<f64>,
    tab_twips: Vec<f64>,
    /// Direct `w:contextualSpacing` (rdocx does not model it).
    contextual: bool,
}

/// Per body paragraph: strict-spelled indents and tab-stop positions.
///
/// ISO/IEC 29500 strict names the horizontal indents `w:start`/`w:end`
/// rather than the transitional `w:left`/`w:right`, and LibreOffice's
/// "Office Open XML Text" export filter writes the strict spelling — as
/// does anything else targeting that conformance class. rdocx reads only
/// the transitional attributes, so those indents arrive as "absent" and
/// the paragraph reads as unindented. Everything else in `w:ind` is
/// already shared between the two spellings (`w:firstLine`, `w:hanging`),
/// as is `w:jc`'s `start`/`end`, which rdocx does map.
///
/// Both vectors are positional, in document order. `body` has one entry
/// per body-level `w:p`, so the caller can pair it with
/// `doc.paragraphs()`. `cells` has one per `w:p` directly inside a
/// top-level table's cells, in the order rdocx's tables → rows → cells →
/// paragraphs walk visits them (#1204). Paragraphs in a table nested inside
/// a cell belong to neither and are skipped.
///
/// Returns the values in twips. Any failure to open or scan the part
/// yields empty vectors, which the caller treats as "nothing to add".
#[derive(Default)]
struct RawParas {
    body: Vec<RawPara>,
    cells: Vec<RawPara>,
    /// Per body-level table, in order: how many body paragraphs precede it.
    tables_at: Vec<usize>,
}

fn raw_paragraph_props(path: &str) -> RawParas {
    fn scan(path: &str) -> Result<RawParas, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml")?, &mut xml)?;

        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut out = RawParas::default();
        let mut table_depth = 0usize;
        // The paragraph list the current depth feeds, if any.
        fn at(out: &mut RawParas, depth: usize) -> Option<&mut Vec<RawPara>> {
            match depth {
                0 => Some(&mut out.body),
                1 => Some(&mut out.cells),
                _ => None,
            }
        }
        // `w:tab` means two different things: a stop inside `w:tabs`, and
        // a tab character inside a run. Only the former has a position,
        // so the flag is what keeps a tabbed line from inventing stops.
        let mut in_tabs = false;
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) => match e.name().as_ref() {
                    "w:tbl" => {
                        if table_depth == 0 {
                            out.tables_at.push(out.body.len());
                        }
                        table_depth += 1;
                    }
                    "w:tabs" => in_tabs = true,
                    "w:p" => {
                        if let Some(list) = at(&mut out, table_depth) {
                            list.push(RawPara::default());
                        }
                    }
                    _ => {}
                },
                quick_xml::events::Event::End(e) => match e.name().as_ref() {
                    "w:tbl" => table_depth = table_depth.saturating_sub(1),
                    "w:tabs" => in_tabs = false,
                    _ => {}
                },
                quick_xml::events::Event::Empty(e) => match e.name().as_ref() {
                    // A `w:p` with nothing in it is still a paragraph.
                    "w:p" => {
                        if let Some(list) = at(&mut out, table_depth) {
                            list.push(RawPara::default());
                        }
                    }
                    "w:tab" if in_tabs => {
                        let Some(last) = at(&mut out, table_depth).and_then(|l| l.last_mut()) else { continue };
                        let mut pos = None;
                        let mut val = None;
                        for a in e.attributes().with_checks(false).flatten() {
                            match a.key.as_ref() {
                                "w:pos" => pos = a.value.trim().parse::<f64>().ok(),
                                "w:val" => val = Some(a.value.trim().to_ascii_lowercase()),
                                _ => {}
                            }
                        }
                        // `w:val` decides whether this is a stop at all.
                        // `clear` *removes* an inherited stop at that
                        // position — LibreOffice writes one to drop the
                        // 2cm default — and `bar`/`num` are a rule and a
                        // list's numbering gap, not user tab stops. An
                        // allowlist keeps each of those from arriving as
                        // a phantom stop the document never had.
                        let keep = matches!(
                            val.as_deref().unwrap_or("left"),
                            "left" | "start" | "center" | "centre" | "right" | "end" | "decimal"
                        );
                        if let (Some(v), true) = (pos, keep) {
                            last.tab_twips.push(v);
                        }
                    }
                    "w:contextualSpacing" => {
                        if let Some(last) = at(&mut out, table_depth).and_then(|l| l.last_mut()) {
                            last.contextual = on_off(&e);
                        }
                    }
                    "w:ind" => {
                        let Some(last) = at(&mut out, table_depth).and_then(|l| l.last_mut()) else { continue };
                        for a in e.attributes().with_checks(false).flatten() {
                            let v = || {
                                a.value.trim().parse::<f64>().ok()
                            };
                            match a.key.as_ref() {
                                "w:start" => last.start_twips = v(),
                                "w:end" => last.end_twips = v(),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        Ok(out)
    }
    scan(path).unwrap_or_default()
}

/// Lay the scanned properties over what rdocx read. Transitional wins
/// where both are present: it is what rdocx read, and a file carrying
/// both is already self-contradictory.
fn apply_strict_indents(style: &mut ParaStyle, raw: &RawPara) {
    if style.left_indent_pt == 0.0 {
        if let Some(tw) = raw.start_twips { style.left_indent_pt = tw / 20.0; }
    }
    if style.right_indent_pt == 0.0 {
        if let Some(tw) = raw.end_twips { style.right_indent_pt = tw / 20.0; }
    }
    style.tab_stops_pt = raw.tab_twips.iter().map(|tw| tw / 20.0).collect();
}

/// An OOXML on/off element's value: present means on, unless `w:val`
/// says "0", "false" or "off".
fn on_off(e: &quick_xml::events::BytesStart<'_>) -> bool {
    !e.attributes().with_checks(false).flatten().any(|a| {
        a.key.as_ref() == "w:val" && matches!(a.value.trim(), "0" | "false" | "off")
    })
}

/// Paragraph styles whose paragraphs have `w:contextualSpacing` ("don't add
/// space between paragraphs of the same style"), through `w:basedOn`.
/// Word's list styles set it: without it every list item of a python-docx
/// or Word list would be 10pt apart.
fn contextual_styles(path: &str) -> std::collections::HashSet<String> {
    /// (style id, basedOn, contextualSpacing if the style itself says)
    type StyleSpacing = (String, Option<String>, Option<bool>);
    fn scan(path: &str) -> Result<Vec<StyleSpacing>, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/styles.xml")?, &mut xml)?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut out: Vec<StyleSpacing> = Vec::new();
        let attr = |e: &quick_xml::events::BytesStart<'_>, key: &str| {
            e.attributes().with_checks(false).flatten().find(|a| a.key.as_ref() == key).map(|a| a.value.to_string())
        };
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) if e.name().as_ref() == "w:style" => {
                    out.push((attr(&e, "w:styleId").unwrap_or_default(), None, None));
                }
                quick_xml::events::Event::Empty(e) => match e.name().as_ref() {
                    "w:basedOn" => {
                        if let Some(last) = out.last_mut() {
                            last.1 = attr(&e, "w:val");
                        }
                    }
                    "w:contextualSpacing" => {
                        if let Some(last) = out.last_mut() {
                            last.2 = Some(on_off(&e));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        Ok(out)
    }
    let styles = scan(path).unwrap_or_default();
    let by_id: std::collections::HashMap<&str, &StyleSpacing> =
        styles.iter().map(|s| (s.0.as_str(), s)).collect();
    styles
        .iter()
        .filter(|s| {
            // Walk basedOn to the first style that decides; bounded, as a
            // hostile file may make the chain a cycle.
            let mut cur = Some(*s);
            for _ in 0..32 {
                let Some(style) = cur else { return false };
                if let Some(on) = style.2 {
                    return on;
                }
                cur = style.1.as_deref().and_then(|b| by_id.get(b).copied());
            }
            false
        })
        .map(|s| s.0.clone())
        .collect()
}

/// Read a .docx file into a Document.
/// How deeply a docx's XML may nest before it is refused unread. rdocx
/// parses a table inside a cell by recursing, at about 100 KB of stack a
/// level in a debug build, and an overflow aborts the app rather than
/// failing the read: 25 nested tables were enough on a 2 MiB thread
/// (#1206). Real documents nest a few dozen elements deep.
pub const MAX_XML_DEPTH: usize = 256;

/// The stack a docx is read on: room for [`MAX_XML_DEPTH`] levels of the
/// deepest recursion with a wide margin. Only the pages used are committed.
const READ_STACK_BYTES: usize = 64 << 20;

/// Read a docx: refused when it nests past [`MAX_XML_DEPTH`], otherwise
/// read on a thread with [`READ_STACK_BYTES`] of stack, whatever thread
/// asked. A panic in the reader reaches the caller as before.
pub fn read(path: &str) -> Result<Document, String> {
    suite_common_core::zip_guard::check_xml_depth(std::path::Path::new(path), MAX_XML_DEPTH)
        .map_err(|e| format!("Cannot open .docx {path}: {e}"))?;
    let owned = path.to_string();
    let reader = std::thread::Builder::new()
        .name("docx-read".into())
        .stack_size(READ_STACK_BYTES)
        .spawn(move || read_on_this_thread(&owned))
        .map_err(|e| format!("Cannot open .docx {path}: {e}"))?;
    reader.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

fn read_on_this_thread(path: &str) -> Result<Document, String> {
    // Smart chips are content controls rdocx does not read runs from:
    // they become sentinel runs first (docx_chips).
    let (doc, (chips, revisions)) = open_with_chips(path).map_err(|e| format!("Cannot open .docx {}: {}", path, e))?;
    let comments = crate::docx_comments::bodies(&doc);
    let mut read = read_opened(path, doc)?;
    // Tracked changes first: a bracket may hold a chip's sentinel.
    crate::docx_revisions::restore(&mut read, &revisions);
    crate::docx_chips::restore(&mut read, &chips);
    // Comments last: their markers may sit in a tracked change.
    if !comments.is_empty() {
        crate::docx_comments::restore(&mut read, comments);
    }
    crate::docx_toc::restore(&mut read);
    // Page-number fields in the header and footer, as "{page}"/"{total}".
    let (header, footer) = header_footer_templates(path);
    if header.is_some() {
        read.header = header;
    }
    if footer.is_some() {
        read.footer = footer;
    }
    Ok(read)
}

/// A zip part's text, if it is there.
fn part_text(zip: &mut zip::ZipArchive<impl std::io::Read + std::io::Seek>, name: &str) -> Option<String> {
    let mut s = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(name).ok()?, &mut s).ok()?;
    Some(s)
}

/// The default header and footer as templates with their PAGE/NUMPAGES
/// fields ("{page}", "{total}"); `None` for a part without such fields.
fn header_footer_templates(path: &str) -> (Option<String>, Option<String>) {
    let Some(mut zip) = std::fs::File::open(path).ok().and_then(|f| zip::ZipArchive::new(f).ok()) else { return (None, None) };
    let (Some(document), Some(rels)) = (part_text(&mut zip, "word/document.xml"), part_text(&mut zip, "word/_rels/document.xml.rels")) else {
        return (None, None);
    };
    let (h, f) = crate::docx_fields::default_parts(&document, &rels);
    let mut template = |part: Option<String>| part.and_then(|p| part_text(&mut zip, &p)).and_then(|xml| crate::docx_fields::template(&xml));
    (template(h), template(f))
}

/// The package at `path`, with its chip controls as sentinel runs.
/// What `open_with_chips` took out of the markup for rdocx: the smart
/// chips and the tracked changes, restored after reading.
type Unwrapped = (Vec<(crate::chips::Chip, String)>, Vec<crate::model::Revision>);

fn open_with_chips(path: &str) -> Result<(rdocx::Document, Unwrapped), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut xml = String::new();
    let found = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .ok()
        .and_then(|mut z| std::io::Read::read_to_string(&mut z.by_name("word/document.xml").ok()?, &mut xml).ok())
        .is_some();
    if !found || !(xml.contains("<w:sdt") || xml.contains("<w:ins ") || xml.contains("<w:del ") || xml.contains("<w:comment") || xml.contains("TOC \\")) {
        return Ok((rdocx::Document::open(path).map_err(|e| e.to_string())?, (Vec::new(), Vec::new())));
    }
    // A table of contents first: it lifts the paragraphs out of their
    // content control before chips look at controls.
    let toc = crate::docx_toc::unwrap(&xml);
    let (patched, chips) = crate::docx_chips::unwrap(toc.as_deref().unwrap_or(&xml));
    let (patched, revisions) = crate::docx_revisions::unwrap(&patched);
    let commented = crate::docx_comments::unwrap(&patched);
    let patched = commented.clone().unwrap_or(patched);
    if chips.is_empty() && revisions.is_empty() && commented.is_none() && toc.is_none() {
        return Ok((rdocx::Document::open(path).map_err(|e| e.to_string())?, (Vec::new(), Vec::new())));
    }
    let bytes = with_part(&bytes, "word/document.xml", |_| patched.clone())?;
    Ok((rdocx::Document::from_bytes(&bytes).map_err(|e| e.to_string())?, (chips, revisions)))
}

/// Tabs in run text as Word's `w:tab` elements. rdocx writes a tab as a
/// character inside `w:t`, which Word and LibreOffice read as a space.
fn tabs_as_elements(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(at) = rest.find("<w:t>").into_iter().chain(rest.find("<w:t ")).min() {
        let Some(open_end) = rest[at..].find('>').map(|e| at + e + 1) else { break };
        let Some(close) = rest[open_end..].find("</w:t>").map(|c| open_end + c) else { break };
        out.push_str(&rest[..open_end]);
        let text = &rest[open_end..close];
        out.push_str(&text.replace('\t', "</w:t><w:tab/><w:t xml:space=\"preserve\">"));
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

/// A run's text as written: bracketed as a tracked change if it is one.
fn bracketed(revisions: &mut Vec<crate::model::Revision>, run: &Run) -> String {
    match &run.style.revision {
        Some(rev) => {
            revisions.push(rev.clone());
            crate::docx_revisions::bracket(revisions.len() - 1, &run.text)
        }
        None => run.text.clone(),
    }
}

/// `package` with part `name` rewritten by `f`.
fn with_part(package: &[u8], name: &str, f: impl Fn(&str) -> String) -> Result<Vec<u8>, String> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|e| e.to_string())?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let mut part = zin.by_index(i).map_err(|e| e.to_string())?;
        let part_name = part.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut part, &mut data).map_err(|e| e.to_string())?;
        if part_name == name {
            let xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            data = f(&xml).into_bytes();
        }
        out.start_file(part_name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

fn read_opened(path: &str, doc: rdocx::Document) -> Result<Document, String> {
    THEME.with(|t| *t.borrow_mut() = theme_fonts(path));
    RESTARTED.with(|r| r.borrow_mut().clear());
    STYLE_FONTS.with(|s| *s.borrow_mut() = style_fonts(path));

    // Paragraph properties rdocx cannot hand back: strict-spelled indents
    // and tab-stop positions. The scan is positional, so it is only
    // trusted when it found exactly as many body paragraphs as rdocx did;
    // otherwise the two disagree about what a paragraph is and pairing
    // them would misattribute a property to its neighbour.
    let body = doc.paragraphs();
    let RawParas { body: raw, cells: raw_cells, tables_at } = raw_paragraph_props(path);
    let raw = (raw.len() == body.len()).then_some(raw);
    let tables = doc.tables();
    // Where each table sits: the number of body paragraphs before it.
    // rdocx gives tables apart from the paragraphs; its body items give
    // their order, but not how many paragraphs a content control holds,
    // which the XML scan counts. When neither can be trusted, the tables
    // go after the body, as they always did.
    let tables_at: Option<Vec<usize>> = if raw.is_some() && tables_at.len() == tables.len() {
        Some(tables_at)
    } else {
        let mut seen = 0usize;
        let mut at = Vec::new();
        let mut known = true;
        for item in doc.body_items() {
            match item {
                rdocx::BodyItemRef::Paragraph(_) => seen += 1,
                rdocx::BodyItemRef::Table(_) => at.push(seen),
                rdocx::BodyItemRef::ContentControl(_) => known = false,
                rdocx::BodyItemRef::UnsupportedXml(_) => {}
            }
        }
        (known && seen == body.len() && at.len() == tables.len()).then_some(at)
    };

    let mut paragraphs = Vec::new();
    // Per kept body paragraph: its style id and whether it has contextual
    // spacing, for the pass after this loop.
    let mut contextual: Vec<(Option<String>, bool)> = Vec::new();
    let contextual_ids = contextual_styles(path);
    // Set when a paragraph ends with a run-level page break, and consumed
    // by the next paragraph this loop keeps.
    let mut carried_break = false;
    // Per body paragraph: how many paragraphs were kept before it, which
    // is where a table that precedes it goes.
    let mut kept_before = Vec::with_capacity(body.len() + 1);
    // Per kept paragraph: the number Word gives it, when it is a numbered
    // list item (`WordCounter`), for the pass after the tables go in.
    let mut word_numbers: Vec<Option<u32>> = Vec::new();
    let mut counter = WordCounter::default();
    for (i, p) in body.iter().enumerate() {
        kept_before.push(paragraphs.len());
        let mut pending_break = false;
        // Decorative rules (LibreOffice's HorizontalLine style) carry no text.
        if p.style_id() == Some("HorizontalLine") && p.text().is_empty() {
            continue;
        }
        let mut para = map_paragraph(&doc, p, None);
        // A run-level break after this paragraph's text belongs to the
        // paragraph that follows, which is where LibreOffice puts the
        // break it converts from an ODF `fo:break-before`. The trailing
        // newline it leaves behind is the break itself, not content: this
        // model's paragraphs never contain one.
        let breaks = run_page_break(p);
        if breaks.trailing {
            if let Some(last) = para.runs.last_mut() {
                if last.text.ends_with('\n') {
                    last.text.pop();
                }
            }
            para.runs.retain(|r| {
                !r.text.is_empty() || r.style.image.is_some() || r.style.footnote.is_some()
            });
            pending_break = true;
        }
        if std::mem::take(&mut carried_break) {
            para.style.page_break_before = true;
        }
        // A paragraph holding nothing but a page break (python-docx's
        // `add_run().add_break(WD_BREAK.PAGE)`, Word's Ctrl+Enter on an
        // empty line) is the break, not a line of its own: LibreOffice
        // starts the next paragraph at the top of the new page. Kept, it
        // put an empty line there instead.
        // (rdocx gives the break's run the text "\n".)
        let only_break = para.runs.iter().all(|r| {
            r.style.image.is_none() && r.style.footnote.is_none() && r.text.chars().all(|c| c == '\n')
        });
        if only_break && breaks.leading && !pending_break && i + 1 < body.len() {
            carried_break = true;
            continue;
        }
        let style_id = p.style_id().map(str::to_string);
        let direct_contextual = raw.as_ref().and_then(|s| s.get(i)).is_some_and(|r| r.contextual);
        let is_contextual = direct_contextual || style_id.as_deref().is_some_and(|id| contextual_ids.contains(id));
        contextual.push((style_id, is_contextual));
        if let Some(r) = raw.as_ref().and_then(|s| s.get(i)) {
            apply_strict_indents(&mut para.style, r);
        }
        list_indent_from_declared(&doc, p, &mut para.style);
        word_numbers.push(counter.number(&doc, p, &mut para.style));
        paragraphs.push(para);
        carried_break = pending_break;
    }

    // Contextual spacing: no space between two paragraphs of one style
    // when the paragraph asks for it (Word's and LibreOffice's rule).
    for k in 1..contextual.len().min(paragraphs.len()) {
        let same = contextual[k - 1].0 == contextual[k].0;
        if same && contextual[k - 1].1 {
            paragraphs[k - 1].style.space_after_pt = 0.0;
        }
        if same && contextual[k].1 {
            paragraphs[k].style.space_before_pt = 0.0;
        }
    }

    kept_before.push(paragraphs.len());

    // Table cells become paragraphs tagged with (table, row, col) — the
    // document stays flat (offset invariants intact) and the grid is fully
    // recoverable. Each table's paragraphs are gathered on their own and
    // put where the table sits, after the contextual pass above, which
    // counts body paragraphs only.
    let mut table_paragraphs: Vec<Vec<Paragraph>> = Vec::with_capacity(tables.len());
    // The cell half of the scan, trusted on the same terms as the body
    // half: only when it counted exactly the paragraphs rdocx walks here.
    let mut cell_paragraphs = 0usize;
    for table in &tables {
        for row in (0..table.row_count()).filter_map(|ri| table.row(ri)) {
            for ci in 0..row.cell_count() {
                cell_paragraphs += row.cell(ci).map_or(0, |c| c.paragraphs().count());
            }
        }
    }
    let raw_cells = (raw_cells.len() == cell_paragraphs).then_some(raw_cells);
    let mut cell_index = 0usize;
    for (ti, table) in tables.iter().enumerate() {
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        for ri in 0..table.row_count() {
            let Some(row) = table.row(ri) else { continue };
            for ci in 0..row.cell_count() {
                let Some(cell) = row.cell(ci) else { continue };
                let mut wrote_any = false;
                let mut keep = |para: Paragraph| {
                    let mut para = para;
                    para.style.table_cell = Some(crate::model::TableCell {
                        table: ti as u32, row: ri as u32, col: ci as u32,
                    });
                    paragraphs.push(para);
                    wrote_any = true;
                };
                for item in cell.items() {
                    match item {
                        rdocx::CellItemRef::Paragraph(cp) => {
                            let raw_cell = raw_cells.as_ref().and_then(|s| s.get(cell_index));
                            cell_index += 1;
                            if cp.text().is_empty() { continue; }
                            let mut para = map_paragraph(&doc, &cp, table.style_id());
                            if let Some(r) = raw_cell {
                                apply_strict_indents(&mut para.style, r);
                            }
                            keep(para);
                        }
                        // A table nested in a cell belongs to its outer
                        // cell, as in the ODT reader: the model has no
                        // nesting, and dropping it lost its text (#1419).
                        // The save asks before it flattens one
                        // (`loss::content_a_save_drops`).
                        rdocx::CellItemRef::Table(nested) => {
                            for_each_nested_paragraph(&nested, &mut |cp| {
                                if !cp.text().is_empty() {
                                    keep(map_paragraph(&doc, cp, table.style_id()));
                                }
                            });
                        }
                        _ => {}
                    }
                }
                // Empty cells still occupy a grid position.
                if !wrote_any {
                    paragraphs.push(Paragraph {
                        style: ParaStyle {
                            table_cell: Some(crate::model::TableCell {
                                table: ti as u32, row: ri as u32, col: ci as u32,
                            }),
                            ..Default::default()
                        },
                        runs: vec![],
                    });
                }
            }
        }
        table_paragraphs.push(paragraphs);
    }
    // In reverse, so each insertion leaves the earlier positions valid;
    // tables at one position keep their order.
    for (ti, cells) in table_paragraphs.into_iter().enumerate().rev() {
        let at = tables_at.as_ref().map_or(paragraphs.len(), |t| kept_before[t[ti].min(body.len())]);
        word_numbers.splice(at..at, std::iter::repeat_n(None, cells.len()));
        paragraphs.splice(at..at, cells);
    }
    number_as_word_does(&mut paragraphs, &word_numbers);
    drop_table_separators(&mut paragraphs);

    if paragraphs.is_empty() {
        let mut d = Document::new();
        d.header = doc.header_text();
        d.footer = doc.footer_text();
        return Ok(d);
    }
    // Footnote texts land in the document list; docx ids remap to
    // zero-based indexes on the referencing runs (see map_paragraph).
    let footnotes: Vec<String> = doc.footnotes().into_iter().map(|(_, t)| t).collect();
    // Table ids are the tables' order, as the cells above are tagged.
    let table_columns = tables.iter().enumerate().filter_map(|(ti, t)| Some((ti as u32, column_widths(t)?))).collect();
    let table_rows = tables.iter().enumerate().filter_map(|(ti, t)| Some((ti as u32, row_heights(t)?))).collect();

    Ok(Document {
        paragraphs,
        footnotes,
        header: doc.header_text(),
        footer: doc.footer_text(),
        page: read_page_geometry(&doc),
        base_font: read_base_font(&doc),
        heading_styles: read_heading_styles(&doc),
        comments: Vec::new(),
        table_columns,
        table_rows,
    })
}

/// A table's row heights, when any row gives one (`w:trHeight`).
fn row_heights(table: &rdocx::TableRef<'_>) -> Option<Vec<Option<crate::model::RowHeight>>> {
    let heights: Vec<Option<crate::model::RowHeight>> = (0..table.row_count())
        .map(|r| match table.row(r)?.height()? {
            rdocx::RowHeight::AtLeast(h) => Some(crate::model::RowHeight { pt: h.to_pt(), exact: false }),
            rdocx::RowHeight::Exact(h) => Some(crate::model::RowHeight { pt: h.to_pt(), exact: true }),
        })
        .map(|h| h.filter(|h| h.pt > 0.0))
        .collect();
    heights.iter().any(Option::is_some).then_some(heights)
}

/// A table's column widths in points: its first row's cell widths, when
/// every cell gives one in absolute units.
fn column_widths(table: &rdocx::TableRef<'_>) -> Option<Vec<f64>> {
    let row = table.row(0)?;
    let widths: Option<Vec<f64>> = (0..row.cell_count()).map(|c| row.cell(c)?.width().map(|w| w.to_pt())).collect();
    widths.filter(|w| !w.is_empty() && w.iter().all(|x| *x > 0.0))
}

/// OOXML needs a paragraph after a table that ends the document or a cell,
/// and between two tables (Word merges adjacent ones). Those carry nothing,
/// so a table followed only by empty paragraphs, up to the next table or
/// the end, loses the first of them. The writer adds one in exactly that
/// case (`needs_separator`), so a save and reopen keeps the document.
fn drop_table_separators(paragraphs: &mut Vec<Paragraph>) {
    let table_of = |p: &Paragraph| p.style.table_cell.map(|t| t.table);
    let mut k = 0;
    while k < paragraphs.len() {
        let last_cell = table_of(&paragraphs[k]).is_some()
            && paragraphs.get(k + 1).map(table_of) != Some(table_of(&paragraphs[k]));
        if last_cell && needs_separator(&paragraphs[k + 1..]) && k + 1 < paragraphs.len() {
            paragraphs.remove(k + 1);
        }
        k += 1;
    }
}

/// Whether the paragraphs after a table, up to the next table or the end,
/// are all empty: the case where a separator paragraph follows the table.
fn needs_separator(after: &[Paragraph]) -> bool {
    after.iter().take_while(|p| p.style.table_cell.is_none()).all(|p| p.runs.is_empty())
}

/// Read a DOCX and retain package members this reader does not interpret.
/// The report is suitable for a structured save-warning UI; the opaque
/// package can be passed to [`write_with_opaque`] after an unrelated edit.
pub fn read_with_report(path: &str) -> Result<(Document, suite_common_core::interop::CompatibilityReport, suite_common_core::interop::OpaquePackage), String> {
    let document = read(path)?;
    let opaque = suite_common_core::interop::OpaquePackage::capture(
        path,
        &[
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
            // Comments are read (docx_comments) and written anew.
            "word/comments.xml",
            "word/commentsExtended.xml",
            "word/commentsIds.xml",
            "word/commentsExtensible.xml",
        ],
    )?;
    let mut report = suite_common_core::interop::CompatibilityReport::new("docx");
    for name in opaque.part_names() {
        report.record(suite_common_core::interop::UnsupportedFeature::new(
            "uninterpreted-package-part", "Uninterpreted package part", name,
            suite_common_core::interop::FeatureDisposition::OpaquePassThrough,
            "will be copied through on an opaque save",
        ));
    }
    Ok((document, report, opaque))
}

/// Page geometry from the docx section properties, when present.
fn read_page_geometry(doc: &rdocx::Document) -> Option<PageGeometry> {
    let sect = doc.section_properties()?;
    // Twips (1/20 pt) → points; the Twips newtype is not re-exported, so
    // work through the tuple field.
    let w = sect.page_width.map(|v| v.0 as f64 / 20.0)?;
    let h = sect.page_height.map(|v| v.0 as f64 / 20.0)?;
    let default = PageGeometry::default();
    Some(PageGeometry {
        width_pt: w,
        height_pt: h,
        margin_top_pt: sect.margin_top.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_top_pt),
        margin_bottom_pt: sect.margin_bottom.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_bottom_pt),
        margin_left_pt: sect.margin_left.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_left_pt),
        margin_right_pt: sect.margin_right.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_right_pt),
        // `w:cols` is absent for a single-column section, which is the
        // default this falls back to.
        columns: sect
            .columns
            .as_ref()
            .and_then(|c| c.num)
            .and_then(|n| u8::try_from(n).ok())
            .filter(|n| *n > 0)
            .unwrap_or(default.columns),
        column_gap_pt: sect
            .columns
            .as_ref()
            .and_then(|c| c.space)
            .map(|v| v.0 as f64 / 20.0)
            .unwrap_or(default.column_gap_pt),
    })
}

/// Write a Document to a .docx file.
pub fn write(doc: &Document, path: impl AsRef<std::path::Path>) -> Result<(), String> {
    let mut out = rdocx::Document::new();
    // Smart chips, written as sentinels and made content controls below.
    let mut chips: Vec<(crate::chips::Chip, String)> = Vec::new();
    // Tracked changes, written as bracketed text and made w:ins/w:del below.
    let mut revisions: Vec<crate::model::Revision> = Vec::new();
    let paras = &doc.paragraphs;
    // Comment threads: marker runs where each one's text starts and ends,
    // made range elements below. `open` are the threads the text written
    // so far is in; a thread with no text is written, empty, first.
    let roots = crate::docx_comments::roots(doc);
    let mut open: Vec<u32> = Vec::new();
    let mut orphans: Vec<u32> = {
        let marked: std::collections::HashSet<u32> = paras.iter().flat_map(|p| &p.runs).flat_map(|r| r.style.comments.iter().copied()).collect();
        roots.iter().copied().filter(|id| !marked.contains(id)).collect()
    };
    let wanted = |run: &Run| -> Vec<u32> { crate::docx_comments::threads_of(&roots, &run.style.comments).collect() };
    // Footnote texts first: model index → docx id.
    let footnote_ids: Vec<i32> = doc.footnotes.iter().map(|t| out.add_footnote(t)).collect();
    // The text column's width, where a table of contents' page numbers go
    // (rdocx's default page: Letter with 1in margins).
    let text_width_pt = doc.page.map_or(468.0, |g| g.width_pt - g.margin_left_pt - g.margin_right_pt);
    let mut has_toc = false;
    // The numIds of the bullet and numbered list definitions, once made.
    let mut list_ids: [Option<u32>; 2] = [None, None];
    // What each numbered item shows, and what Word would show it as.
    let ordinals = crate::lists::ordinals(paras.iter().map(|p| &p.style));
    let mut word = WordNumbers::default();
    let mut i = 0;
    while i < paras.len() {
        // Consecutive paragraphs sharing a table id become one rdocx table.
        if let Some(tc0) = paras[i].style.table_cell {
            let start = i;
            while i < paras.len()
                && paras[i].style.table_cell.map(|t| t.table) == Some(tc0.table)
            {
                i += 1;
            }
            let group = &paras[start..i];
            let rows = group.iter().filter_map(|p| p.style.table_cell.map(|t| t.row)).max().unwrap_or(0) as usize + 1;
            let cols = group.iter().filter_map(|p| p.style.table_cell.map(|t| t.col)).max().unwrap_or(0) as usize + 1;
            let mut tbl = out.add_table(rows, cols);
            // The file's column widths, when the table still has as many
            // columns as it was read with; rdocx's own are equal.
            if let Some(widths) = doc.table_columns.get(&tc0.table).filter(|w| w.len() == cols) {
                for (c, w) in widths.iter().enumerate() {
                    tbl.set_column_width(c, rdocx::Length::pt(*w));
                }
            }
            if let Some(heights) = doc.table_rows.get(&tc0.table) {
                for (r, h) in heights.iter().enumerate().take(rows) {
                    let (Some(h), Some(mut row)) = (h, tbl.row(r)) else { continue };
                    if h.exact {
                        row.set_height_exact(rdocx::Length::pt(h.pt));
                    } else {
                        row.set_height(rdocx::Length::pt(h.pt));
                    }
                }
            }
            let mut filled = std::collections::HashSet::new();
            for p in group {
                let tc = p.style.table_cell.expect("grouped by table_cell");
                if let Some(mut cell) = tbl.cell(tc.row as usize, tc.col as usize) {
                    // A new cell already holds an empty paragraph: replace
                    // it, or every cell opens with a blank line in Writer
                    // and Word (our reader skipped empty cell paragraphs,
                    // which hid it; #1296).
                    if filled.insert((tc.row, tc.col)) {
                        cell.remove_first_empty_paragraph();
                    }
                    let mut cp = cell.add_paragraph("");
                    // A cell paragraph's indents and alignment, which the
                    // reader maps the same way it maps a body paragraph's.
                    if p.style.left_indent_pt != 0.0 {
                        cp = cp.indent_left(rdocx::Length::pt(p.style.left_indent_pt));
                    }
                    if p.style.right_indent_pt != 0.0 {
                        cp = cp.indent_right(rdocx::Length::pt(p.style.right_indent_pt));
                    }
                    cp = match p.style.alignment {
                        Alignment::Center => cp.alignment(rdocx::Alignment::Center),
                        Alignment::Right => cp.alignment(rdocx::Alignment::Right),
                        Alignment::Justify => cp.alignment(rdocx::Alignment::Justify),
                        _ => cp,
                    };
                    // A cell's comments open and close inside it.
                    let mut in_cell: Vec<u32> = Vec::new();
                    for run in p.runs.iter().map(Some).chain([None]) {
                        let want = run.map(wanted).unwrap_or_default();
                        for (id, start) in crate::docx_comments::transition(&mut in_cell, &want) {
                            let _ = cp.add_run(&crate::docx_comments::marker(id, start));
                        }
                        let Some(run) = run else { break };
                        let mut r = cp.add_run(&run.text);
                        if run.style.bold { r = r.bold(true); }
                        if run.style.italic { r = r.italic(true); }
                        if run.style.underline { r = r.underline(true); }
                        if run.style.strikethrough { r = r.strike(true); }
                        if run.style.highlight { r = r.highlight("yellow"); }
                        if run.style.code { r = r.style("SourceText"); }
                    }
                }
            }
            // OOXML requires a paragraph in every cell, which a cell left
            // empty keeps from its creation, and one after a table that
            // ends the document or meets another; the reader drops it
            // again (`drop_table_separators`).
            if needs_separator(&paras[i..]) {
                out.add_paragraph("");
            }
            continue;
        }
        let para = &paras[i];
        i += 1;
        let level = u32::from(para.style.list_level);
        let mut p = match (para.style.list, list_ids[usize::from(para.style.list == ListKind::Numbered)]) {
            (ListKind::None, _) => out.add_paragraph(""),
            // rdocx's add_*_list_item clones the whole document on every
            // call (`clone_for_staging`), so a document with n list items
            // cost O(n²): 5,000 paragraphs took 30 s to save (#1208). It
            // only needs to allocate the list definition once; later items
            // are plain paragraphs on that numId, which is the XML the
            // builder would have written anyway.
            (ListKind::Bullet | ListKind::Numbered, Some(num_id)) => out.add_paragraph("").numbering(num_id, level),
            (ListKind::Bullet, None) => {
                let _ = out.add_bullet_list_item("", level);
                list_ids[0] = out.paragraphs().last().and_then(|p| p.numbering()).map(|(num_id, _)| num_id);
                out.last_paragraph_mut().expect("the list item just added")
            }
            // Every level "N.", as Letters draws a numbered item with no
            // label of its own: rdocx's own definition letters the second
            // level and numbers the third in roman, which Word and
            // LibreOffice showed and the reader read back as labels.
            (ListKind::Numbered, None) => {
                let levels = vec![rdocx::ListLevel::decimal(); 9];
                let id = out.add_numbering_definition(&levels).and_then(|d| out.add_numbering_instance(d, &[]));
                match id {
                    Ok(id) => {
                        list_ids[1] = Some(id);
                        out.add_paragraph("").numbering(id, level)
                    }
                    Err(_) => {
                        let _ = out.add_numbered_list_item("", level);
                        list_ids[1] = out.paragraphs().last().and_then(|p| p.numbering()).map(|(num_id, _)| num_id);
                        out.last_paragraph_mut().expect("the list item just added")
                    }
                }
            }
        };
        if let Some(level) = para.style.heading {
            p = p.style(&format!("Heading{}", level.clamp(1, 6)));
        }
        if para.style.block_quote {
            p = p.style("Quote");
        }
        if para.style.code_block.is_some() && para.style.heading.is_none() {
            p = p.style("PreformattedText");
        }
        if let Some(name) = &para.style.named_style {
            p = p.style(name);
        }
        if para.style.page_break_before {
            p = p.page_break_before(true);
        }
        if para.style.keep_with_next {
            p = p.keep_with_next(true);
        }
        if (para.style.line_spacing - 1.0).abs() > 0.01 {
            p = p.line_spacing_multiple(para.style.line_spacing as f64);
        }
        // Paragraph indents and spacing. The odt writer has carried these
        // since it was written; this one never emitted them at all, so a
        // document with an indented or spaced paragraph lost that on every
        // .docx save while keeping it on .odt. rdocx has had the builders
        // the whole time — they were simply never called.
        //
        // Only non-zero values are written: OOXML treats an absent `w:ind`
        // or `w:spacing` as "inherit from the style", and writing an
        // explicit zero is a different claim, one that overrides a style's
        // own indent with nothing.
        if para.style.list != ListKind::None {
            // A list item says where its text and marker go, as Letters
            // draws them: the text one list indent per level in from the
            // paragraph's own indent, the marker one hanging indent before
            // it. Left to rdocx's numbering definition (0.5in per level)
            // LibreOffice and Word drew our lists twice as deep as we do;
            // the edit-render journey's bullet showed it (#1201). The reader
            // takes the list's own indent back off.
            let text = para.style.left_indent_pt.max(0.0) + crate::lists::text_indent_pt(para.style.list_level);
            p = p.indent_left(rdocx::Length::pt(text)).hanging_indent(rdocx::Length::pt(crate::lists::HANGING_PT));
        } else if para.style.left_indent_pt != 0.0 {
            p = p.indent_left(rdocx::Length::pt(para.style.left_indent_pt));
        }
        if para.style.right_indent_pt != 0.0 {
            p = p.indent_right(rdocx::Length::pt(para.style.right_indent_pt));
        }
        if para.style.first_line_indent_pt != 0.0 && para.style.list == ListKind::None {
            p = p.first_line_indent(rdocx::Length::pt(para.style.first_line_indent_pt));
        }
        if para.style.space_before_pt != 0.0 {
            p = p.space_before(rdocx::Length::pt(para.style.space_before_pt));
        }
        if para.style.space_after_pt != 0.0 {
            p = p.space_after(rdocx::Length::pt(para.style.space_after_pt));
        }
        // Tab stops, which neither writer persisted: a paragraph's stops
        // were lost on every save in both formats, self round trip
        // included. The model has positions only, so every stop is a
        // left-aligned one.
        // A table of contents entry: Word's TOC style for its level, and
        // its page number at a right tab with a dot leader at the margin.
        if let Some(level) = para.style.toc {
            p = p.style(&format!("TOC{}", level.clamp(1, 9)));
            p = p.add_tab_stop_with_leader(rdocx::TabAlignment::Right, rdocx::Length::pt(text_width_pt), rdocx::TabLeader::Dot);
        } else {
            for pos in &para.style.tab_stops_pt {
                p = p.add_tab_stop(rdocx::TabAlignment::Left, rdocx::Length::pt(*pos));
            }
        }
        p = match para.style.alignment {
            Alignment::Left => p,
            Alignment::Center => p.alignment(rdocx::Alignment::Center),
            Alignment::Right => p.alignment(rdocx::Alignment::Right),
            Alignment::Justify => p.alignment(rdocx::Alignment::Justify),
        };
        let _ = p; // release the builder borrow before append_hyperlink
        // A numbered item Word would number otherwise: a restart (#1205),
        // or the first item of a list after a paragraph that ended the one
        // before, which Word would count on from it. It gets its own
        // instance of the numbered list's definition, with a start
        // override at its level, which the items after it continue.
        if let (ListKind::Numbered, Some(mut num_id)) = (para.style.list, list_ids[1]) {
            // So does an item labelled otherwise than its level of the
            // instance: the new instance's override replaces the level, and
            // keeps the labels the instance gave its other levels.
            let number = ordinals[i - 1];
            let label = &para.style.list_label;
            if word.next(num_id, level) != number || word.label(num_id, level) != label.as_ref() {
                let definition = out.numbering_instance(num_id).map(|n| n.definition_id);
                let mut labels = word.labels.get(&num_id).cloned().unwrap_or_default();
                labels[level.min(8) as usize] = label.clone();
                let overrides: Vec<rdocx::NumberingLevelOverride> = (0..9u32)
                    .filter(|&l| l == level || labels[l as usize].is_some())
                    .map(|l| rdocx::NumberingLevelOverride {
                        level: l,
                        start: (l == level).then_some(number),
                        replacement: (labels[l as usize].is_some() || word.label(num_id, l).is_some())
                            .then(|| word_level(labels[l as usize].as_ref(), l, (l == level).then_some(number))),
                        paragraph_style_link: None,
                        has_unmodeled_properties: false,
                    })
                    .collect();
                if let Some(Ok(id)) = definition.map(|d| out.add_numbering_instance(d, &overrides)) {
                    out.last_paragraph_mut().expect("the list item").set_numbering(id, level);
                    list_ids[1] = Some(id);
                    word.restart(id, level, number);
                    word.labels.insert(id, labels);
                    num_id = id;
                }
            }
            word.count(num_id, level, number);
        }
        // A table of contents is Word's TOC field around its entries.
        let toc_edge = |k: usize| paras.get(k).is_none_or(|q| q.style.toc.is_none());
        if para.style.toc.is_some() && (i < 2 || toc_edge(i - 2)) {
            let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_toc::begin_marker());
            has_toc = true;
        }
        for id in std::mem::take(&mut orphans) {
            for start in [true, false] {
                let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_comments::marker(id, start));
            }
        }
        for run in &para.runs {
            for (id, start) in crate::docx_comments::transition(&mut open, &wanted(run)) {
                let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_comments::marker(id, start));
            }
            if let Some(src) = &run.style.image {
                // Images embed via add_picture, which appends its own
                // paragraph — mid-paragraph images therefore split the
                // paragraph (documented v1 limitation). Unreadable sources
                // degrade to the alt text.
                match std::fs::read(src) {
                    Ok(bytes) => {
                        let name = std::path::Path::new(src)
                            .file_name().map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "image.png".into());
                        // The size it was shown at; every picture used to
                        // be saved 4in x 3in whatever its size.
                        let (w, h) = match run.style.image_extent_emu {
                            Some((w, h)) => (rdocx::Length::emu(w as i64), rdocx::Length::emu(h as i64)),
                            None => (rdocx::Length::inches(4.0), rdocx::Length::inches(3.0)),
                        };
                        let mut pic = out.add_picture(&bytes, &name, w, h);
                        pic = pic.style("Figure");
                        let _ = pic;
                        out.add_paragraph("");
                    }
                    Err(_) => {
                        let mut p = out.last_paragraph_mut().expect("paragraph");
                        let _ = p.add_run(&run.text);
                    }
                }
                continue;
            }
            if let Some(idx) = run.style.footnote {
                if let Some(&fid) = footnote_ids.get(idx) {
                    let mut p = out.last_paragraph_mut().expect("paragraph");
                    p.add_footnote_ref(fid);
                }
                continue;
            }
            if let Some(chip) = &run.style.chip {
                let mark = crate::docx_chips::sentinel(chips.len());
                chips.push((chip.clone(), run.text.clone()));
                match &run.style.link {
                    Some(url) => out.append_hyperlink(&mark, url),
                    None => {
                        let _ = out.last_paragraph_mut().expect("paragraph").add_run(&mark);
                    }
                }
                continue;
            }
            if let Some(url) = &run.style.link {
                // Hyperlinks need a document-level relationship; styles on
                // link text are not yet carried through append_hyperlink.
                let text = bracketed(&mut revisions, run);
                out.append_hyperlink(&text, url);
                continue;
            }
            let mut p = out.last_paragraph_mut().expect("paragraph just added");
            let text = bracketed(&mut revisions, run);
            let mut r = p.add_run(&text);
            if run.style.bold { r = r.bold(true); }
            if run.style.italic { r = r.italic(true); }
            if run.style.underline { r = r.underline(true); }
            if run.style.strikethrough { r = r.strike(true); }
            if run.style.highlight { r = r.highlight("yellow"); }
            if run.style.code { r = r.style("SourceText"); }
            if let Some(f) = &run.style.font_family { r = r.font(f); }
            if let Some(hp) = run.style.font_size_hp { r = r.size(hp as f64 / 2.0); }
            if let Some(c) = &run.style.color { r = r.color(c); }
            match run.style.vert_align {
                Some(crate::model::VertAlign::Superscript) => { r = r.superscript(); }
                Some(crate::model::VertAlign::Subscript) => { r = r.subscript(); }
                None => {}
            }
        }
        if para.style.toc.is_some() && toc_edge(i) {
            let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_toc::end_marker());
        }
        // Close the threads the next paragraph's text is not in (all of
        // them before a table, whose cells hold their own).
        let next: Vec<u32> = match paras.get(i) {
            Some(n) if n.style.table_cell.is_none() => n.runs.first().map(wanted).unwrap_or_default(),
            _ => Vec::new(),
        };
        let keep: Vec<u32> = open.iter().copied().filter(|id| next.contains(id)).collect();
        for (id, start) in crate::docx_comments::transition(&mut open, &keep) {
            let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_comments::marker(id, start));
        }
    }
    if let Some(h) = &doc.header {
        out.set_header(h);
    }
    if let Some(f) = &doc.footer {
        out.set_footer(f);
    }
    if let Some(pg) = &doc.page {
        out.set_page_size(rdocx::Length::pt(pg.width_pt), rdocx::Length::pt(pg.height_pt));
        out.set_margins(
            rdocx::Length::pt(pg.margin_top_pt),
            rdocx::Length::pt(pg.margin_right_pt),
            rdocx::Length::pt(pg.margin_bottom_pt),
            rdocx::Length::pt(pg.margin_left_pt),
        );
        // A single column is the default section layout, and `w:cols`
        // with `w:num="1"` is noise; more than one has to be written or a
        // two-column document saves as one.
        if pg.columns > 1 {
            out.set_columns(pg.columns as u32, rdocx::Length::pt(pg.column_gap_pt));
        }
    }
    let bytes = out
        .to_bytes()
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
    let bytes = with_letters_styles(&bytes, &doc.base_font, &doc.heading_styles)
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
    let tabbed = paras.iter().flat_map(|p| &p.runs).any(|r| r.text.contains('\t'));
    let bytes = if chips.is_empty() && revisions.is_empty() && doc.comments.is_empty() && !has_toc && !tabbed {
        bytes
    } else {
        with_part(&bytes, "word/document.xml", |xml| {
            let xml = if tabbed { tabs_as_elements(xml) } else { xml.to_string() };
            let xml = crate::docx_revisions::wrap(&crate::docx_chips::wrap(&xml, &chips), &revisions);
            let xml = crate::docx_comments::wrap(&xml, &doc.comments);
            if has_toc { crate::docx_toc::wrap(&xml) } else { xml }
        })
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?
    };
    let bytes = if doc.comments.is_empty() {
        bytes
    } else {
        crate::docx_comments::add_parts(&bytes, &doc.comments).map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?
    };
    // "{page}" and "{total}" in the header and footer become Word's PAGE
    // and NUMPAGES fields, so every page shows its own number.
    let fielded = |t: &Option<String>| t.as_deref().is_some_and(|t| t.contains("{page}") || t.contains("{total}"));
    let bytes = if fielded(&doc.header) || fielded(&doc.footer) {
        let parts = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).ok().and_then(|mut z| {
            Some(crate::docx_fields::default_parts(&part_text(&mut z, "word/document.xml")?, &part_text(&mut z, "word/_rels/document.xml.rels")?))
        });
        let mut bytes = bytes;
        for part in parts.into_iter().flat_map(|(h, f)| [h, f]).flatten() {
            bytes = with_part(&bytes, &part, crate::docx_fields::fields)
                .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
        }
        bytes
    } else {
        bytes
    };
    suite_common_core::atomic_save::atomic_write_bytes(path.as_ref(), &bytes)
}

/// The styles a document written by Letters must carry so that Word and
/// LibreOffice show it as Letters does.
///
/// rdocx's template says body text is Calibri 11pt with 8pt after each
/// paragraph at 1.08 line spacing, and defines only Heading 1 (16pt blue)
/// — Heading 2 to 6 fell back to body text. Letters draws none of that,
/// so every saved document looked different elsewhere, and, now that the
/// reader resolves styles, would look different in Letters after a reload.
/// This writes the document's base font with no paragraph spacing, and
/// Heading 1–6 as Letters draws them: bold, `layout::heading_scale` times
/// the body size.
fn with_letters_styles(package: &[u8], base: &crate::model::BaseFont, heading_styles: &[RunStyle]) -> Result<Vec<u8>, String> {
    let family = base.family.clone().unwrap_or_else(|| crate::layout::LayoutOptions::default().font_family);
    let size_hp = base.size_hp.unwrap_or((crate::layout::LayoutOptions::default().font_size_pt * 2.0) as u16);
    let family = xml_escape(&family);
    let defaults = format!(
        "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{family}\" w:hAnsi=\"{family}\" \
         w:eastAsia=\"{family}\" w:cs=\"{family}\"/><w:sz w:val=\"{size_hp}\"/><w:szCs w:val=\"{size_hp}\"/>\
         </w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" \
         w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>"
    );
    let headings: String = (1u8..=6)
        .map(|n| {
            // The document's own heading look if it has one, else Letters'.
            let rpr = match heading_styles.get(usize::from(n) - 1) {
                Some(h) => {
                    let mut r = String::new();
                    if let Some(f) = &h.font_family {
                        let f = xml_escape(f);
                        r.push_str(&format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>"));
                    }
                    if h.bold {
                        r.push_str("<w:b/><w:bCs/>");
                    }
                    if h.italic {
                        r.push_str("<w:i/><w:iCs/>");
                    }
                    if let Some(c) = &h.color {
                        r.push_str(&format!("<w:color w:val=\"{}\"/>", xml_escape(c)));
                    }
                    let hp = h.font_size_hp.map_or(u32::from(size_hp), u32::from);
                    r.push_str(&format!("<w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/>"));
                    r
                }
                None => {
                    let hp = (f64::from(size_hp) * crate::layout::heading_scale(n)).round() as u32;
                    format!("<w:b/><w:bCs/><w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/>")
                }
            };
            format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\"/>\
                 <w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/>\
                 <w:outlineLvl w:val=\"{lvl}\"/></w:pPr><w:rPr>{rpr}</w:rPr></w:style>",
                lvl = n - 1
            )
        })
        .collect();

    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|e| e.to_string())?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).map_err(|e| e.to_string())?;
        if name == "word/styles.xml" {
            let xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            data = patch_styles_xml(&xml, &defaults, &headings).into_bytes();
        } else if name.starts_with("word/theme/") && name.ends_with(".xml") {
            let xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            data = patch_theme_fonts(&xml, &family).into_bytes();
        }
        out.start_file(name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Name `family` as the theme's heading and body fonts (`a:majorFont` and
/// `a:minorFont`'s Latin typeface). rdocx 0.14 writes Office's default
/// theme, whose fonts are Aptos Display and Aptos: the text still renders
/// in `family` (docDefaults names it outright), but Word offers the theme's
/// fonts as the document's "(Headings)" and "(Body)" fonts, and anything
/// that resolves a theme font would get Aptos, usually not installed.
fn patch_theme_fonts(xml: &str, family: &str) -> String {
    let mut xml = xml.to_string();
    for scheme in ["<a:majorFont>", "<a:minorFont>"] {
        let Some(at) = xml.find(scheme) else { continue };
        let Some(latin) = xml[at..].find("<a:latin ").map(|l| at + l) else { continue };
        let Some(end) = xml[latin..].find("/>").map(|e| latin + e + 2) else { continue };
        xml.replace_range(latin..end, &format!("<a:latin typeface=\"{family}\"/>"));
    }
    xml
}

/// Replace `styles.xml`'s docDefaults and its Heading1–6 styles.
fn patch_styles_xml(xml: &str, defaults: &str, headings: &str) -> String {
    let mut xml = xml.to_string();
    if let (Some(a), Some(b)) = (xml.find("<w:docDefaults"), xml.find("</w:docDefaults>")) {
        xml.replace_range(a..b + "</w:docDefaults>".len(), defaults);
    } else if let Some(at) = xml.find("<w:style ") {
        xml.insert_str(at, defaults);
    }
    for n in 1..=6 {
        let id = format!("w:styleId=\"Heading{n}\"");
        while let Some(at) = xml.find(&id) {
            let Some(start) = xml[..at].rfind("<w:style ") else { break };
            let Some(len) = xml[start..].find("</w:style>") else { break };
            xml.replace_range(start..start + len + "</w:style>".len(), "");
        }
    }
    if let Some(end) = xml.rfind("</w:styles>") {
        xml.insert_str(end, headings);
    }
    // The quote, code block and inline code styles paragraphs and runs
    // name, which the template lacks (#1205). A style a paragraph names
    // but the file doesn't define is dropped by LibreOffice, so a block
    // quote and inline code read back as plain text after a pass through
    // Writer. The names are the ones Writer itself uses for these.
    for (id, def) in [
        ("Quote", "<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\" w:right=\"720\"/></w:pPr><w:rPr><w:i/></w:rPr></w:style>"),
        ("PreformattedText", "<w:style w:type=\"paragraph\" w:styleId=\"PreformattedText\"><w:name w:val=\"Preformatted Text\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Liberation Mono\" w:hAnsi=\"Liberation Mono\" w:cs=\"Liberation Mono\"/><w:sz w:val=\"20\"/></w:rPr></w:style>"),
        ("SourceText", "<w:style w:type=\"character\" w:styleId=\"SourceText\"><w:name w:val=\"Source Text\"/><w:qFormat/><w:rPr><w:rFonts w:ascii=\"Liberation Mono\" w:hAnsi=\"Liberation Mono\" w:cs=\"Liberation Mono\"/></w:rPr></w:style>"),
    ] {
        if !xml.contains(&format!("w:styleId=\"{id}\"")) {
            if let Some(end) = xml.rfind("</w:styles>") {
                xml.insert_str(end, def);
            }
        }
    }
    // Word's table of contents entry styles, which the template lacks; a
    // table of contents' entries name them (their indents are direct).
    if !xml.contains("w:styleId=\"TOC1\"") {
        let toc: String = (1..=9)
            .map(|n| format!("<w:style w:type=\"paragraph\" w:styleId=\"TOC{n}\"><w:name w:val=\"toc {n}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"39\"/><w:unhideWhenUsed/></w:style>"))
            .collect();
        if let Some(end) = xml.rfind("</w:styles>") {
            xml.insert_str(end, &toc);
        }
    }
    xml
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Write a DOCX and append previously captured, non-conflicting package parts.
pub fn write_with_opaque(
    doc: &Document,
    path: impl AsRef<std::path::Path>,
    opaque: &suite_common_core::interop::OpaquePackage,
) -> Result<(), String> {
    let path = path.as_ref();
    write(doc, path)?;
    opaque.append_to(path)
}

/// Map one rdocx paragraph (body or table cell) into a model paragraph.
/// Where a run-level page break sits in a paragraph, if there is one.
///
/// OOXML expresses a page break two ways: `w:pageBreakBefore` in the
/// paragraph properties, and a `<w:br w:type="page"/>` inside a run.
/// LibreOffice writes the second when it converts an ODF
/// `fo:break-before="page"` — as the *last* run of the paragraph
/// *before* the break, which is the same thing as this model's
/// `page_break_before` on the paragraph that follows.
///
/// A break before any text in its own paragraph means the same flag on
/// that paragraph, which is how Word writes a break the user inserted at
/// the start of a line. A break with text on both sides of it splits one
/// paragraph across pages, which this model cannot express and which
/// this therefore ignores rather than misreport as a paragraph break.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct RunPageBreak {
    /// Before any text in this paragraph.
    leading: bool,
    /// After all of this paragraph's text, so the break belongs to the
    /// next paragraph.
    trailing: bool,
}

fn run_page_break(p: &rdocx::ParagraphRef<'_>) -> RunPageBreak {
    let mut seen_text = false;
    let mut break_after_text = false;
    let mut leading = false;
    for r in p.runs() {
        for item in r.items() {
            match item {
                rdocx::RunItemRef::Break(rdocx::BreakKind::Page) => {
                    if seen_text {
                        break_after_text = true;
                    } else {
                        leading = true;
                    }
                }
                rdocx::RunItemRef::Text(t) if !t.is_empty() => {
                    seen_text = true;
                    // Text after a break means it was not trailing after all.
                    break_after_text = false;
                }
                _ => {}
            }
        }
    }
    RunPageBreak { leading, trailing: break_after_text }
}

/// A numbering level labelled `label` ("N." for `None`), Word's level
/// text: the prefix, the level's own placeholder, the suffix.
fn word_level(label: Option<&ListLabel>, level: u32, start: Option<u32>) -> rdocx::ListLevel {
    let plain = ListLabel { suffix: ".".into(), ..Default::default() };
    let label = label.unwrap_or(&plain);
    let format = match label.format {
        NumberFormat::Decimal => rdocx::ListNumberFormat::Decimal,
        NumberFormat::LowerLetter => rdocx::ListNumberFormat::LowerLetter,
        NumberFormat::UpperLetter => rdocx::ListNumberFormat::UpperLetter,
        NumberFormat::LowerRoman => rdocx::ListNumberFormat::LowerRoman,
        NumberFormat::UpperRoman => rdocx::ListNumberFormat::UpperRoman,
        NumberFormat::None => rdocx::ListNumberFormat::None,
    };
    // A "%" of the label's own would read as a placeholder.
    let literal = |t: &str| t.replace('%', "");
    let own = if label.format == NumberFormat::None { String::new() } else { format!("%{}", level + 1) };
    let level = rdocx::ListLevel::new(format).level_text(format!("{}{own}{}", literal(&label.prefix), literal(&label.suffix)));
    match start {
        Some(n) => level.start(n),
        None => level,
    }
}

/// The numbers Word will give the numbered items written so far: per list
/// instance and level, the last number shown and where a level starts.
#[derive(Default)]
struct WordNumbers {
    counts: std::collections::HashMap<u32, [Option<u32>; 9]>,
    starts: std::collections::HashMap<(u32, u32), u32>,
    /// The levels an instance labels otherwise than "N." (`word_level`).
    labels: std::collections::HashMap<u32, [Option<ListLabel>; 9]>,
}

impl WordNumbers {
    fn label(&self, num_id: u32, level: u32) -> Option<&ListLabel> {
        self.labels.get(&num_id)?[level.min(8) as usize].as_ref()
    }

    /// The number Word gives the next item at `level` of `num_id`.
    fn next(&self, num_id: u32, level: u32) -> u32 {
        let last = self.counts.get(&num_id).and_then(|c| c[level.min(8) as usize]);
        last.map_or_else(|| self.starts.get(&(num_id, level)).copied().unwrap_or(1), |n| n + 1)
    }

    fn restart(&mut self, num_id: u32, level: u32, start: u32) {
        self.starts.insert((num_id, level), start);
    }

    /// Count an item numbered `n` at `level`, which restarts the levels
    /// below it.
    fn count(&mut self, num_id: u32, level: u32, n: u32) {
        let counts = self.counts.entry(num_id).or_default();
        let at = level.min(8) as usize;
        counts[at] = Some(n);
        counts[at + 1..].iter_mut().for_each(|c| *c = None);
    }
}

/// The numbers Word gives numbered list items, in body order.
///
/// Word keeps one count per list instance (`numId`) and level. A paragraph
/// that is not in the list does not end it: the minutes that number
/// "1.1", then a paragraph of discussion, then "1.2" are one list, which
/// this model's own count (`lists::ordinals`) would restart at every plain
/// paragraph. An item restarts the levels below its own, and a level's
/// first item starts at the level's `w:start`, or at the instance's
/// restart (`list_start`, read from its `w:startOverride`).
#[derive(Default)]
struct WordCounter {
    counts: std::collections::HashMap<u32, [Option<u32>; 9]>,
}

impl WordCounter {
    /// Count the item `p`, styled `style`, and give it its label
    /// (`list_label`); Word's number for it, when it is a numbered item.
    fn number(&mut self, doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>, style: &mut ParaStyle) -> Option<u32> {
        if style.list != ListKind::Numbered {
            return None;
        }
        let (num_id, level) = paragraph_numbering(doc, p)?;
        let at = usize::from(style.list_level);
        let counts = self.counts.entry(num_id).or_default();
        counts[at + 1..].iter_mut().for_each(|c| *c = None);
        let first = |l: u32| numbering_level(doc, num_id, l).map_or(1, |l| l.0);
        let n = style.list_start.unwrap_or_else(|| counts[at].map_or_else(|| first(level), |c| c + 1));
        counts[at] = Some(n);
        // The levels above this one, as Word shows them in "%1.%2": the
        // number each is at, or its first number before any item.
        let shown = |k: u32| counts.get(k as usize).copied().flatten().unwrap_or_else(|| first(k));
        let format = |k: u32| numbering_level(doc, num_id, k).map_or(NumberFormat::Decimal, |l| l.1);
        let text = numbering_level(doc, num_id, level).and_then(|l| l.2);
        style.list_label = text.and_then(|t| word_label(&t, level, format(level), |k| (shown(k), format(k))));
        Some(n)
    }
}

/// Level `level` of list instance `num_id`: its first number, format and
/// level text. The instance's own replacement of the level (a
/// `w:lvlOverride` holding a `w:lvl`) wins over its definition's.
fn numbering_level(doc: &rdocx::Document, num_id: u32, level: u32) -> Option<(u32, NumberFormat, Option<String>)> {
    let replaced = doc.numbering_instance(num_id).and_then(|n| {
        n.level_overrides.into_iter().find(|o| o.level == level).and_then(|o| o.replacement)
    });
    if let Some(l) = replaced {
        let format = match l.format {
            rdocx::ListNumberFormat::LowerLetter => NumberFormat::LowerLetter,
            rdocx::ListNumberFormat::UpperLetter => NumberFormat::UpperLetter,
            rdocx::ListNumberFormat::LowerRoman => NumberFormat::LowerRoman,
            rdocx::ListNumberFormat::UpperRoman => NumberFormat::UpperRoman,
            rdocx::ListNumberFormat::None => NumberFormat::None,
            _ => NumberFormat::Decimal,
        };
        return Some((l.start.unwrap_or(1), format, l.level_text_value().map(str::to_string)));
    }
    let l = doc.numbering_level(num_id, level)?;
    Some((l.start, number_format(l.format_name), l.level_text.map(str::to_string)))
}

/// A numbering format name (`w:numFmt`) as this model draws it; the many
/// it has no digits for are drawn as decimals, LibreOffice's fallback.
fn number_format(name: &str) -> NumberFormat {
    match name {
        "lowerLetter" => NumberFormat::LowerLetter,
        "upperLetter" => NumberFormat::UpperLetter,
        "lowerRoman" => NumberFormat::LowerRoman,
        "upperRoman" => NumberFormat::UpperRoman,
        "none" => NumberFormat::None,
        _ => NumberFormat::Decimal,
    }
}

/// The label of an item at `level` whose level text (`w:lvlText`) is
/// `text`: the text before the item's own placeholder is its prefix and
/// the text after its suffix, each with the placeholders of the levels
/// above as `above` gives them (`%1` is level 0). With no placeholder of
/// its own, as "2.1", the text is all prefix and no number shows. `None`
/// for "N.", the label every numbered item has without one.
fn word_label(text: &str, level: u32, format: NumberFormat, above: impl Fn(u32) -> (u32, NumberFormat)) -> Option<ListLabel> {
    let own = format!("%{}", level + 1);
    let fill = |part: &str| {
        let mut out = String::new();
        let mut chars = part.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek().and_then(|d| d.to_digit(10))) {
                ('%', Some(d @ 1..=9)) => {
                    chars.next();
                    let (n, f) = above(d - 1);
                    out.push_str(&crate::lists::format_number(f, n));
                }
                _ => out.push(c),
            }
        }
        out
    };
    let label = match text.find(&own) {
        Some(at) => ListLabel { prefix: fill(&text[..at]), format, suffix: fill(&text[at + own.len()..]) },
        None => ListLabel { prefix: fill(text), format: NumberFormat::None, suffix: String::new() },
    };
    (label != ListLabel { prefix: String::new(), format: NumberFormat::Decimal, suffix: ".".into() }).then_some(label)
}

/// Give each numbered item Word's number (`WordCounter`) where this
/// model's count would show another, by restarting the count there. Where
/// the two agree nothing changes, so a save writes the list as it was.
fn number_as_word_does(paragraphs: &mut [Paragraph], word_numbers: &[Option<u32>]) {
    // A restart this model's count makes anyway is no restart: the first
    // item of a list, which a save gives its own instance starting at 1.
    for k in 0..paragraphs.len() {
        let Some(start) = paragraphs[k].style.list_start else { continue };
        paragraphs[k].style.list_start = None;
        if crate::lists::ordinals(paragraphs[..=k].iter().map(|p| &p.style))[k] != start {
            paragraphs[k].style.list_start = Some(start);
        }
    }
    loop {
        let ours = crate::lists::ordinals(paragraphs.iter().map(|p| &p.style));
        let differs = word_numbers.iter().zip(&ours).position(|(w, o)| w.is_some_and(|w| w != *o));
        let Some(k) = differs else { return };
        paragraphs[k].style.list_start = word_numbers[k];
    }
}

/// A paragraph's list numbering as `(num_id, level)`, wherever it is set.
///
/// Word's built-in list styles ("List Bullet", "List Number 2", …) carry
/// their `w:numPr` on the *style*, not the paragraph: python-docx, Word's
/// own style gallery and many templates write list items that way. Reading
/// only the paragraph's direct `w:numPr` turned every such list into plain
/// paragraphs with no marker at all.
///
/// The level is the paragraph's own `w:ilvl` when it has one. Otherwise a
/// numbered built-in style names its level ("List Bullet 3" is the third
/// level: Word gives each its own `numId` at `ilvl` 0, indented one step
/// further), and failing that the style's inherited `w:ilvl`. `numId` 0
/// means "no numbering" and switches an inherited list off.
fn paragraph_numbering(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>) -> Option<(u32, u32)> {
    if let Some((num_id, level)) = p.numbering() {
        return (num_id != 0).then_some((num_id, level));
    }
    let style_id = p.style_id()?;
    let resolved = doc.resolve_paragraph_properties(Some(style_id));
    let num_id = resolved.num_id.filter(|&id| id != 0)?;
    let level = doc
        .style(style_id)
        .and_then(|s| s.name().and_then(builtin_list_style_level))
        .or(resolved.num_ilvl)
        .unwrap_or(0);
    Some((num_id, level))
}

/// A directly numbered list paragraph's indent, as the model keeps it: the
/// declared left edge of its text (the paragraph's own `w:ind`, else its
/// numbering level's) less the list indent the layout adds for its level.
/// The writer declares exactly `left_indent + text_indent(level)`, so its
/// own lists come back as written; a Word toolbar list (0.5in per level)
/// keeps the extra depth it is drawn with in Word and LibreOffice. A list's
/// hanging indent is the list's own, not a first-line indent.
///
/// Paragraphs numbered through a built-in list style ("List Bullet") are
/// left alone: those styles' 0.25in levels are what the layout already
/// draws, and their level is the style's, not the numbering's.
fn list_indent_from_declared(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>, style: &mut ParaStyle) {
    if style.list == ListKind::None {
        return;
    }
    let Some((num_id, level)) = p.numbering().filter(|&(id, _)| id != 0) else { return };
    let text = crate::lists::text_indent_pt(style.list_level);
    let declared = if style.left_indent_pt != 0.0 {
        style.left_indent_pt
    } else {
        doc.numbering_level(num_id, level).and_then(|l| l.indent_left).map(twips_pt).unwrap_or(text)
    };
    style.left_indent_pt = (declared - text).max(0.0);
    if style.first_line_indent_pt < 0.0 {
        style.first_line_indent_pt = 0.0;
    }
}

/// The level a Word built-in list style name implies: "List Bullet" and
/// "List Number" are level 0, "List Bullet 2" … "List Bullet 5" levels 1–4.
fn builtin_list_style_level(name: &str) -> Option<u32> {
    let rest = name
        .strip_prefix("List Bullet")
        .or_else(|| name.strip_prefix("List Number"))?
        .trim();
    if rest.is_empty() {
        return Some(0);
    }
    rest.parse::<u32>().ok().filter(|n| (2..=9).contains(n)).map(|n| n - 1)
}

/// Every paragraph of `table`, its own nested tables' included, in
/// document order.
fn for_each_nested_paragraph(table: &rdocx::TableRef<'_>, f: &mut dyn FnMut(&rdocx::ParagraphRef<'_>)) {
    for ri in 0..table.row_count() {
        let Some(row) = table.row(ri) else { continue };
        for ci in 0..row.cell_count() {
            let Some(cell) = row.cell(ci) else { continue };
            for item in cell.items() {
                match item {
                    rdocx::CellItemRef::Paragraph(p) => f(&p),
                    rdocx::CellItemRef::Table(t) => for_each_nested_paragraph(&t, f),
                    _ => {}
                }
            }
        }
    }
}

/// `table_style` is the style of the table a cell paragraph is in.
fn map_paragraph(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>, table_style: Option<&str>) -> Paragraph {
    let heading = p.style_id().and_then(style_id_to_heading);
    // LO uses "Quotations"; Word uses "Quote"/"IntenseQuote".
    let block_quote = matches!(p.style_id(), Some("Quote") | Some("Quotations") | Some("IntenseQuote") | Some("BlockQuote") | Some("BlockQuotation"));
    // LibreOffice emits PreformattedText for <pre>/code blocks.
    let code_block = matches!(p.style_id(), Some("PreformattedText") | Some("HTMLPreformatted"))
        .then(String::new);
    // What the paragraph's style chain (docDefaults, basedOn, its style)
    // says, for everything the paragraph does not set itself. Numbering
    // indents are left out: the model's list level carries them.
    let mut styled = doc.resolve_paragraph_properties(p.style_id());
    if let Some(ts) = table_style {
        under_table_style(doc, &mut styled, ts);
    }
    let alignment = match p.alignment() {
        Some(rdocx::Alignment::Center) => Alignment::Center,
        Some(rdocx::Alignment::Right) => Alignment::Right,
        Some(rdocx::Alignment::Justify) => Alignment::Justify,
        Some(_) => Alignment::Left,
        None => match styled.jc {
            Some(ST_Jc::Center) => Alignment::Center,
            Some(ST_Jc::Right | ST_Jc::End) => Alignment::Right,
            Some(ST_Jc::Both | ST_Jc::Distribute) => Alignment::Justify,
            _ => Alignment::Left,
        },
    };
    let named_style = match p.style_id() {
        Some(id @ ("Title" | "Subtitle")) => Some(id.to_string()),
        _ => None,
    };
    // Either spelling counts: the paragraph property, or a run-level
    // break before this paragraph's own text.
    let page_break_before = p.is_page_break_before() || run_page_break(p).leading;
    // Keep with next: the paragraph's own setting, else its style's.
    let keep_with_next = p.keep_with_next_value().or(styled.keep_next).unwrap_or(false);
    let numbering = paragraph_numbering(doc, p);
    let (list, list_level) = match numbering {
        // The item's own level says bullet or number: a numbered list's
        // second level is often bulleted, and the reverse.
        Some((num_id, level)) => (match doc.numbering_level(num_id, level).map(|l| l.format_name == "bullet").or_else(|| doc.numbering_is_bullet(num_id)) {
            Some(false) => ListKind::Numbered,
            // Unknown num_id defaults to bullet — the safer visual guess.
            _ => ListKind::Bullet,
        }, level.min(8) as u8),
        None => (ListKind::None, 0),
    };
    // Where a list restarts: a start override on this instance at this
    // level (#1205). Only the first item of that instance carries it.
    let list_start = numbering.and_then(|(num_id, level)| {
        let start = doc.numbering_instance(num_id)?.level_overrides.iter().find(|o| o.level == level)?.start?;
        RESTARTED.with(|r| r.borrow_mut().insert((num_id, level))).then_some(start)
    });

    // Per-run link URLs from hyperlink spans (indexes into the runs vec).
    let spans = p.hyperlink_spans();
    let link_for = |idx: usize| -> Option<String> {
        spans.iter()
            .find(|(start, end, _)| idx >= *start && idx < *end)
            .and_then(|(_, _, rel_id)| rel_id.as_deref().and_then(|id| doc.hyperlink_url(id)))
    };

    let base = read_base_font(doc);
    let mut runs = Vec::new();
    for (idx, r) in p.runs().enumerate() {
        // Inline images: extract bytes to a cache file so the model's
        // image path is always locally readable.
        if let Some((rel_id, alt)) = r.inline_image() {
            if let Some(bytes) = doc.image_data(rel_id) {
                // gh-268: the previous code wrote to a predictable
                // /tmp/letters-images/<content-hash>.png via fs::write, which
                // follows a pre-created symlink — a local user could point the
                // write anywhere. The media cache writes into a private 0700
                // per-process directory under an unpredictable name. The file
                // is NOT deleted when the model is done: it lives as long as
                // this process, identical bytes share one file, and the next
                // process sweeps the directory once this one is gone (#455).
                let path = match suite_common_core::media_cache::persist(&bytes) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                // The displayed size is the drawing's extent, not the
                // image's pixels: a 200px picture placed 2in wide is 2in.
                let extent = r.items().into_iter().find_map(|item| match item {
                    rdocx::RunItemRef::Drawing(d) => d.width().zip(d.height()),
                    _ => None,
                });
                runs.push(Run {
                    text: alt.unwrap_or("").to_string(),
                    style: RunStyle {
                        image: Some(path.to_string_lossy().into_owned()),
                        image_extent_emu: extent
                            .map(|(w, h)| (w.to_emu().max(0) as u64, h.to_emu().max(0) as u64))
                            .filter(|(w, h)| *w > 0 && *h > 0),
                        ..Default::default()
                    },
                });
                continue;
            }
        }
        if let Some(fid) = r.footnote_id() {
            if let Some(idx) = doc.footnotes().iter().position(|(id, _)| *id == fid) {
                runs.push(Run {
                    text: String::new(),
                    style: RunStyle { footnote: Some(idx), ..Default::default() },
                });
            }
            continue;
        }
        let text = r.text();
        if text.is_empty() { continue; }
        // A style's font, size and colour (a heading style's 16pt blue)
        // are the run's too; only what differs from the body font is
        // recorded, so an ordinary run stays unstyled.
        let eff = if heading.is_some() {
            // A heading's look is its level (layout::heading_scale); the
            // file's heading style is not copied onto every run.
            rdocx_oxml::properties::CT_RPr::default()
        } else {
            doc.effective_run_properties(p, &r)
        };
        let family = r.font_name().map(|f| f.to_string()).or_else(|| {
            heading.is_none().then(|| style_family(p.style_id())).flatten().filter(|f| Some(f) != base.family.as_ref())
        });
        let size_hp = r
            .size()
            .map(|pt| (pt * 2.0).round() as u16)
            .or_else(|| eff.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16).filter(|hp| Some(*hp) != base.size_hp));
        // "auto" is Word's automatic (default) text colour, not a colour:
        // on the run itself it must not hide the style's either. In a
        // heading, though, the heading style's colour is drawn under every
        // run without one, so "auto" there is the run undoing it (a black
        // subtitle in a blue Heading 2): black, as Word and LibreOffice
        // draw it.
        let auto = r.color().is_some_and(|c| c.eq_ignore_ascii_case("auto"));
        let color = r
            .color()
            .filter(|c| !c.eq_ignore_ascii_case("auto"))
            .map(|c| c.trim_start_matches('#').to_uppercase())
            .or_else(|| (auto && heading.is_some()).then(|| "000000".to_string()))
            .or_else(|| eff.color.as_deref().filter(|c| !c.eq_ignore_ascii_case("auto")).map(|c| c.to_uppercase()));
        runs.push(Run {
            text,
            style: RunStyle {
                bold: r.is_bold() || eff.bold == Some(true),
                italic: r.is_italic() || eff.italic == Some(true),
                underline: r.is_underline(),
                strikethrough: r.is_strike(),
                highlight: r.highlight().is_some(),
                code: r.style_id() == Some("SourceText"),
                link: link_for(idx),
                image: None,
                image_extent_emu: None,
                footnote: None,
                html: false,
                chip: None,
                revision: None,
                comments: Vec::new(),
                font_family: family,
                font_size_hp: size_hp,
                color,
                vert_align: match r.vert_align() {
                    Some("superscript") => Some(crate::model::VertAlign::Superscript),
                    Some("subscript") => Some(crate::model::VertAlign::Subscript),
                    // LibreOffice encodes super/subscript as raised/lowered
                    // position instead of vertAlign.
                    _ => match r.position() {
                        Some(p) if p > 0 => Some(crate::model::VertAlign::Superscript),
                        Some(p) if p < 0 => Some(crate::model::VertAlign::Subscript),
                        _ => None,
                    },
                },
            },
        });
    }
    let mut para = Paragraph {
        style: ParaStyle {
            heading, alignment, list, code_block, block_quote,
            list_level, list_start,
            named_style, page_break_before, keep_with_next,
            // Absent means "inherit": the paragraph looks the way its style
            // chain says, which the model has no styles to express, so the
            // inherited value is read into the paragraph. Reading only
            // direct values drew every paragraph of a python-docx or Word
            // document without its style's 10pt spacing and 1.15 lines.
            line_spacing: p
                .line_spacing_multiple()
                .or_else(|| styled_line_multiple(&styled))
                .map(|m| m as f32)
                .unwrap_or(1.0),
            left_indent_pt: p
                .indent_left()
                .map(|l| l.to_pt())
                .or_else(|| styled.ind_left.or(styled.ind_start).map(twips_pt))
                .unwrap_or(0.0),
            right_indent_pt: p
                .indent_right()
                .map(|l| l.to_pt())
                .or_else(|| styled.ind_right.or(styled.ind_end).map(twips_pt))
                .unwrap_or(0.0),
            first_line_indent_pt: p
                .first_line_indent()
                .map(|l| l.to_pt())
                .or_else(|| {
                    styled.ind_first_line.map(twips_pt).or_else(|| styled.ind_hanging.map(|h| -twips_pt(h)))
                })
                .unwrap_or(0.0),
            space_before_pt: p.space_before().map(|l| l.to_pt()).or_else(|| styled.space_before.map(twips_pt)).unwrap_or(0.0),
            space_after_pt: p.space_after().map(|l| l.to_pt()).or_else(|| styled.space_after.map(twips_pt)).unwrap_or(0.0),
            ..Default::default()
        },
        runs,
    };
    normalize(&mut para);
    para
}

fn twips_pt(t: rdocx_oxml::Twips) -> f64 {
    f64::from(t.0) / 20.0
}

/// A cell paragraph's properties with its table's style beneath its own
/// paragraph style: Word applies the document defaults, then the table
/// style, then the paragraph style. Word's "Table Grid" sets no space
/// after and single lines; read without it, every cell of such a table
/// took the document's default 8pt after and 1.08 lines, and the table
/// drew a third taller than in Word or LibreOffice.
///
/// What the paragraph's style chain sets is what differs from the
/// defaults alone (a style naming the default's own value draws the
/// same either way, unless the table style differs; Word documents'
/// Normal style rarely sets spacing).
fn under_table_style(doc: &rdocx::Document, styled: &mut rdocx_oxml::properties::CT_PPr, table_style: &str) {
    // No style has an empty id: the defaults alone.
    let defaults = doc.resolve_paragraph_properties(Some(""));
    let table = doc.resolve_paragraph_properties(Some(table_style));
    if styled.space_before == defaults.space_before {
        styled.space_before = table.space_before;
    }
    if styled.space_after == defaults.space_after {
        styled.space_after = table.space_after;
    }
    if (styled.line_spacing, &styled.line_rule) == (defaults.line_spacing, &defaults.line_rule) {
        styled.line_spacing = table.line_spacing;
        styled.line_rule = table.line_rule.clone();
    }
    if styled.jc == defaults.jc {
        styled.jc = table.jc;
    }
}

/// A style's "auto" line spacing as a multiple of single (240 = 1.0).
/// Exact and at-least spacing have no multiple and read as unset.
fn styled_line_multiple(ppr: &rdocx_oxml::properties::CT_PPr) -> Option<f64> {
    let line = ppr.line_spacing?;
    match ppr.line_rule.as_deref() {
        None | Some("auto") if line.0 > 0 => Some(f64::from(line.0) / 240.0),
        _ => None,
    }
}

/// The theme's major (headings) and minor (body) Latin fonts, from
/// `word/theme/theme1.xml`; Office's current defaults when the file has no
/// theme or names none.
#[derive(Clone, Debug)]
struct ThemeFonts {
    major: String,
    minor: String,
}

fn theme_fonts(path: &str) -> ThemeFonts {
    let read = || -> Option<String> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/theme/theme1.xml").ok()?, &mut xml).ok()?;
        Some(xml)
    };
    let xml = read().unwrap_or_default();
    let face = |which: &str| {
        let at = xml.find(&format!("<a:{which}>"))?;
        let latin = xml[at..].find("<a:latin ")? + at;
        let tf = xml[latin..].find("typeface=\"")? + latin + "typeface=\"".len();
        let end = xml[tf..].find('"')? + tf;
        Some(xml[tf..end].to_string()).filter(|f| !f.is_empty())
    };
    ThemeFonts {
        major: face("majorFont").unwrap_or_else(|| "Calibri Light".into()),
        minor: face("minorFont").unwrap_or_else(|| "Calibri".into()),
    }
}

thread_local! {
    /// The theme of the document being read (set by `read`).
    static THEME: std::cell::RefCell<ThemeFonts> =
        std::cell::RefCell::new(ThemeFonts { major: "Calibri Light".into(), minor: "Calibri".into() });
}

/// One `w:rFonts`: a theme font (which wins over an explicit face on the
/// same element, ISO/IEC 29500 §17.3.2.26) or a named face.
#[derive(Clone, Debug, PartialEq)]
enum FontRef {
    Major,
    Minor,
    Named(String),
}

/// The fonts styles.xml names, for resolving a style's font family through
/// its `w:basedOn` chain the way Word does: the most derived style that
/// names a font decides, whether by name or by theme. rdocx's merged
/// properties keep both a base style's explicit face and a derived style's
/// theme font, and cannot say which came from where (Heading 1's theme
/// "major" font lost to Normal's "Liberation Serif").
#[derive(Default)]
struct StyleFonts {
    /// style id -> (basedOn, its own run font)
    styles: std::collections::HashMap<String, (Option<String>, Option<FontRef>)>,
    default_para: Option<String>,
    doc_default: Option<FontRef>,
}

fn style_fonts(path: &str) -> StyleFonts {
    fn scan(path: &str) -> Result<StyleFonts, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/styles.xml")?, &mut xml)?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut out = StyleFonts::default();
        let attr = |e: &quick_xml::events::BytesStart<'_>, key: &str| {
            e.attributes().with_checks(false).flatten().find(|a| a.key.as_ref() == key).map(|a| a.value.to_string())
        };
        // Where we are: inside a style (its id), inside docDefaults'
        // rPrDefault, and inside a pPr (whose rPr is the paragraph mark's).
        let (mut style, mut in_defaults, mut in_ppr) = (None::<String>, false, false);
        loop {
            let ev = reader.read_event()?;
            let (e, empty) = match &ev {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) => (e.clone(), false),
                quick_xml::events::Event::Empty(e) => (e.clone(), true),
                quick_xml::events::Event::End(e) => {
                    match e.name().as_ref() {
                        "w:style" => style = None,
                        "w:rPrDefault" => in_defaults = false,
                        "w:pPr" => in_ppr = false,
                        _ => {}
                    }
                    continue;
                }
                _ => continue,
            };
            match e.name().as_ref() {
                "w:style" if !empty => {
                    let id = attr(&e, "w:styleId").unwrap_or_default();
                    let is_para = attr(&e, "w:type").as_deref() == Some("paragraph");
                    if is_para && matches!(attr(&e, "w:default").as_deref(), Some("1" | "true")) {
                        out.default_para = Some(id.clone());
                    }
                    out.styles.entry(id.clone()).or_default();
                    style = Some(id);
                }
                "w:rPrDefault" if !empty => in_defaults = true,
                "w:pPr" if !empty => in_ppr = true,
                "w:basedOn" => {
                    if let Some(id) = &style {
                        out.styles.entry(id.clone()).or_default().0 = attr(&e, "w:val");
                    }
                }
                "w:rFonts" if !in_ppr => {
                    let font = match attr(&e, "w:asciiTheme").or_else(|| attr(&e, "w:hAnsiTheme")) {
                        Some(t) if t.starts_with("major") => Some(FontRef::Major),
                        Some(t) if t.starts_with("minor") => Some(FontRef::Minor),
                        _ => attr(&e, "w:ascii").or_else(|| attr(&e, "w:hAnsi")).map(FontRef::Named),
                    };
                    if let Some(id) = &style {
                        out.styles.entry(id.clone()).or_default().1 = font;
                    } else if in_defaults {
                        out.doc_default = font;
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
    scan(path).unwrap_or_default()
}

thread_local! {
    /// The `(num_id, level)` list restarts already given to a paragraph in
    /// the document being read: only the first item of a restart carries it.
    static RESTARTED: std::cell::RefCell<std::collections::HashSet<(u32, u32)>> = Default::default();
}

thread_local! {
    /// The styles' fonts of the document being read (set by `read`).
    static STYLE_FONTS: std::cell::RefCell<StyleFonts> = std::cell::RefCell::new(StyleFonts::default());
}

/// The font family paragraph style `id` (or the default paragraph style)
/// gives its text, resolved through `w:basedOn` and docDefaults.
fn style_family(id: Option<&str>) -> Option<String> {
    STYLE_FONTS.with(|sf| {
        let sf = sf.borrow();
        let mut cur = id.map(str::to_string).or_else(|| sf.default_para.clone());
        let mut found = None;
        for _ in 0..32 {
            let Some(c) = cur else { break };
            let Some((based_on, font)) = sf.styles.get(&c) else { break };
            if font.is_some() {
                found = font.clone();
                break;
            }
            cur = based_on.clone();
        }
        let font = found.or_else(|| sf.doc_default.clone())?;
        THEME.with(|t| {
            let t = t.borrow();
            Some(match font {
                FontRef::Major => t.major.clone(),
                FontRef::Minor => t.minor.clone(),
                FontRef::Named(n) => n,
            })
        })
    })
}

/// How the document's Heading 1–6 styles look, as `RunStyle`s; empty when
/// it defines none (then Letters' own heading look applies).
fn read_heading_styles(doc: &rdocx::Document) -> Vec<RunStyle> {
    if !(1..=6).any(|n| doc.style(&format!("Heading{n}")).is_some()) {
        return Vec::new();
    }
    (1..=6)
        .map(|n| {
            let id = format!("Heading{n}");
            let rpr = doc.resolve_run_properties(Some(&id), None);
            RunStyle {
                bold: rpr.bold == Some(true),
                italic: rpr.italic == Some(true),
                font_family: style_family(Some(&id)),
                font_size_hp: rpr.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16),
                color: rpr.color.as_deref().filter(|c| !c.eq_ignore_ascii_case("auto")).map(|c| c.to_uppercase()),
                ..Default::default()
            }
        })
        .collect()
}

/// The body font: docDefaults run properties plus the default paragraph
/// style's (python-docx and Word put "Liberation Serif 12pt" or "Calibri
/// 11pt" there, never on the runs).
fn read_base_font(doc: &rdocx::Document) -> crate::model::BaseFont {
    let rpr = doc.resolve_run_properties(None, None);
    crate::model::BaseFont { family: style_family(None), size_hp: rpr.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16) }
}

fn style_id_to_heading(id: &str) -> Option<u8> {
    let level = id.strip_prefix("Heading")?.parse::<u8>().ok()?;
    (1..=6).contains(&level).then_some(level)
}

fn normalize(p: &mut Paragraph) {
    p.runs
        .retain(|r| !r.text.is_empty() || r.style.image.is_some() || r.style.footnote.is_some());
    crate::model::merge_adjacent_runs(&mut p.runs, |a, b| {
        a.style.footnote.is_none() && b.style.footnote.is_none() && a.style == b.style
    });
}

#[cfg(test)]
mod tests {
    /// 31 nested tables, inside rdocx's own cap of 32, read on whichever
    /// thread asks: on a 2 MiB test thread they overflowed the stack before
    /// the read moved to its own. Deeper, up to [`super::MAX_XML_DEPTH`],
    /// is refused by rdocx rather than crashing, and one level past it is
    /// refused unread (#1206).
    /// A docx Letters wrote, with `body` in place of its `w:body`'s content.
    fn docx_with_body(dir: &std::path::Path, body: &str, name: &str) -> std::path::PathBuf {
        use std::io::{Read, Write};
        let base = dir.join("base.docx");
        if !base.exists() {
            super::write(&Document::from_plain_text("x"), &base).unwrap();
        }
        let xml = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
        );
        let mut source = zip::ZipArchive::new(std::fs::File::open(&base).unwrap()).unwrap();
        let path = dir.join(name);
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for i in 0..source.len() {
            let mut entry = source.by_index(i).unwrap();
            let entry_name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if entry_name == "word/document.xml" {
                bytes = xml.clone().into_bytes();
            }
            writer.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    fn para(text: &str) -> String {
        format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
    }

    /// A table nested in a cell keeps its text, in its outer cell, in
    /// document order (#1419): its two cells used to be dropped.
    #[test]
    fn a_nested_tables_text_is_read_into_its_outer_cell() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}<w:tbl><w:tr><w:tc>{}<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>{}",
            para("before"), para("outer A"), para("inner one"), para("inner two"), para("outer A, after"), para("outer B"), para("after")
        );
        let path = docx_with_body(dir.path(), &body, "nested.docx");
        let doc = super::read(path.to_str().unwrap()).unwrap();
        let cell = |row, col| -> Vec<String> {
            doc.paragraphs
                .iter()
                .filter(|p| p.style.table_cell.is_some_and(|c| (c.table, c.row, c.col) == (0, row, col)))
                .map(|p| p.text())
                .collect()
        };
        assert_eq!(cell(0, 0), ["outer A", "inner one", "inner two", "outer A, after"]);
        assert_eq!(cell(0, 1), ["outer B"]);
    }

    /// 31 nested tables, inside rdocx's own cap of 32, read on whichever
    /// thread asks: on a 2 MiB test thread they overflowed the stack before
    /// the read moved to its own. Deeper, up to [`super::MAX_XML_DEPTH`],
    /// is refused by rdocx rather than crashing, and one level past it is
    /// refused unread (#1206).
    #[test]
    fn nesting_is_read_up_to_the_limit_and_refused_past_it() {
        let dir = tempfile::tempdir().unwrap();
        let with_tables = |levels: usize, name: &str| {
            let body = format!(
                "{}<w:p><w:r><w:t>x</w:t></w:r></w:p>{}",
                "<w:tbl><w:tr><w:tc>".repeat(levels),
                "</w:tc></w:tr></w:tbl>".repeat(levels)
            );
            docx_with_body(dir.path(), &body, name)
        };
        let nested = with_tables(31, "nested.docx");
        let doc = super::read(nested.to_str().unwrap()).expect("31 nested tables read");
        // The innermost cell's text, in the outermost cell (#1419).
        let texts: Vec<String> = doc.paragraphs.iter().filter(|p| p.style.table_cell.is_some()).map(|p| p.text()).collect();
        assert_eq!(texts, ["x"]);

        // Three elements a level, between <w:document><w:body> and the
        // innermost cell's <w:p><w:r><w:t>.
        let deepest = (super::MAX_XML_DEPTH - 5) / 3;
        let inside = with_tables(deepest, "inside.docx");
        if let Err(error) = super::read(inside.to_str().unwrap()) {
            assert!(!error.contains("nests its elements"), "refused inside the limit: {error}");
        }
        let past = with_tables(deepest + 1, "past.docx");
        let error = super::read(past.to_str().unwrap()).expect_err("a docx past the limit is refused");
        assert!(error.contains("nests its elements"), "{error}");
    }

    use super::*;
    use crate::model::{Paragraph, Run, RunStyle};

    // ── style_id_to_heading ───────────────────────────────────────────────────

    #[test]
    fn style_id_heading_1_to_6_parse() {
        assert_eq!(style_id_to_heading("Heading1"), Some(1));
        assert_eq!(style_id_to_heading("Heading3"), Some(3));
        assert_eq!(style_id_to_heading("Heading6"), Some(6));
    }

    #[test]
    fn style_id_heading_out_of_range_is_none() {
        assert_eq!(style_id_to_heading("Heading0"), None);
        assert_eq!(style_id_to_heading("Heading7"), None);
    }

    #[test]
    fn style_id_heading_non_numeric_or_malformed_is_none() {
        assert_eq!(style_id_to_heading("HeadingX"), None);
        assert_eq!(style_id_to_heading("heading1"), None);
        assert_eq!(style_id_to_heading("Subtitle"), None);
    }

    // ── normalize ─────────────────────────────────────────────────────────────

    fn para_with(runs: Vec<Run>) -> Paragraph {
        Paragraph { style: Default::default(), runs }
    }

    fn run(text: &str, style: RunStyle) -> Run {
        Run { text: text.to_string(), style }
    }

    #[test]
    fn normalize_merges_adjacent_runs_with_same_style() {
        let mut p = para_with(vec![
            run("foo", RunStyle::default()),
            run("bar", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 1);
        assert_eq!(p.runs[0].text, "foobar");
    }

    #[test]
    fn normalize_keeps_runs_with_different_styles() {
        let mut p = para_with(vec![
            run("foo", RunStyle { bold: true, ..Default::default() }),
            run("bar", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 2);
    }

    #[test]
    fn normalize_drops_empty_runs() {
        let mut p = para_with(vec![
            run("", RunStyle::default()),
            run("kept", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 1);
        assert_eq!(p.runs[0].text, "kept");
    }

    #[test]
    fn normalize_does_not_merge_footnote_runs() {
        let mut p = para_with(vec![
            run("a", RunStyle { footnote: Some(0), ..Default::default() }),
            run("b", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 2);
    }
}
