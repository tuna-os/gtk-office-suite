# Changelog

## v1.1.0 (2026-06-24)

### Tables
- Column resize via drag on header divider with col-resize cursor
- Column auto-fit on double-click divider (PangoLayout text measurement)
- Sort on column headers (click to cycle Ascending → Descending → None)
- Number formatting: General, Number, Currency, Date, Percent, Scientific
- Cell borders: Solid, Dashed, Dotted, Double with Cairo rendering
- Freeze panes: 5-zone Cairo clip region rendering
- Merge cells: span rendering with centered text
- Data validation: List, WholeNumber, Decimal, TextLength, Regex rules
- Charts dialog: AdwDialog with Bar/Line/Pie type selector and live preview
- Undo/Redo: CellEdit, ColResize, Sort, Format, Border, Merge, Freeze commands

### Decks
- Canvas undo/redo via Command pattern (Ctrl+Z / Ctrl+Shift+Z)
- Image loading and rendering (PNG/JPEG/WebP/GIF) on slide canvas
- Inline text editing on slides (double-click TextBox → GtkTextView overlay)
- Object drag with snap-to-grid (20px) via GestureDrag
- Slide transitions: Fade, PushLeft, WipeLeft, CoverLeft, SplitHorizontal
- Speaker notes pane (collapsible GtkExpander with auto-save)
- Module refactor: canvas.rs, toolbar.rs, sidebar.rs extracted from window.rs

### suite-common
- Generic Command<T> + UndoManager<T> shared undo infrastructure
- NumberFormat engine (General, Number, Currency, Date, Percent, Scientific)
  with Excel serial date conversion
- Event Broadcaster<H> + Listener<H> system
- Maps to LibreOffice svl/ patterns: SfxUndoAction, SvNumberFormatter, SfxBroadcaster

### Infrastructure
- 15+ Rust crates leveraged (ironcalc_base, calamine, rust_xlsxwriter, num-format,
  chrono, image, zip, quick-xml, pulldown-cmark, pangocairo, regex, cairo-rs, rdocx, zspell)
- LibreOffice source at /var/home/james/dev/libreoffice-core/ used as architecture reference
- 52 GitHub issues closed total
- 10 remaining issues are v2 enhancements

## v1.0.0 (2026-06-23)

First release of the Hanthor Rust Office Suite — a GNOME-native office suite
written in Rust with GTK4 and libadwaita.

### Letters — Word Processor
- Tabbed documents with drag-to-new-window support
- Rich text formatting: Bold, Italic, Underline, Strikethrough, Highlight
- Markdown macros: type `**bold**`, `*italic*`, `# heading` for auto-formatting
- Find & Replace overlay with case-sensitive toggle
- Paragraph styles: H1-H6, Code, Blockquote
- Text alignment: Left, Center, Right, Justify
- Font size increase/decrease
- Bullet and numbered lists with auto-continuation
- Insert image (file picker + drag-and-drop), link, Markdown table
- File I/O: Markdown, HTML, DOCX, PDF export via Typst
- Spell-check toggle
- Auto-save timer
- Undo/Redo (Ctrl+Z/Y)

### Tables — Spreadsheet
- Cairo-rendered grid with column headers and row numbers
- Cell editing via double-click overlay
- Formula evaluation via IronCalc engine (83 functions)
- Multi-sheet workbooks with tab switcher
- File I/O: XLSX, ODS, CSV import/export
- Copy/paste TSV clipboard (cross-app exchange)
- Column auto-width on double-click divider

### Decks — Presentations
- Slide sidebar with AdwOverlaySplitView
- Cairo slide canvas with Pango text rendering
- Shapes: rectangles, circles
- Image insertion via file picker
- Fullscreen present mode with keyboard navigation
- Slide management: add, delete, reorder
- File I/O: PPTX via zip + OpenXML

### Infrastructure
- All three apps build as Flatpaks
- GSettings for preferences persistence
- GNOME HIG-compliant UI (libadwaita, AdwTabView, AdwHeaderBar)
- Keyboard shortcuts with AdwShortcutsDialog
- Dark mode support (system + manual toggle)
- Responsive toolbar breakpoints
- CI: cargo check, clippy, test, Flatpak build
