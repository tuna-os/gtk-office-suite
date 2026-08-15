# Hanthor Office Suite — Technical Specification

## Overview

The Hanthor Office Suite is a **GNOME-native** office productivity suite written in **Rust** using GTK4 and libadwaita. It consists of three applications:

1. **Letters** — Word processor with tabbed documents
2. **Decks** — Presentation editor with slide canvas
3. **Tables** — Spreadsheet with formula evaluation

---

## Architecture

### Component Diagram

```
┌─────────────────────────────────────────┐
│             Suite-Common                 │
│  ┌─────────┐ ┌────────┐ ┌───────────┐  │
│  │ SuiteWin │ │ Toast  │ │ FileDlg   │  │
│  └─────────┘ └────────┘ └───────────┘  │
└─────────────────────────────────────────┘
         ▲             ▲             ▲
         │             │             │
┌─────────┐   ┌────────┐   ┌──────────┐
│ Letters │   │ Decks  │   │  Tables  │
└─────────┘   └────────┘   └──────────┘
```

### Key Design Decisions

- *GTK4 + libadwaita* for native GNOME look and feel
- *Cairo* for canvas rendering (Decks) and spreadsheet grid (Tables)
- *Markdown* as the native document format for Letters
- *zspell* (pure Rust) for spell checking
- *Typst* for PDF export

### Data Flow

1. User types in Letters editor (GtkTextBuffer)
2. Text is stored as Markdown internally
3. On save, text is written to `.md` file
4. On export, Markdown is converted to Typst → PDF

---

## Usage Examples

```rust
// Create a new document
let doc = Document::from_text("# Hello World\n\nThis is **bold** text.");
let path = "hello.md";
doc.save(path).expect("Save failed");
```

## Formatting Features

| Feature | Letters | Decks | Tables |
|---------|---------|-------|--------|
| Bold | ✅ | ✅ | ❌ |
| Italic | ✅ | ✅ | ❌ |
| Underline | ✅ | ✅ | ❌ |
| Bullet Lists | ✅ | ❌ | ❌ |
| Numbered Lists | ✅ | ❌ | ❌ |
| Formulas | ❌ | ❌ | ✅ |
| Charts | ❌ | ❌ | ✅ |
| Transitions | ❌ | ✅ | ❌ |

---

## Installation

```bash
git clone https://github.com/tuna-os/gtk-office-suite.git
cd gtk-office-suite
cargo build --release
./target/release/letters
```

*For more information, see the README.md in the project root.*
