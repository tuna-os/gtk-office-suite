# conftest.py — shared pytest configuration for the GUI journeys.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# A conftest rather than a change to framework/base.py: this applies to every
# GUI test module whether or not it goes through BaseGUITestCase, and it keeps
# the setting out of a file several open branches are editing.

import os
import time

from dogtail import config as dogtail_config

# dogtail types one character at a time, and its own `typeText` docstring says:
#
#     Please note, you may have to set a higher typing delay, if your machine
#     misses/switches the characters typed. Needed sometimes on slow
#     setups/VMs typing non-ASCII utf8 chars.
#
# The default is 0.1s. A CI runner and a container are both "slow setups/VMs",
# and at 0.1s characters are dropped: a three-character cell reference like
# "A10" arrives as "A", so the journey then times out waiting for a selection
# that was never made. The failure surfaces as "Timed out waiting for A10
# selected for edit", which reads like a navigation bug rather than lost input,
# which is why it has been hard to place.
#
# Measured in a container matching the CI image: at 0.1s the three csv/tsv
# corpus journeys fail at that step; at 0.4s the same three pass end to end
# with no other change.
#
# Override with GUI_TEST_TYPING_DELAY when tuning; lower is faster but starts
# dropping keystrokes again.
dogtail_config.config.typingDelay = float(os.environ.get("GUI_TEST_TYPING_DELAY", "0.4"))

# ── Diagnostic: find the fixed waits that are load-bearing ───────────
#
# The journeys hold 231 `time.sleep` calls, 163 seconds of unconditional
# waiting per full run. Some are real synchronisation — the app needs a
# moment and nothing observable says when it is done — and some are habit.
# The difference matters, because a fixed wait that is only just long
# enough is a flake waiting for a loaded runner, and #354 asks for bounded
# predicates instead.
#
# Guessing which is which is how a crash gets "fixed" with a longer sleep.
# So this scales every sleep, and a run at a fraction of normal speed says
# empirically which waits the journeys actually depend on: whatever fails
# at 0.25 is a wait that should become a predicate on the state it is
# really waiting for.
#
#   GUI_TEST_SLEEP_SCALE=0.25 tests/gui/run_gui_tests.sh test_smoke.py
#
# Diagnostic only. The default is 1.0, which patches nothing, and a
# campaign or gate must never set it: a suite that passes at 0.25 has not
# proven the app is fast, only that these particular waits were not the
# thing holding it together.
_SLEEP_SCALE = float(os.environ.get("GUI_TEST_SLEEP_SCALE", "1"))
if _SLEEP_SCALE != 1.0:
    _real_sleep = time.sleep

    def _scaled_sleep(seconds):
        # The framework's bounded waits poll through time.sleep too, and
        # scaling those only makes them poll tighter against the same
        # deadline — the timeout is wall-clock, so a predicate that holds
        # still passes.
        _real_sleep(seconds * _SLEEP_SCALE)

    time.sleep = _scaled_sleep
    print(f"GUI_TEST_SLEEP_SCALE={_SLEEP_SCALE}: fixed waits scaled "
          "(diagnostic; not a gate configuration)")
