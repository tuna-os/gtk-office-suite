# Walkthrough screenshots

Generated — do not edit by hand. The `Screenshots` GitHub Actions
workflow (weekly + manual dispatch) builds the apps, drives them through
`tests/gui/walkthrough.py` under Xvfb, and commits the refreshed PNGs
here. Regenerate locally with:

```bash
cargo build --bin letters --bin tables --bin decks
tests/gui/capture_walkthrough.sh docs/screenshots
```

| File | Shows |
|---|---|
| `letters.png` | Heading + body text, style dropdown, live status bar |
| `letters-palette.png` | Ctrl+K command palette over the document |
| `tables.png` | Data + `=SUM` formula, name box, range selection with live Sum/Avg/Count |
| `decks.png` | Slide canvas fit-to-viewport, object inspector, presenter pill |
