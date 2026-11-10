// The conversion itself: reads the input with the same core the desktop apps
// use and writes the output through the same save path (ADR 0015).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Phase 2a: the text category (md/odt/docx in; odt/docx/md/html out). The
// spreadsheet and presentation engines and PDF/PNG follow.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::json;
use suite_common_core::interop::{CompatibilityReport, FeatureDisposition};

use crate::{Args, Category, Exit, Format, Input, LossLevel, Output, Plan};

/// One thing the conversion could not carry across.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Loss {
    pub id: String,
    pub label: String,
    pub location: String,
    pub level: LossLevel,
    pub detail: String,
}

/// A finished conversion.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    pub losses: Vec<Loss>,
}

impl Outcome {
    /// The worst loss, `None` when nothing was lost.
    pub fn worst(&self) -> LossLevel {
        self.losses
            .iter()
            .map(|l| l.level)
            .max()
            .unwrap_or(LossLevel::None)
    }

    /// Whether the loss goes over a `--fail-on-loss` budget.
    pub fn over(&self, budget: LossLevel) -> bool {
        self.worst() > budget
    }

    /// The `--report-loss` JSON.
    pub fn report_json(&self, plan: &Plan) -> String {
        let losses: Vec<_> = self
            .losses
            .iter()
            .map(|l| json!({ "id": l.id, "label": l.label, "location": l.location, "level": l.level.name(), "detail": l.detail }))
            .collect();
        let value = json!({ "from": plan.from.name(), "to": plan.to.name(), "worst": self.worst().name(), "losses": losses });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }
}

/// Why a conversion stopped, with the exit code it maps to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Failure {
    pub exit: Exit,
    pub message: String,
}

impl Failure {
    fn new(exit: Exit, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
        }
    }
}

/// How a core feature's disposition counts against a loss budget.
fn level(disposition: FeatureDisposition) -> LossLevel {
    match disposition {
        FeatureDisposition::OpaquePassThrough => LossLevel::None,
        FeatureDisposition::WarnOnLoss => LossLevel::Minor,
        FeatureDisposition::MustPreserve => LossLevel::Major,
        FeatureDisposition::HardError => LossLevel::Critical,
    }
}

fn losses(report: &CompatibilityReport) -> Vec<Loss> {
    report
        .features
        .iter()
        .map(|f| Loss {
            id: f.id.clone(),
            label: f.label.clone(),
            location: f.location.clone(),
            level: level(f.disposition),
            detail: f.detail.clone(),
        })
        .collect()
}

/// A file the engine can name: the real path, or a temporary copy of stdin.
struct Source {
    path: PathBuf,
    _keep: Option<tempfile::TempDir>,
}

fn source(args: &Args, from: Format) -> Result<Source, Failure> {
    match &args.input {
        Input::Path(p) => {
            if !p.is_file() {
                return Err(Failure::new(
                    Exit::Usage,
                    format!("`{}` is not a file", p.display()),
                ));
            }
            Ok(Source {
                path: p.clone(),
                _keep: None,
            })
        }
        Input::Stdin => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|e| Failure::new(Exit::Convert, format!("reading stdin: {e}")))?;
            let dir = tempfile::tempdir()
                .map_err(|e| Failure::new(Exit::Convert, format!("temporary directory: {e}")))?;
            let path = dir.path().join(format!("stdin.{}", from.name()));
            std::fs::write(&path, bytes)
                .map_err(|e| Failure::new(Exit::Convert, format!("temporary file: {e}")))?;
            Ok(Source {
                path,
                _keep: Some(dir),
            })
        }
        Input::Batch(_) => Err(Failure::new(
            Exit::Usage,
            "--batch is handled before a single conversion",
        )),
    }
}

