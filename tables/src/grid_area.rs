// grid_area.rs — DrawingArea subclass exposing spreadsheet cells as
// virtual AT-SPI children (issue #87, deep half).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// GTK4 pattern for non-widget accessibles: each cell is a plain GObject
// implementing GtkAccessible; the grid overrides
// get_first_accessible_child, cells chain via get_next_accessible_sibling
// and point back via get_accessible_parent. Screen readers then see
// role=cell nodes with real names and bounds instead of one opaque
// image.

use gtk4::{self as gtk, glib, prelude::*, subclass::prelude::*};
use std::cell::RefCell;

use tables_core::sheet::{col_label, COL_HEADER_HEIGHT, ROW_HEADER_WIDTH, ROW_HEIGHT};

// ── CellAccessible: one virtual cell ─────────────────────────────────

mod imp_cell {
    use super::*;
    use std::cell::{Cell, OnceCell};

    #[derive(Default)]
    pub struct CellAccessible {
        pub grid: OnceCell<glib::WeakRef<super::GridArea>>,
        pub row: Cell<usize>,
        pub col: Cell<usize>,
        pub context: RefCell<Option<gtk::ATContext>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CellAccessible {
        const NAME: &'static str = "TablesCellAccessible";
        type Type = super::CellAccessible;
        type ParentType = glib::Object;
        type Interfaces = (gtk::Accessible,);
    }

    impl ObjectImpl for CellAccessible {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: std::sync::OnceLock<Vec<glib::ParamSpec>> = std::sync::OnceLock::new();
            PROPS.get_or_init(|| {
                vec![glib::ParamSpecOverride::for_interface::<gtk::Accessible>(
                    "accessible-role",
                )]
            })
        }
        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "accessible-role" => gtk::AccessibleRole::Cell.to_value(),
                _ => unimplemented!(),
            }
        }
        fn set_property(&self, _id: usize, _value: &glib::Value, _pspec: &glib::ParamSpec) {
            // The role is fixed at Cell.
        }
    }

    impl AccessibleImpl for CellAccessible {
        fn at_context(&self) -> Option<gtk::ATContext> {
            if let Some(ctx) = self.context.borrow().as_ref() {
                return Some(ctx.clone());
            }
            let grid = self.grid.get()?.upgrade()?;
            let display = grid.display();
            let ctx = gtk::ATContext::create(
                gtk::AccessibleRole::Cell,
                self.obj().upcast_ref::<gtk::Accessible>(),
                &display,
            )?;
            *self.context.borrow_mut() = Some(ctx.clone());
            Some(ctx)
        }

        fn accessible_parent(&self) -> Option<gtk::Accessible> {
            self.grid
                .get()
                .and_then(|w| w.upgrade())
                .map(|g| g.upcast::<gtk::Accessible>())
        }

        fn next_accessible_sibling(&self) -> Option<gtk::Accessible> {
            let grid = self.grid.get()?.upgrade()?;
            grid.imp().next_sibling_of(self.row.get(), self.col.get())
        }

        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            None
        }

        fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
            let grid = self.grid.get()?.upgrade()?;
            let (x, w) = grid.imp().col_span(self.col.get());
            let y = COL_HEADER_HEIGHT + self.row.get() as f64 * ROW_HEIGHT;
            Some((x as i32, y as i32, w as i32, ROW_HEIGHT as i32))
        }

        fn platform_state(&self, _state: gtk::AccessiblePlatformState) -> bool {
            false
        }
    }
}

glib::wrapper! {
    pub struct CellAccessible(ObjectSubclass<imp_cell::CellAccessible>)
        @implements gtk::Accessible;
}

impl CellAccessible {
    fn new(grid: &GridArea, row: usize, col: usize) -> Self {
        let cell: Self = glib::Object::builder().build();
        let weak = glib::WeakRef::new();
        weak.set(Some(grid));
        cell.imp().grid.set(weak).ok();
        cell.imp().row.set(row);
        cell.imp().col.set(col);
        cell
    }

    fn update(&self, value: &str, selected: bool) {
        let (row, col) = (self.imp().row.get(), self.imp().col.get());
        let name = if value.is_empty() {
            format!("{}{}, empty", col_label(col), row + 1)
        } else {
            format!("{}{}: {}", col_label(col), row + 1, value)
        };
        self.update_property(&[gtk::accessible::Property::Label(&name)]);
        self.update_state(&[gtk::accessible::State::Selected(Some(selected))]);
    }
}

// ── GridArea: the drawing area exposing the cells ────────────────────

mod imp_grid {
    use super::*;

