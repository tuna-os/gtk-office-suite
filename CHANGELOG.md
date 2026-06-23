# Changelog

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
