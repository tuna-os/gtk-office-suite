# GUI Guidelines: Letters, Tables and Decks

These are the suite's rules for its interface. They follow the
[GNOME Human Interface Guidelines](https://developer.gnome.org/hig/) as
closely as an office suite can, and say how each rule is met in this code.
Where the HIG leaves a choice open, this document makes it once for all
three apps, so that the same kind of thing looks and works the same way in
each of them.

`tests/test_gui_guidelines.py` holds the code to the rules marked **(tested)**.
The `*ButtonsNamedSmoke` journeys and `tests/adaptive_editor_contract.py`
check others in the running apps.

## 1. The window

```
AdwApplicationWindow
└── AdwToastOverlay
    └── AdwToolbarView
        ├── top:     AdwHeaderBar   [New][Open]   document title   [app actions][Main Menu]
        ├── top:     AdwTabBar      (Letters, once a second document is open)
        ├── top:     formatting toolbar (.toolbar)
        ├── content: the document (with AdwOverlaySplitView for a sidebar)
        └── bottom:  status bar / sheet bar
```

- The window is an `AdwApplicationWindow`, built by `suite_common::SuiteWindow`.
- The header bar comes from `suite_common::make_header_bar`. Its title is the
  document's name, and it is always centred: nothing else is packed so wide that
  it pushes the title aside. Labelled buttons belong in the toolbar row.
- The **main menu** is the last button at the end of the header bar
  (`open-menu-symbolic`, tooltip "Main Menu", `primary`, so F10 opens it). Its
  sections are unlabelled, as in GNOME's own apps. "About Letters" (and so on)
  is the last item.
- The **formatting toolbar** is a box with the `toolbar` style class, which gives
  it the standard padding above, below and between buttons. Bold, Italic and
  Underline come first in every app.
- **Tabs** (Letters only) are an `AdwTabBar` under the header bar, never the
  header bar's title widget. With one document open, the header bar shows its
  name and the tab bar hides itself.
- **Narrow windows**: below 700sp (`suite_common`'s shared breakpoint) sidebars
  collapse and secondary toolbar controls hide. A control is hidden only when
  its action stays reachable from a menu or the command palette. Touch targets
  are at least 44sp (`apply_touch_target`, ADR 0006).

## 2. Dialogs

The HIG's rules for every dialog: it has a parent window and is modal to it.
It has a heading. The affirmative button is a specific verb, never "OK" or
"Yes" for a choice. Cancel comes first. Enter does the affirmative action,
unless that action is destructive. Escape cancels. Keyboard focus starts on
the control the user will use first.

Each dialog in the suite is one of the kinds below. Each kind has one builder
in `suite_common::dialogs`. Apps build dialogs only through those builders,
never from `adw::Dialog` or `adw::AlertDialog` fields directly **(tested)**.
That is what keeps a Letters dialog and a Tables dialog of the same kind
alike.

| Kind | Use it for | Builder | Looks like |
|---|---|---|---|
| **Alert** | Telling the user something failed, or asking a question before doing something they may not want (closing unsaved work, saving into a format that loses content) | `adw::AlertDialog`; `show_error_dialog`, `save_changes_question`, `confirm_discarding` | Heading, body text, a row of buttons: Cancel then the verb |
| **Prompt** | Asking for exactly one line of text (Rename Sheet, Insert Link, Go to Cell, Define Name, Add Comment) | `dialogs::prompt` | An alert with one text field, focused, under the body |
| **Action dialog** | Collecting several settings before doing something (Page Setup, Format Cells, Headers and Footers, Conditional Formatting), or choosing one thing from many (New From Template, Choose a Theme, Insert Chart) | `dialogs::action_dialog` + `dialogs::form_body` | Header bar with the title, Cancel at its start, the verb at its end (blue); the form or chooser below |
| **Viewer** | Showing something with nothing to decide (Keyboard Shortcuts) | `dialogs::viewer_dialog` | Header bar with the title and a close button |
| **Preferences** | The app's settings | `adw::PreferencesDialog` (`suite_common::make_preferences_window`) | libadwaita's preferences, with search |
| **About** | The app's name, version, developer and licence | `suite_common::about::show` (`AdwAboutDialog`, from the metainfo) | libadwaita's about dialog |
| **System** | Opening and saving files, printing | `gtk::FileDialog`, `gtk::PrintOperation` | The desktop's own (the portal's under Flatpak) |

The command palette is the one deliberate exception. It is a chromeless
`adw::Dialog` that is all search field, as in GNOME Text Editor and Builder.

### Choosing the kind

- **One field means a prompt; two or more mean an action dialog.** A form never
  goes inside an alert. Only `dialogs::prompt` puts a field into one **(tested)**.
- Settings that change as you touch them go in a **popover** or the
  **inspector**, not a dialog. Examples: the text colour, a cell's border, a
  slide's layout.
- A destructive action that can be undone needs no confirmation. Do it, and
  say so in a toast with an Undo button (HIG: "Undo is preferred"). Deleting a
  sheet, which Undo restores, asks first only because the whole sheet goes.
- Success is a **toast** ("Exported to Report.pdf"), never an alert. A failure
  the user must act on is an alert. One that needs no action is a toast.
- GTK's own dialog windows are not used: `GtkDialog`, `GtkMessageDialog`,
  `GtkPageSetupUnixDialog`, `GtkPrintUnixDialog`, `GtkAboutDialog`,
  `GtkShortcutsWindow` and file chooser windows **(tested)**. They have their
  own title bars and none of libadwaita's styling. This rule is why Page Setup
  is `suite_common::page_setup`, not GTK's.

### Alerts

- **Heading**: a short question or statement, in header capitalization
  **(tested)**. A question ends with "?".
- **Body**: sentence capitalization, one or two sentences. It says what will
  happen, or what went wrong and what to do about it. Name the document in
  quotes: “Budget.xlsx”.
- **Buttons**: Cancel first, then the action, named by its verb. Every label
  has a mnemonic (`_Save`) and header capitalization **(tested)**.
  - A destructive action is red (`Destructive`) and is never the default. When
    the alert's whole purpose is that action, Cancel is the default.
  - A safe action is blue (`Suggested`) and is the default.
  - Escape is the close response, and it cancels.
- **Errors** use `show_error_dialog`, and the heading is one of these: "Could
  Not Open File", "Could Not Save File", "Could Not Export PDF", or "Cannot …"
  for a refusal. The body gives the reason. The button is "_OK".
- **Closing unsaved work** asks `save_changes_question` in every app: "Save
  Changes?", naming the document, with Cancel, Discard (red) and Save (blue,
  default). With several documents it lists them, and the buttons become
  Discard All and Save All.

