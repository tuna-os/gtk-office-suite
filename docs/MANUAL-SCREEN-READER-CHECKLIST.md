# Manual Screen-Reader (Orca) Journey Checklist

This document provides a manual testing guide for validating keyboard navigation and screen-reader (Orca) accessibility across the GTK Office Suite editors (**Letters**, **Tables**, **Decks**).

---

## 1. Letters (Word Processor)

### Journey: Create, Edit, Format, Find/Replace, and Save
1. **Launch Letters**: Start `letters`.
   - **Expected Orca announcement**: "Letters, window, Untitled Document — Letters, document text".
2. **Keyboard Navigation & Structure**:
   - Focus is automatically placed in the main `GtkTextView`.
   - Type headings `# Heading 1`, list items `- Item 1`, links `[label](http://example.com)`, tables, and bold/italic text using keyboard shortcuts (`Ctrl+B`, `Ctrl+I`, etc.).
   - Navigate paragraph-by-paragraph with Up/Down arrow keys. Orca announces formatting tags, headings, links, and list items.
3. **Find & Replace**:
   - Press `Ctrl+F` to open the search bar.
   - Orca announces: "Find, text entry".
   - Type search term and press `Enter` (or `Shift+Enter`). Orca announces match counts and match navigation.
   - Tab through search bar controls: "Previous match", "Next match", "Replace", "Replace All", "Case sensitive", and "Close search bar".
4. **Document Save**:
   - Press `Ctrl+S` to open the file chooser dialog.
   - Type filename and press `Enter` to save.

---

## 2. Tables (Spreadsheet)

### Journey: Cell Navigation, Entry, Formatting, Formula Bar, and Zoom
1. **Launch Tables**: Start `tables`.
   - **Expected Orca announcement**: "Tables, window, Spreadsheet grid, table".
2. **Virtual Cell Navigation**:
   - Use Arrow keys (`Up`, `Down`, `Left`, `Right`) to navigate cells.
   - Orca announces cell coordinate and value (e.g. "A1, empty", "B2: 42").
   - Cell AT-SPI bounds report correct screen extents after scrolling or zooming.
3. **Formula & Text Entry**:
   - Press `F2` or start typing to edit cell content or press `Ctrl+L` / click formula bar.
   - Type `=SUM(A1:A10)` and press `Enter`.
   - Orca announces updated cell values and calculated results.
4. **Selection & Undo**:
   - Press `Shift+Arrow` to extend selection. Orca announces selection bounds.
   - Press `Ctrl+Z` to undo cell edits. Orca announces state change.

---

## 3. Decks (Presentation Editor)

### Journey: Canvas Navigation, Object Direct Manipulation, and Layout
1. **Launch Decks**: Start `decks`.
   - **Expected Orca announcement**: "Decks, window, Slide canvas, list".
2. **Canvas Object AT-SPI Children**:
   - Press `Tab` or Arrow keys to move focus between slide objects on the canvas.
   - Orca announces object type, label, and selection state (e.g. "Text box: Title, list item, selected", "Rectangle, list item").
   - Object bounds dynamically update during resize, drag-move, and zoom.
3. **Object Direct Manipulation**:
   - Use arrow keys / shortcuts to move or resize objects (`Ctrl+Arrow`).
   - Contextual properties panel / sidebar updates accessible properties live.
4. **Slide Navigation**:
   - Use `Page Down` / `Page Up` to navigate slides. Orca announces slide number and object counts.

---

## 4. AT-SPI Automated Verification Suite

Run automated AT-SPI accessibility checks via cargo:
```bash
cargo test --workspace
```
