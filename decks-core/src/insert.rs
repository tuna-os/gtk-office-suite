// insert.rs — what the Insert buttons put on a slide.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "Insert buttons, not menus" (iWork's Text · Shape ·
// Table · Media row): the objects each button inserts, in the theme's
// default look, placed in the middle of the slide, and the shape library
// the Shape button's popover lists and searches. GTK-free and unit-tested;
// decks/src/insert_bar.rs builds the buttons.

use crate::engine::chart::{ChartData, ChartKind};
use crate::engine::shape::{ShapeKind, ShapeStyle};
use crate::engine::table::{TableCell, TableData};
use crate::engine::SlideObject;

/// One entry in the shape library.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryShape {
    pub name: &'static str,
    /// Other words it's found by.
    pub keywords: &'static str,
    pub kind: ShapeKind,
}

/// The shapes the library offers, in the order it shows them.
pub fn shape_library() -> Vec<LibraryShape> {
    vec![
        LibraryShape { name: "Rectangle", keywords: "square box", kind: ShapeKind::Rect },
        LibraryShape { name: "Rounded Rectangle", keywords: "square box round", kind: ShapeKind::RoundRect { radius: 1.0 / 6.0 } },
        LibraryShape { name: "Ellipse", keywords: "circle oval round", kind: ShapeKind::Ellipse },
        LibraryShape { name: "Triangle", keywords: "arrow up", kind: ShapeKind::Triangle },
        LibraryShape { name: "Diamond", keywords: "rhombus decision", kind: ShapeKind::Diamond },
    ]
}

/// Whether `shape` matches the search `query` (any word, case-insensitive,
/// against its name and keywords; an empty query matches everything).
pub fn matches(shape: &LibraryShape, query: &str) -> bool {
    let hay = format!("{} {}", shape.name, shape.keywords).to_lowercase();
    query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

/// `(x, y)` that centres a `w`×`h` box on the 960×540 slide.
fn centred(w: f64, h: f64) -> (f64, f64) {
    ((960.0 - w) / 2.0, (540.0 - h) / 2.0)
}

/// A new text box.
pub fn text_box() -> SlideObject {
    let (w, h) = (360.0, 60.0);
    let (x, y) = centred(w, h);
    SlideObject::TextBox { text: "Text".into(), x, y, w, h, rotation: 0.0, runs: vec![], body: Default::default() }
}

/// A new shape of `kind`, in the theme's default fill and outline.
pub fn shape(kind: ShapeKind) -> SlideObject {
    let (w, h) = if kind == ShapeKind::Rect || matches!(kind, ShapeKind::RoundRect { .. }) { (200.0, 150.0) } else { (160.0, 160.0) };
    let (x, y) = centred(w, h);
    SlideObject::Shape { kind, x, y, w, h, rotation: 0.0, style: ShapeStyle::default() }
}

/// A new `rows`×`cols` table in PowerPoint's default style: a header row
/// and banded rows, empty cells.
pub fn table(rows: usize, cols: usize) -> SlideObject {
    let (rows, cols) = (rows.max(1), cols.max(1));
    let (w, h) = ((cols as f64 * 160.0).min(880.0), (rows as f64 * 40.0).min(460.0));
    let (x, y) = centred(w, h);
    SlideObject::Table {
        x,
        y,
        w,
        h,
        rotation: 0.0,
        table: TableData {
            col_widths: vec![w / cols as f64; cols],
            row_heights: vec![h / rows as f64; rows],
            rows: vec![vec![TableCell::default(); cols]; rows],
            first_row: true,
            band_row: true,
            accent: None,
            cell_margins: None,
        },
    }
}

/// The kinds the Chart button offers, in the order it shows them; the
/// `insert-chart` action's target is an index into this.
pub const CHART_KINDS: [ChartKind; 5] = [ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Pie, ChartKind::Scatter];

/// A new chart of `kind` with PowerPoint's sample series, 480 x 300 in
/// the middle of the slide.
pub fn chart(kind: ChartKind) -> SlideObject {
    let (w, h) = (480.0, 300.0);
    let (x, y) = centred(w, h);
    SlideObject::Chart { x, y, w, h, rotation: 0.0, chart: ChartData::sample(kind) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::obj_bounds;

    #[test]
    fn a_chart_is_inserted_in_the_middle_with_the_sample_series() {
        for kind in CHART_KINDS {
            let c = chart(kind);
            assert_eq!(obj_bounds(&c), (240.0, 120.0, 480.0, 300.0));
            let SlideObject::Chart { chart, .. } = c else { panic!() };
            assert_eq!((chart.kind, chart.series.as_str(), chart.points.len()), (kind, "Sales", 4));
        }
    }

    #[test]
    fn the_library_is_searched_by_name_and_keyword() {
        let lib = shape_library();
        let names = |q: &str| lib.iter().filter(|s| matches(s, q)).map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(names("").len(), lib.len());
        assert_eq!(names("TRI"), ["Triangle"]);
        assert_eq!(names("circle"), ["Ellipse"]);
        assert_eq!(names("round"), ["Rounded Rectangle", "Ellipse"]);
        assert_eq!(names("round box"), ["Rounded Rectangle"]);
        assert!(names("hexagon").is_empty());
    }

    #[test]
    fn inserted_objects_sit_in_the_middle_of_the_slide() {
        for o in [text_box(), shape(ShapeKind::Diamond), shape(ShapeKind::Rect), table(3, 3)] {
            let (x, y, w, h) = obj_bounds(&o);
            assert!(((x + w / 2.0) - 480.0).abs() < 1e-9 && ((y + h / 2.0) - 270.0).abs() < 1e-9, "{o:?}");
        }
    }

    #[test]
    fn a_table_has_its_grid_and_the_default_style() {
        let SlideObject::Table { table, w, h, .. } = table(3, 4) else { panic!() };
        assert_eq!((table.rows.len(), table.rows[0].len()), (3, 4));
        assert_eq!((table.col_widths.iter().sum::<f64>(), table.row_heights.iter().sum::<f64>()), (w, h));
        assert!(table.first_row && table.band_row);
        let SlideObject::Table { table, .. } = super::table(0, 0) else { panic!() };
        assert_eq!(table.rows.len(), 1, "never empty");
    }
}