    #[derive(Default)]
    pub struct GridArea {
        pub cells: RefCell<Vec<CellAccessible>>,
        pub cols: std::cell::Cell<usize>,
        pub col_widths: RefCell<Vec<f64>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GridArea {
        const NAME: &'static str = "TablesGridArea";
        type Type = super::GridArea;
        type ParentType = gtk::DrawingArea;
        // Re-declare the interface: without this the AccessibleImpl
        // overrides below are never registered and GTK keeps using the
        // widget defaults (no virtual children).
        type Interfaces = (gtk::Accessible,);
    }

    impl ObjectImpl for GridArea {}
    impl WidgetImpl for GridArea {}
    impl DrawingAreaImpl for GridArea {}

    impl AccessibleImpl for GridArea {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            self.cells
                .borrow()
                .first()
                .map(|c| c.clone().upcast::<gtk::Accessible>())
        }
    }

    impl GridArea {
        pub fn next_sibling_of(&self, row: usize, col: usize) -> Option<gtk::Accessible> {
            let cols = self.cols.get();
            if cols == 0 {
                return None;
            }
            let idx = row * cols + col + 1;
            self.cells
                .borrow()
                .get(idx)
                .map(|c| c.clone().upcast::<gtk::Accessible>())
        }

        /// x-origin and width of a column, in widget coordinates.
        pub fn col_span(&self, col: usize) -> (f64, f64) {
            let widths = self.col_widths.borrow();
            let x: f64 = ROW_HEADER_WIDTH + widths.iter().take(col).sum::<f64>();
            let w = widths.get(col).copied().unwrap_or(90.0);
            (x, w)
        }
    }
}

glib::wrapper! {
    pub struct GridArea(ObjectSubclass<imp_grid::GridArea>)
        @extends gtk::DrawingArea, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for GridArea {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}

impl GridArea {
    /// Rebuild/update the virtual cells for the used data region (plus
    /// the selection), refreshing names, selection state, and geometry.
    /// The child set is rebuilt only when the exposed region grows.
    pub fn sync_cells(
        &self,
        data: &[Vec<String>],
        col_widths: &[f64],
        sel: (usize, usize, usize, usize),
    ) {
        // Used extent: rows/cols containing data, plus the selection.
        let mut max_r = sel.2;
        let mut max_c = sel.3;
        for (r, row) in data.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                if !v.is_empty() {
                    if r > max_r {
                        max_r = r;
                    }
                    if c > max_c {
                        max_c = c;
                    }
                }
            }
        }
        let rows = (max_r + 1).min(data.len());
        let cols = (max_c + 1).min(data.first().map(|r| r.len()).unwrap_or(0));

        *self.imp().col_widths.borrow_mut() = col_widths.to_vec();

        // Persistent flat child list, grown by appending and linked via
        // update_next_accessible_sibling (rebuilding the chain leaves
        // the AT-SPI bridge with stale references — it then reports no
        // children at all). Cells are addressed row-major over a column
        // count that only grows; row/col labels are re-assigned on
        // geometry changes and surplus cells are hidden.
        let grid_cols = self.imp().cols.get().max(cols);
        let needed = rows.max(1) * grid_cols.max(1);
        // Append without holding the borrow across GTK calls —
        // set_accessible_parent re-enters first_accessible_child.
        loop {
            let (len, prev) = {
                let cells = self.imp().cells.borrow();
                (cells.len(), cells.last().cloned())
            };
            if len >= needed {
                break;
            }
            let cell =
                CellAccessible::new(self, len / grid_cols.max(1), len % grid_cols.max(1));
            self.imp().cells.borrow_mut().push(cell.clone());
            cell.set_accessible_parent(Some(self), None::<&CellAccessible>);
            if let Some(prev) = prev {
                prev.update_next_accessible_sibling(Some(&cell));
            }
        }
        self.imp().cols.set(grid_cols);

        let cells = self.imp().cells.borrow();
        for (idx, cell) in cells.iter().enumerate() {
            let (r, c) = (idx / grid_cols.max(1), idx % grid_cols.max(1));
            cell.imp().row.set(r);
            cell.imp().col.set(c);
            if r < rows && c < cols {
                cell.update_state(&[gtk::accessible::State::Hidden(false)]);
                let value = data.get(r).and_then(|row| row.get(c));
                let selected = r >= sel.0 && r <= sel.2 && c >= sel.1 && c <= sel.3;
                cell.update(value.map(String::as_str).unwrap_or(""), selected);
            } else {
                cell.update_state(&[gtk::accessible::State::Hidden(true)]);
            }
        }
    }
}