/// Convert one document. For a text input the format a reader uses follows the
/// extension, so a stdin copy keeps the one `--from` named.
pub fn convert(args: &Args, plan: &Plan) -> Result<Outcome, Failure> {
    let engine = match plan.from.category() {
        Some(Category::Text)
            if matches!(
                plan.to,
                Format::Md | Format::Odt | Format::Docx | Format::Html | Format::Pdf | Format::Png
            ) =>
        {
            Engine::Text
        }
        Some(Category::Spreadsheet)
            if matches!(
                plan.to,
                Format::Xlsx | Format::Csv | Format::Pdf | Format::Html | Format::Png
            ) =>
        {
            Engine::Sheet
        }
        Some(Category::Presentation) if matches!(plan.to, Format::Odp | Format::Pptx) => {
            Engine::Deck
        }
        _ => {
            return Err(Failure::new(
                Exit::Convert,
                format!(
                    "converting {} to {} has no engine wired in yet",
                    plan.from.name(),
                    plan.to.name()
                ),
            ))
        }
    };

    let input = source(args, plan.from)?;
    // Write to a temporary file named for the target format, then deliver it,
    // so stdout and a bad path behave alike.
    let scratch = tempfile::tempdir()
        .map_err(|e| Failure::new(Exit::Convert, format!("temporary directory: {e}")))?;
    let staged = scratch.path().join(format!("out.{}", plan.to.name()));
    // PNG is the PDF drawn: stage the PDF through the same engine, then raster it.
    let target = if plan.to == Format::Png {
        Format::Pdf
    } else {
        plan.to
    };
    let pdf_stage = scratch.path().join("out.pdf");
    let engine_out = if plan.to == Format::Png {
        &pdf_stage
    } else {
        &staged
    };
    let found = match engine {
        Engine::Text => text(&input.path, engine_out, target)?,
        Engine::Sheet => sheet(&input.path, engine_out, target)?,
        Engine::Deck => deck(&input.path, &staged)?,
    };
    if plan.to == Format::Png {
        let pdf = std::fs::read(&pdf_stage)
            .map_err(|e| Failure::new(Exit::Convert, format!("reading the staged pdf: {e}")))?;
        let png = rasterise(pdf, args.page, args.size)?;
        std::fs::write(&staged, png)
            .map_err(|e| Failure::new(Exit::Convert, format!("writing png: {e}")))?;
    }
    match &args.output {
        Some(Output::Path(p)) => deliver(&staged, p)?,
        Some(Output::Stdout) => {
            let bytes = std::fs::read(&staged)
                .map_err(|e| Failure::new(Exit::Convert, format!("reading the output: {e}")))?;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| Failure::new(Exit::Convert, format!("writing stdout: {e}")))?;
        }
        None => return Err(Failure::new(Exit::Usage, "no output given")),
    }
    Ok(Outcome { losses: found })
}

enum Engine {
    Text,
    Sheet,
    Deck,
}

/// Page `page` (from 1) of a PDF as a PNG `width` pixels wide.
pub fn rasterise(pdf: Vec<u8>, page: u32, width: u32) -> Result<Vec<u8>, Failure> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{render, PixmapSettings, RenderCache, RenderSettings};

    let pdf = Pdf::new(pdf)
        .map_err(|e| Failure::new(Exit::Convert, format!("reading the pdf: {e:?}")))?;
    let pages = pdf.pages();
    let index = (page as usize).saturating_sub(1);
    let Some(page_ref) = pages.get(index) else {
        return Err(Failure::new(
            Exit::Usage,
            format!(
                "--page {page} is out of range: the document has {} page(s)",
                pages.len()
            ),
        ));
    };
    let (natural_width, _) = page_ref.render_dimensions();
    let scale = width as f32 / natural_width;
    let pixmap = render(
        page_ref,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        },
    );
    pixmap
        .into_png()
        .map_err(|e| Failure::new(Exit::Convert, format!("encoding png: {e:?}")))
}

fn deck(input: &Path, staged: &Path) -> Result<Vec<Loss>, Failure> {
    let (deck, report, _opaque) = decks_core::read_deck_with_report(&input.to_string_lossy())
        .map_err(|e| Failure::new(Exit::Parse, e))?;
    decks_core::write_deck(&staged.to_string_lossy(), &deck)
        .map_err(|e| Failure::new(Exit::Convert, e))?;
    Ok(losses(&report))
}

