# ADR 0006: Cross-suite adaptive editor shell

## Status

Accepted.

## Decision

The three editors share one responsive shell contract:

- 1280sp is the wide audit viewport; 800sp is the medium threshold where
  extended toolbar actions collapse into More; and 500sp is the narrow
  threshold where secondary controls fold while primary actions remain.
- Toolbar and contextual action buttons use a minimum 44sp target, including
  icon-only buttons. Tooltips and the action registry remain attached when a
  control moves into a popover or More menu.
- Custom Cairo/Pango renderers obtain accent/high-contrast colors from the
  active GTK/libadwaita theme, provide readable text fallbacks, and measure
  text with Pango rather than guessed character widths.

## Evidence

`SuiteWindow` owns the thresholds and `SuiteToolbar` owns the target size, so
Letters, Tables, and Decks inherit the contract. GUI journeys should capture
all three apps at 400, 800, and 1280px under light, dark, and a high-contrast
theme. Keyboard navigation and the command palette are the fallback path for
every action hidden by an adaptive breakpoint.

The selected GNOME patterns are the adaptive-layout and header-bar patterns
from `gnome-gui-spec`, with the shared `AdwToolbarView`/raised toolbar as the
cross-suite shell. Tables additionally uses the theme accent named color for
selection outlines and Pango layouts for all custom grid text. This keeps the
canvas aligned with the shell instead of maintaining an app-specific palette.
