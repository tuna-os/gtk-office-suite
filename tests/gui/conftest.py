# conftest.py — shared pytest configuration for the GUI journeys.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# A conftest rather than a change to framework/base.py: this applies to every
# GUI test module whether or not it goes through BaseGUITestCase, and it keeps
# the setting out of a file several open branches are editing.

import os

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