### Prompts

`dialogs::prompt(heading, body, field_name, text, action)` gives an alert
with Cancel and `action` as its buttons, and one `gtk::Entry`:

- The field is named for screen readers (`field_name`, in sentence case:
  "Sheet name"). It has the keyboard focus when the dialog opens, with its text
  selected. Enter in it does the action.
- A placeholder may show an example ("https://", "For example, TaxRate"),
  but never replaces the heading or the field's name.
- When some input can't be accepted, disable the action
  (`set_response_enabled`) while the field is invalid. Say why in the body, and
  give the field the `error` style. An alert closes on any response, so the
  input can't be refused after the button is pressed. Define Name works this
  way.

### Action dialogs

`dialogs::action_dialog(title, action, width, content)`:

- The header bar shows the title. Cancel is at its start and the action at its
  end, in blue. The action is the default widget, so Enter does it. Escape and
  Cancel close without changing anything.
- Widths are 360 to 440 for a form, 520 for a list, 600 to 800 for a chooser
  with previews. A title must fit without being cut short.
- **Forms** are `dialogs::form_body(&[groups])`: `adw::PreferencesGroup`s of
  libadwaita rows, spaced 12 above, 24 at the sides and below, and 24 between
  groups. Use the row type for the value:
  - `adw::EntryRow` for text;
  - `adw::SpinRow` for a number, with its unit in the subtitle ("Millimetres");
  - `adw::ComboRow` for one choice from a list;
  - `adw::SwitchRow` for on and off;
  - `adw::ActionRow` with the control as its suffix only when no row type holds
    the control, such as a colour button.
  A row's title uses header capitalization ("Decimal Places"). A group may have
  a title ("Margins") and a description for help text ("Type {page} where the
  page number goes"). A row that doesn't apply to the current choice is made
  insensitive, not hidden.
- **Choosers** put the choices in the content area, with the first one (or the
  current one) selected. The action makes the selected choice ("_Create",
  "_Insert"). Activating a choice (a double click, or Enter on a row) makes it
  too.

## 3. Writing

From the HIG's [writing style](https://developer.gnome.org/hig/guidelines/writing-style.html):

- **Header capitalization** is used for headings (dialog headings, header bar
  titles, group titles), tab titles, button labels, menu items, row titles and
  **tooltips**. Capitalize:
  - all words of four letters or more;
  - verbs and nouns of any length;
  - the first and last word;
  - both halves of a hyphenated word ("Self-Test").

  Articles, conjunctions and short prepositions (a, the, and, or, to, in, of,
  as, at, by, for, on) stay lower case. Examples: "Save as Plain Text?", "New
  From Template", "Go to Cell" **(tested for dialogs and tooltips)**.
- **Sentence capitalization** is used for body text, descriptions, check box
  labels, field names ("Link address"), and placeholders.
- Be brief and neutral. Don't say "please", and don't address the user as "you"
  where it can be avoided. Use the user's words, not the code's ("sheet", not
  "worksheet object"). Write "for example", not "e.g.".
- Text doesn't end with a period unless it is more than one sentence.
- An ellipsis (…, U+2026, not three dots) ends a menu item or button label
  that opens a dialog before anything happens ("Page Setup…", "Save As…"). A
  label that acts at once has none.
- Typographic characters: “quotes”, the en dash (–) in ranges, and × in
  dimensions.

## 4. Tooltips

- Every control in a header bar has a tooltip (HIG). So does every icon-only
  button anywhere. If one control in a container has a tooltip, they all do.
  Every visible button has a name or a tooltip **(tested in each app,
  `*ButtonsNamedSmoke`)**.
- A tooltip is short, uses header capitalization and has no final period
  **(tested)**. It names the action ("Insert Chart"). It doesn't repeat a
  visible label. A shortcut may follow in parentheses: "Next Match (Enter)".
- Never put information only in a tooltip. Tooltips don't appear on touch, and
  not everyone hears them.

## 5. Icons

- Use symbolic icons (`-symbolic`) from the Adwaita theme. Where Adwaita has no
  icon with the right meaning, the suite ships its own in `suite-common/icons`
  (`office-*-symbolic`). GTK recolours them like theme icons.
- Never use an icon whose meaning is wrong (a light bulb for "insert shape", a
  circled cross for "delete"), however close it looks. Draw one instead.
- Every icon name in the apps must exist in the theme or among the suite's own
  icons. A missing one draws as a broken image (`icons::tests`).
- Never use emoji as icons.

## 6. Menus and commands

- Every command is a `gio` action. A menu item, a button and a shortcut name
  the same action, so they stay in step and are disabled together. A command
  that can't run is disabled rather than offered and then refused. Delete Sheet
  is disabled on the last sheet.
- Menu items use header capitalization. They are grouped in unlabelled
  sections, end in "…" when they open a dialog, and show their shortcut.
- Every command is in the command palette (Ctrl+K), and every shortcut is in
  Keyboard Shortcuts and in `docs/ACCESSIBILITY.md`.
- Standard shortcuts mean the same in all three apps: Ctrl+N, Ctrl+O, Ctrl+S,
  Ctrl+Shift+S, Ctrl+P, Ctrl+Z, Ctrl+Shift+Z, Ctrl+F, Ctrl+B/I/U, Ctrl+?, F10.

## 7. Feedback

- **Toasts** (`suite_common::toast_manager`) report a finished action: saved,
  exported, deleted with Undo. They never carry information the user has to
  act on.
- **Alerts** report a failure that leaves the user with something to do (§2).
  A failure is never only a log line: under Flatpak nobody reads stderr.
- **Banners** (`AdwBanner`) report a lasting state of the document, such as
  Decks' master-editing mode.
- **Status pages** (`AdwStatusPage`) fill a view that can be empty: the empty
  Comments view, and the start page.

## 8. Accessibility

- Every feature works from the keyboard alone, and focus is always visible.
- Every control has an accessible name. For an icon button, it comes from its
  tooltip. For a field, the name is its row title or `field_name`.
- Colour is never the only signal. A conditional-format fill goes with the
  value, and an error field goes with text that says what is wrong.
- Custom-drawn surfaces (the Tables grid, the Decks canvas, the Letters page)
  expose their content to AT-SPI. See `docs/ACCESSIBILITY.md` and
  `MANUAL-SCREEN-READER-CHECKLIST.md`.

## 9. Style

- Spacing comes from the GNOME scale: 6 between related controls, 12 between
  groups of controls and as a container's padding, 24 between sections and
  around a dialog's form.
- Use libadwaita's style classes (`toolbar`, `boxed-list`, `card`, `heading`,
  `dim-label`, `caption`, `suggested-action`, `destructive-action`, `flat`,
  `linked`, `error`), not custom CSS, for anything they cover.
- Use the system font and the system's light or dark style. The app's own Dark
  Style toggle overrides the system's and is remembered.

## Reference

- GNOME HIG: https://developer.gnome.org/hig/
  - [Dialogs](https://developer.gnome.org/hig/patterns/feedback/dialogs.html)
  - [Boxed lists](https://developer.gnome.org/hig/patterns/containers/boxed-lists.html)
  - [Tooltips](https://developer.gnome.org/hig/patterns/feedback/tooltips.html)
  - [Writing style](https://developer.gnome.org/hig/guidelines/writing-style.html)
- libadwaita: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-5/
- The audit these rules came out of: `docs/design/hig-audit-2026-10.md`.
- Layout and feature direction: `docs/DESIGN-UI.md`.
