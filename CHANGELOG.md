# Changelog

## Unreleased

### Letters
- Spell check follows your language: it loads the hunspell dictionary for your locale (from LANGUAGE, LC_ALL, LC_MESSAGES or LANG) and falls back to US English only when none is installed. Words with accented or non-Latin letters, such as "café" and "naïve", are checked whole instead of being split at the accent, and a curly apostrophe counts as one.
- Live collaboration over Durable Streams, behind the off-by-default `collab` feature (RFC-0005 P5): Share This Document… and Join a Shared Document… in the main menu, a status button, and a banner with Keep Mine, Keep Theirs and Open Both when two versions exist. Tables, footnotes, headers and footers, sections and comments are refused with a message, not shared in part. A peer's edit moves your caret over it instead of to the end of the change, and Undo takes back only your own steps.
- Screen readers now get bold, italic, underline and strikethrough from the page view as text attributes.
- Export as PDF With Typst (Approximate) works in the Flatpak: it typesets in process instead of running a `typst` program the app never shipped, does it without freezing the window, leaves no `.typ` file beside the PDF, and typesets the document itself on its own page size rather than the screen's list markers and table pipes. Text holding `#`, `$` or `//` no longer fails the export (#1985).
- Save As Markdown now warns before it drops a document's tables and footnotes: Markdown here writes table cells as plain paragraphs and leaves footnote text out (#1981).
- Underlines (on underlined text, links and tracked insertions) are drawn where LibreOffice Writer draws them: lower and thicker than before, so a bold underline no longer runs into the letters.
- The Letters Flatpak no longer sees the whole session runtime directory (`/run/user/<uid>`, where the session's service sockets live); it sees only the GVfs mounts under it, which is what it needed it for (#1996).
- Misspelled words get their red wavy underline in the page view again. The spell checker found them, but only the old editor drew the underline; it still leaves print and PDF alone (#1894).
- Typing quickly in a long document no longer queues a whole-document spell check and word count for every keystroke: each runs once when typing pauses (#1990).
- A picture that was missing when the page was first drawn, or is replaced at the same path, now shows without reopening the document, and pictures removed from a document no longer stay in memory until the tab closes (#1992).

### Tables
- Insert Pivot Table… (cell menu and command palette) makes a pivot table from the selected table: rows by one field, optionally columns by another, and a value summarized by sum, count, average, min or max. It is written where Place At says, two columns to the right by default, as one undo step. Refresh Pivot Tables (cell menu and command palette) rewrites the sheet's pivot tables from their sources after the data changes, as one undo step. Results save as values; the pivot itself is not kept on save, and the save warning says so (#1657).
- A cell reference past the last column (XFD) or row, or long enough to overflow, is now rejected instead of crashing or wrapping to a wrong cell, whether typed in the name box or read from a chart or conditional-format range in a file (#1988).
- Print scale and fit-to-page are read from XLSX and kept on save. They used to be dropped, so a sheet set to print at 46% or one page wide saved back at 100%.
- Export to PDF prints at the sheet's print scale, or shrinks it to fit the pages its fit-to-page asks for, as Calc does. It also prints the used range or print area with each column at its own width, where it used to put every cell in one column, and leaves hidden rows and columns out.
- Opening an ODS reads each sheet's page setup from its page style: paper size, orientation, margins, print scale and fit to pages wide and tall.
- Opening, saving and editing a filtered sheet no longer stalls on a long column of different values: listing a 50 000-row column's values took seconds and now takes a fraction of one (#1994).
- A workbook whose content or styling reaches far past its data, such as a lone cell near XFD1048576, is refused with a message naming the sheet instead of hanging or running out of memory while it opens. A sheet opens at up to 10 million cells (rows × columns from A1), a workbook at up to 20 million together, and opening no longer visits the blank cells inside that range.
- Sorting moves each cell's lock and hidden-formula setting with its row, as it already moved formats, styles, borders, validation and notes. They used to stay put, so a sort on a sheet with unlocked cells locked the wrong ones.

### Decks
- PowerPoint groups that are rotated or flipped open where PowerPoint draws them, instead of upright and unflipped. Decks has no group object yet, so saving a deck that had groups now warns that its shapes will be saved ungrouped.
- Undo keeps the last 200 steps. A long editing session used to keep every step it ever made, whole slides included, so memory grew for as long as the window stayed open (#1993).
- Links on slide text to a web or mail address survive opening and saving, in PowerPoint and ODF files and in table cells. They used to be dropped on save, with a warning. Jumps to another slide, click actions and links on a whole shape still warn before a save drops them (#1998).

### Infrastructure
- Lifafa, the email client, is now built and tested in CI (in a Fedora 43 container, since it needs libadwaita 1.7 and WebKitGTK 6.0), and its lockfile has its own RustSec gate, which also runs daily. Its lock moves to patched rustls-webpki, webpki-roots, event-listener and pulldown-cmark, and off two yanked crates (#1986).
- Fuzzing now covers the collaboration byte paths as well as the document importers: the sidecar protocol codec, dcp-core's update ingest and its package check, each in the nightly smoke run.
### Decks
- A PowerPoint file saved by Decks stores each picture once, however many slides show it, and names it for what it is: a JPEG photo used to be saved as a `.png` part, and a logo on thirty slides was stored thirty times (#1984).

## v1.0 (post-release) — 2026-06-24

Features added after the v1.0 tag and GitHub release.

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
- Wrapped merged cells auto-fit row height to the merged column span.

### Decks
- Canvas undo/redo via Command pattern (Ctrl+Z / Ctrl+Shift+Z)
- Image loading and rendering (PNG/JPEG/WebP/GIF) on slide canvas
- Inline text editing on slides (double-click TextBox → GtkTextView overlay)
- Rich text in the in-place editor (DCK-007): bold, italic, underline, strikethrough, font, size and colour on the selected characters; alignment, lists, indent and spacing on the paragraphs it touches; its own undo, styled copy/paste, and one undo step on Apply — saved in PPTX and ODP
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
- LibreOffice source at ~/dev/libreoffice-core/ used as architecture reference
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
