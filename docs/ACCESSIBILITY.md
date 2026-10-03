# Accessibility and keyboard use

Letters, Tables and Decks can be used without a pointer, and they expose
their documents to screen readers through AT-SPI, the accessibility layer
GNOME's Orca reads. This page covers:

- the keyboard shortcuts in each app;
- how to do the everyday tasks from the keyboard;
- what a screen reader can and can't reach today.

The suite is pre-alpha. If something here doesn't hold in practice, please
[file a bug](https://github.com/tuna-os/gtk-office-suite/issues/new/choose).

## Finding commands

- **Ctrl+K** opens the command palette in every app. It lists every
  command the app has, including the ones with no shortcut. Type to filter
  the list, use Up and Down to move through it, and press Enter to run the
  selected command.
- **Ctrl+?** opens the Keyboard Shortcuts list for the app you're in.
- **F1** opens Help.

## Keyboard shortcuts

These tables match each app's Keyboard Shortcuts list (Ctrl+?). A GUI test
(`*AccessibilityDocShortcutsSmoke` in `tests/gui/test_smoke.py`) opens that list in each app and fails if
the two ever disagree.

In every app, **Ctrl+Shift+Z** redoes as well as Ctrl+Y.

### Every app

| Action | Shortcut |
|---|---|
| Command Palette | Ctrl+K |
| New Document | Ctrl+N |
| New from Template… | Shift+Ctrl+N |
| Open… | Ctrl+O |
| Save | Ctrl+S |
| Save As… | Shift+Ctrl+S |
| Preferences | Ctrl+, |
| Help | F1 |
| Keyboard Shortcuts | Ctrl+? |
| Quit | Ctrl+Q |
| Undo | Ctrl+Z |
| Redo | Ctrl+Y |

### Letters

| Action | Shortcut |
|---|---|
| Bold | Ctrl+B |
| Italic | Ctrl+I |
| Underline | Ctrl+U |
| Bullet List | Shift+Ctrl+8 |
| Numbered List | Shift+Ctrl+7 |
| Insert Link… | Shift+Ctrl+K |
| Indent List | Ctrl+] |
| Outdent List | Ctrl+[ |
| Insert Page Break | Ctrl+⏎ |
| Find and Replace | Ctrl+F |
| Page Setup… | Shift+Ctrl+L |
| Print… | Ctrl+P |
| Print Preview | Shift+Ctrl+P |
| Export as PDF… | Shift+Ctrl+E |
| Insert Footnote… | Ctrl+Alt+F |
| Toggle Ruler | Shift+Ctrl+R |
| Distraction-Free Typing | Ctrl+Alt+D |
| Insert Smart Chip… | Ctrl+Alt+C |
| Track Changes | Ctrl+Alt+T |
| Add Comment | Ctrl+Alt+M |
| Show Outline | Ctrl+Alt+O |
| Show Page Thumbnails | Ctrl+Alt+P |
| Show Tracked Changes | Ctrl+Alt+R |
| Show Comments | Shift+Ctrl+Alt+A |

Letters also accepts Markdown as you type. For example, `**word**` followed
by a space becomes bold, and `_word_` becomes italic.

### Tables

| Action | Shortcut |
|---|---|
| Edit Note… | Shift+F2 |
| Go to Cell… | Ctrl+G |
| Number Format… | Shift+Ctrl+F |

Ctrl+Alt+C also opens Edit Note….

In the grid:

- **Arrow keys** move the selection.
- **Typing** starts editing the selected cell.
- **F2** or **Enter** edits the cell's current contents in the formula bar.
- **Delete** or **Backspace** clears the cell.
- **Escape** returns from the formula bar to the grid.
- **Ctrl+G** jumps to a cell or range by name, such as `B7` or `A1:C3`. On
  a narrow window, where the name box is hidden, it opens a dialog instead.

### Decks

| Action | Shortcut |
|---|---|
| Duplicate Slide | Shift+Ctrl+D |
| Move Slide Up | Shift+Ctrl+Page_Up |
| Move Slide Down | Shift+Ctrl+Page_Down |
| Present | F5 |

To add an object, open the command palette (Ctrl+K) and type **Add Text
Box**, **Add Shape** or **Add Image…**.

## Everyday tasks from the keyboard

The automated GUI journeys run each of these with keys alone, with no
pointer, and check both the document model and the saved file:
`LettersKeyboardOnlySmoke`, `TablesKeyboardOnlySmoke` and
`DecksKeyboardOnlySmoke`.

- **Letters:** open a document, press Ctrl+End to go to the end, type,
  press Ctrl+Z to undo, type again, and press Ctrl+S to save.
- **Tables:** press Ctrl+G, type a cell name such as `B2`, press Enter,
  type a value, and press Enter again. Then press Escape to leave the
  formula bar, Ctrl+Z to undo and Ctrl+S to save.
- **Decks:** press Ctrl+K, type a command such as *Add Shape*, press
  Enter, press Ctrl+Z and Ctrl+Shift+Z to undo and redo, and press Ctrl+S
  to save.

The menus, dialogs and side panels are standard GTK and libadwaita
widgets, so Tab, Shift+Tab, the arrow keys, Enter and Escape move through
them as in other GNOME apps.

## Screen readers

What each app exposes through AT-SPI, and what automated tests check:

- **Letters:** the page view is one text area that a screen reader reads
  through GTK's accessible-text interface, including after zooming and
  scrolling. Headings, the outline, find and replace, and the dialogs are
  named controls.
- **Tables:** every visible cell is an accessible cell with its column,
  row and value. The grid's description names the selected cell and its
  value (for example "cell B2: 42"), and follows the selection.
- **Decks:** each object on a slide is an accessible object with its
  bounds. The slide list and the inspector are
  named controls.

The suite's GUI tests check names, roles, states and on-screen bounds
against the app's own model after scrolling, zooming and resizing. They
also walk each app's whole accessible tree the way a screen reader does,
to make sure the walk can't crash the app. See
[performance-accessibility.md](readiness-2026-09/performance-accessibility.md)
for the details and the evidence.

### Known limits

- **Letters can't report the bounds of individual characters** until the
  suite builds against GTK 4.16. The whole text area's bounds are right, so
  a magnifier can follow the text area but not the caret within it.
- **A full manual Orca pass has not been done yet.** Automated role
  checks are not the same as usability with a real screen reader. The
  [manual checklist](MANUAL-SCREEN-READER-CHECKLIST.md) is the plan for that
  pass, and it is a release requirement (#1208). Reports from Orca users are
  especially welcome.

## Display settings

The apps follow the system's light, dark and high-contrast styles and its
display scale. A fixed-font visual test checks every
app at widths 400, 800 and 1280, in light, dark and high contrast, at
scales 1 and 2 (`.github/workflows/visual-matrix.yml`). A light or dark
choice made in an app's own menu is remembered.

## For contributors

- New actions need a label in the action registry, so the command palette
  and the Keyboard Shortcuts list can show them. A test fails if one is
  missing.
- If you add or change a shortcut, update the tables above;
  the `*AccessibilityDocShortcutsSmoke` journeys fail if you forget.
- [GUI-TESTING-SPEC.md](GUI-TESTING-SPEC.md) describes the AT-SPI journey
  harness, and
  [MANUAL-SCREEN-READER-CHECKLIST.md](MANUAL-SCREEN-READER-CHECKLIST.md)
  describes the manual pass.
