#!/usr/bin/env bash
# Decide which apps' journeys a change needs, from the list of changed
# paths on stdin. Prints the pytest -k expression on stdout (empty means
# "every app") and the reason on stderr.
#
# This lived inline in gui-tests.yml, where the conservative fallback —
# the branch that must run everything when a change is shared, touches the
# harness, or matches nothing we recognise — could not be tested at all.
# That is the branch whose failure is silent: picking too few apps still
# produces a green run, just one that never exercised the app that broke.
# See tests/test_journey_selection.py.
set -euo pipefail

changed="$(cat)"

# A change here is a change to every app's journeys by definition: these
# crates are linked into all three apps, the harness runs all of them, and
# a lockfile change can move any dependency underneath any of them. This
# script counts as harness too — a selector that selects itself wrongly is
# exactly the failure the fallback exists for.
if printf '%s\n' "$changed" | grep -qE '^(suite-common|suite-common-core|suite-export)/|^tests/gui/(framework/|conftest|run_gui_tests\.sh|select_journeys\.sh)|^Cargo\.(toml|lock)$'; then
    echo "Shared crate or test-harness change: testing all apps." >&2
    exit 0
fi

apps=""
printf '%s\n' "$changed" | grep -qE '^letters(-core)?/' && apps="$apps Letters"
printf '%s\n' "$changed" | grep -qE '^tables(-core)?/' && apps="$apps Tables"
printf '%s\n' "$changed" | grep -qE '^decks(-core)?/' && apps="$apps Decks"

# Nothing recognised is not "nothing to test": it is a path this script has
# never been taught about, which is precisely when guessing is wrong.
if [ -z "$apps" ]; then
    echo "No recognized app path changed: testing all apps (fail-safe)." >&2
    exit 0
fi

filter="$(echo "$apps" | xargs | sed 's/ / or /g')"
echo "Testing only: $filter" >&2
# The bare expression, without the -k or its quoting: the step that runs
# pytest turns it into a real argument, so no quoting has to survive a
# round trip through a step output and a second shell parse.
printf '%s\n' "$filter"
