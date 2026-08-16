# Comprehensive GNOME HIG Audit Framework

A systematic 12-category audit framework for verifying GNOME HIG compliance in
GTK4/libadwaita applications. Can be run in parallel sub-agents for speed.

## Audit Categories

### Batch 1 — Frontend/UI Focus

| # | Category | Scope | What to Check |
|---|----------|-------|---------------|
| 1 | **Text** | All user-visible strings | Sentence case, ellipsis (…), menu item conventions, tooltip text, empty state descriptions, preference labels |
| 2 | **Buttons** | All interactive widgets | Toolbar icons exist in Adwaita theme, labels use sentence case, toggle vs push buttons, flat style on header bar buttons |
| 3 | **Layout/margins** | Window chrome | Margin consistency, AdwToolbarView usage, OverlaySplitView sidebar sizing, status bar patterns, AdwStatusPage empty states, default window sizes |
| 4 | **Icons** | Every icon_name reference | Verify each icon name against `/usr/share/icons/Adwaita/symbolic/actions/` — report missing icons with valid alternatives |
| 5 | **Keyboard shortcuts** | All accelerators | Standard GNOME bindings (Ctrl+N/O/S/Z/Y/P/Q), no conflicting shortcuts, all actions have accelerators |
| 6 | **Accessibility** | Labels, roles, tooltips | Accessible labels on all widgets, AccessibleRole on DrawingArea, tooltip text on icon-only buttons, keyboard navigation, focus indicators |

### Batch 2 — Theme/Pattern/Infrastructure Focus

| # | Category | Scope | What to Check |
|---|----------|-------|---------------|
| 7 | **Color scheme** | UI colors | Hardcoded colors vs Adwaita CSS, dark mode palette, accent color on primary actions, contrast ratios, AdwStyleManager usage |
| 8 | **Dialog patterns** | All dialogs | AlertDialog response ordering (Cancel/Destructive/Suggested), modal vs non-modal, parent window anchoring, preferences dialog structure |
| 9 | **Responsive design** | Window sizing | AdwBreakpoint usage, responsive toolbar collapse, minimum window sizes, narrow layout, adaptive sidebar |
| 10 | **CSS classes** | Adwaita styling | Valid Adwaita CSS class names (flat, pill, suggested-action, destructive-action, navigation-sidebar, linked, caption), no inline CSS where class would work |
| 11 | **GSettings schema** | Schema files | Kebab-case key naming, proper types, valid defaults, range constraints, schema ID matches app ID |
| 12 | **i18n/accessibility** | Translation readiness | Hardcoded strings that should be translatable, RTL layout support, text direction handling, focus indicators |

## Running the Audit

### Using Pi Sub-Agents (Parallel)

```bash
# Batch 1 — UI focus
pi subagent --chain '[
  {"agent": "oracle", "task": "Audit text..."},
  {"agent": "oracle", "task": "Audit buttons..."},
  {"agent": "oracle", "task": "Audit layout..."},
  {"agent": "oracle", "task": "Audit icons..."},
  {"agent": "oracle", "task": "Audit shortcuts..."},
  {"agent": "oracle", "task": "Audit accessibility..."}
]'

# Batch 2 — Infrastructure focus
pi subagent --chain '[
  {"agent": "oracle", "task": "Audit colors..."},
  {"agent": "oracle", "task": "Audit dialogs..."},
  {"agent": "oracle", "task": "Audit responsive..."},
  {"agent": "oracle", "task": "Audit CSS..."},
  {"agent": "oracle", "task": "Audit GSettings..."},
  {"agent": "oracle", "task": "Audit i18n..."}
]'
```

### Verdict Output Format

Each sub-agent reports in this structured format:

```json
{
  "category": "text",
  "total_violations": 12,
  "critical": 0,
  "minor": 12,
  "violations": [
    {"file": "letters/src/window.rs:231", "issue": "sentence case", "text": "Bullet List", "suggested": "Bullet list", "severity": "minor"}
  ],
  "clean": true
}
```

## Committing Fixes

After the audit, fix all violations by category order, committing per category:

```bash
git commit -m "fix(gnome-hig): <category> — <summary of fixes>"
```

## Source Repositories

- **Tool for GNOME audits**: https://github.com/tuna-os/suite-common
- **Office suite being audited**: https://github.com/tuna-os/gtk-office-suite