fn text(input: &Path, staged: &Path, to: Format) -> Result<Vec<Loss>, Failure> {
    let doc = letters_core::save::read(input).map_err(|e| Failure::new(Exit::Parse, e))?;
    if to == Format::Pdf {
        // The in-process Typst export: approximate by design (ADR 0010), so
        // the report says so rather than implying print fidelity.
        let source = letters_core::typst_export::typst_source(&doc);
        suite_export::compile_pdf_to_file(&source, &staged.to_string_lossy())
            .map_err(|e| Failure::new(Exit::Convert, e))?;
        return Ok(vec![Loss {
            id: "typst-approximate".into(),
            label: "Approximate PDF".into(),
            location: "whole document".into(),
            level: LossLevel::Minor,
            detail:
                "PDF is the Typst re-typesetting of the document's Markdown, not the print layout."
                    .into(),
        }]);
    }
    let report =
        letters_core::save::write(&doc, staged).map_err(|e| Failure::new(Exit::Convert, e))?;
    Ok(losses(&report))
}

fn sheet(input: &Path, staged: &Path, to: Format) -> Result<Vec<Loss>, Failure> {
    let path = input.to_string_lossy();
    let (engine, sheets) =
        tables_core::io::load_workbook(&path).map_err(|e| Failure::new(Exit::Parse, e))?;
    let mut found = Vec::new();
    match to {
        Format::Xlsx => {
            tables_core::io::save_sheets_to_xlsx_with_engine(
                &staged.to_string_lossy(),
                &sheets,
                Some(&engine),
            )
            .map_err(|e| Failure::new(Exit::Convert, e))?;
        }
        Format::Html => {
            std::fs::write(staged, html(&sheets))
                .map_err(|e| Failure::new(Exit::Convert, format!("writing html: {e}")))?;
        }
        Format::Csv | Format::Pdf => {
            // One table: the first sheet, as the values it displays.
            if sheets.len() > 1 {
                found.push(Loss {
                    id: "single-sheet-output".into(),
                    label: "Other sheets".into(),
                    location: sheets[1..]
                        .iter()
                        .map(|s| s.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    level: LossLevel::Major,
                    detail: format!(
                        "{} holds one sheet here; only the first was written.",
                        to.name().to_uppercase()
                    ),
                });
            }
            let first = sheets
                .first()
                .ok_or_else(|| Failure::new(Exit::Parse, "the workbook has no sheets"))?;
            if to == Format::Pdf {
                tables_core::export::to_pdf_for_sheet(&engine, first, &staged.to_string_lossy())
                    .map_err(|e| Failure::new(Exit::Convert, e))?;
            } else {
                std::fs::write(staged, csv(first))
                    .map_err(|e| Failure::new(Exit::Convert, format!("writing csv: {e}")))?;
            }
        }
        _ => unreachable!("convert() routes only xlsx, csv, pdf and html here"),
    }
    Ok(found)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A standalone HTML page: one table per sheet, each under its name, holding
/// the values the sheet displays.
pub fn html(sheets: &[tables_core::sheet::SheetModel]) -> String {
    let mut out = String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Spreadsheet</title></head><body>\n");
    for sheet in sheets {
        out.push_str(&format!(
            "<h2>{}</h2>\n<table border=\"1\">\n",
            escape_html(&sheet.name)
        ));
        if let Some((last_row, last_col)) = sheet.used_extent() {
            for r in 0..=last_row {
                out.push_str("<tr>");
                for c in 0..=last_col {
                    out.push_str(&format!("<td>{}</td>", escape_html(sheet.cell(r, c))));
                }
                out.push_str("</tr>\n");
            }
        }
        out.push_str("</table>\n");
    }
    out.push_str("</body></html>\n");
    out
}

/// RFC 4180 text of a sheet's used range.
pub fn csv(sheet: &tables_core::sheet::SheetModel) -> String {
    let Some((last_row, last_col)) = sheet.used_extent() else {
        return String::new();
    };
    let mut out = String::new();
    for r in 0..=last_row {
        for c in 0..=last_col {
            if c > 0 {
                out.push(',');
            }
            let cell = sheet.cell(r, c);
            if cell.contains([',', '"', '\n', '\r']) {
                out.push('"');
                out.push_str(&cell.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(cell);
            }
        }
        out.push_str("\r\n");
    }
    out
}

fn deliver(staged: &Path, to: &Path) -> Result<(), Failure> {
    let bytes = std::fs::read(staged)
        .map_err(|e| Failure::new(Exit::Convert, format!("reading the output: {e}")))?;
    suite_common_core::atomic_save::atomic_write_bytes(to, &bytes)
        .map_err(|e| Failure::new(Exit::Convert, format!("writing `{}`: {e}", to.display())))
}
