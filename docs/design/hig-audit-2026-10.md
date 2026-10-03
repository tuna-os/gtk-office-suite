# GNOME HIG audit: Letters, Tables and Decks (October 2026)

An audit of every window, menu, popover and dialog of the three apps against
the [GNOME Human Interface Guidelines](https://developer.gnome.org/hig/) and
libadwaita's patterns.

**How it was done.** `tests/gui/hig_audit.py` opened each app in Xvfb, with a
compositor and AT-SPI. It listed the app's actions over D-Bus and opened each
one in a fresh app, keeping a screenshot whenever the screen changed. It also
captured the window itself and its main menu. That covered 76 actions in
Letters, 43 in Tables and 42 in Decks, and the screenshots were reviewed by
hand. Dialogs that open a separate window, such as Print and the file
chooser, appear at the top-left of the bare X server. On a GNOME desktop
they are centred, and in the Flatpak the print and file dialogs are the
portal's own.

Each finding below is marked **fixed** (by the PR that adds this report) or
**open**, with the issue that tracks it.

## Shared by all three apps

| # | Finding | Status |
|---|---|---|
| S1 | The formatting toolbar had no `toolbar` style. Its buttons sat flush against the header bar above and the content below, with padding only at the sides. Decks' text toolbar had the same problem. | fixed |
| S2 | The main menu button was not `primary`, so F10 did not open it. | fixed |
| S3 | The main menu's sections had headings ("File", "Edit", "Help"); GNOME primary menus use unlabelled sections. "About" did not name the app. | fixed: "About Letters" and so on |
| S4 | The About dialog showed "Hanthor Office", version 0.1.0 and no icon in every app. It now shows the app's own name, summary, developer and icon, and its latest release, all read from its metainfo. The metainfo itself was missing the 1.2.0, 2.0.0 and 2.1.0 releases. | fixed |
| S5 | Icons with the wrong metaphor. Adwaita has no highlighter, line-spacing, columns, chart, shape, table, border, merge, number-format or row and column icons, so the nearest icon was used: a lightbulb (`insert-object`) for Shape, Chart and Merge Cells; strikethrough for cell borders; the same `+` and `−` for rows and columns; an eyedropper for Highlight. The suite now ships 18 symbolic icons of its own (`suite-common/icons`, `office-*-symbolic`), which GTK recolours like theme icons. | fixed |
| S6 | An icon name the theme lacks (`chat-bubble-text-symbolic`, for the comments sidebar) was drawn as a broken image, and `funnel` and `tag` (Tables) are not in Adwaita 46. A test now checks every icon name in the apps' code against the theme and the suite's own icons. | fixed |
| S7 | Tooltips: every visible button in all three apps has a name or a tooltip. A GUI journey now holds them to that (`*ButtonsNamedSmoke`). | already true; now tested |
| S8 | Help opened a "Help & System Diagnostics" dialog with paragraphs of text and no way to reach the help pages. It also claimed "lossless round-tripping", which the apps don't do. Help now explains briefly where the user guide is and opens it with Open Online Help. | fixed |

## Letters

| # | Finding | Status |
|---|---|---|
| L1 | With one document open, the window showed no title. The tab bar was the header bar's title widget, and a tab bar with one tab hides itself. The header bar now shows the document's name, and the tab bar sits under it once a second document is open, as in GNOME Text Editor. | fixed |
| L2 | Page Setup was `GtkPageSetupUnixDialog`. That is a GTK 3-era window with its own title bar and no libadwaita styling, a printer chooser that page setup has no use for, and margins behind a further dialog. It is now a libadwaita dialog shared with Tables, with paper size, orientation and a margin for each side. | fixed |
| L3 | The sidebar's view switcher (Outline, Pages, Changes, Comments) used the wide layout, and four labels never fitted: each was cut to "…". It now uses the narrow layout, with each label under its icon. | fixed |
| L4 | The empty Comments view showed a blurry broken-image icon (see S6). | fixed |
| L5 | Preferences listed "Editor margin — Not yet implemented — coming soon" as a disabled row. The body font was a free-text entry, followed by a second row holding only its help text. The font is now a font chooser row, and the unimplemented setting is gone. | fixed |
| L6 | Headers and Footers laid out its form as centred labels above bare entries. It now uses a boxed list of entry rows. | fixed |
| L7 | The paragraph style picker ("Normal") had no dropdown arrow and read as a plain button. | fixed |
| L8 | The status bar's style readout looks like a repeat of the toolbar's style picker, but it also shows character formatting ("Heading 2 · Bold"), which the picker doesn't. | kept |

## Tables

| # | Finding | Status |
|---|---|---|
| T1 | Format Cells, Conditional Formatting, Define Name and Filter by Column were dialogs whose only content was a bare grid. They had no header bar, so no title on screen and no Cancel, only a full-width action button at the bottom. All four now have a header bar with Cancel and the action. | fixed |
| T2 | Page Setup was one of those dialogs, with a single margin for all four sides. It is now the shared Page Setup dialog (L2), with a margin per side. | fixed |
| T3 | The sheet bar's Delete Sheet button used a circled cross (`edit-delete`), which reads as "close". It is now the trash icon. | fixed |
| T4 | Tables had no Bold, Italic or Underline at all: no toolbar buttons, and Ctrl+B, Ctrl+I and Ctrl+U did nothing. They are now actions with those shortcuts, first on the toolbar, and a GUI journey checks Ctrl+B. The toolbar also carried twenty buttons, rare commands among them. It now holds eight: text styling, number format, border, merge, filter and chart. Commands on rows and columns moved to a context menu on the cells, which used to open Format Cells and nothing else. Export as PDF, Set Print Area, Freeze Rows and Protect Sheet moved to the main menu, which already had Page Setup. | fixed |
| T5 | The form dialogs (T1) laid out labels and fields in a bare grid. They now use a boxed list of rows (`suite_common::dialogs::form_rows`), each field at the end of a row named by its title, and are wide enough for their titles. | fixed |
| T6 | Delete Sheet on the only sheet opened an alert saying it couldn't be done. The button and action are now disabled when one sheet is left. | fixed |

## Decks

| # | Finding | Status |
|---|---|---|
| D1 | The labelled Insert buttons (Text, Shape, Table, Chart, Image) filled the start of the header bar and pushed the document title off centre. They are now in the toolbar row, after the text formatting buttons. | fixed |
| D2 | Shape, Chart and Table had the wrong icons (S5). | fixed |
| D3 | The text toolbar (bold, italic, underline) had no toolbar padding (S1). | fixed |

## Consistency across the apps

The first pass fixed each dialog where it stood. It left the same kind of
dialog built differently in each app, and that difference was easy to see.
Letters asked for its header and footer in an alert. Tables used header-bar
dialogs for forms. The chart dialog had its button below the preview, and
the template picker had no buttons at all. Prompts focused their field or
didn't, and had mnemonics or didn't. The save question was "Save
document?" in Letters and "Save changes?" in the other two.

[GNOME-GUIDELINES.md](../GNOME-GUIDELINES.md) now sets out the suite's
rules. Each kind of dialog (alert, prompt, action dialog, viewer) has one
builder in `suite_common::dialogs`, and every dialog in the three apps
uses it. The same pass put headings, buttons, menu items, command names
and tooltips into the HIG's header capitalization. It also gave every
button a mnemonic, and worded each kind of failure the same way in all
three apps. `tests/test_gui_guidelines.py` reads the sources and fails on
anything that goes around the builders or the writing rules.

## What was already right

- Alert dialogs (Insert Link, Insert Footnote, Rename Sheet, Can't Delete Sheet, save questions) use `AdwAlertDialog`, with the suggested action on the right.
- Preferences use `AdwPreferencesDialog`, with search.
- New from Template (Letters, Tables) and Choose a Theme (Decks) are libadwaita dialogs with header-bar actions.
- Keyboard Shortcuts, the command palette, toasts, the find bar and Decks' master-editing banner use the platform widgets.
- Dark style follows the 