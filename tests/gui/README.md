# Deterministic GUI journeys

`framework/base.py` is the shared AT-SPI harness for the smoke journeys. Each
test launches with a stable locale, UTC timezone, fixed scale, theme, font
aliases, disabled animations, and `SOURCE_DATE_EPOCH=0`. Tests that touch user
data should layer `isolate_xdg()`, `isolate_gsettings()`, or
`isolate_autosave_state()` onto that environment.

Use `wait_for_condition()` and `wait_for_node()` for observable readiness
instead of sleeping for a guessed duration. `trigger_snapshot()` invokes the
test-only `test-snapshot` action and waits for the JSON write to complete. The
action is registered by the application only when `GTK_OFFICE_TEST_MODE=1`,
so no diagnostic backdoor is present in production launches.

On failure, the base class retains the screenshot, AT-SPI tree, application
log, input trace, and snapshot (when configured) under
`tests/gui/failure_artifacts/`.
