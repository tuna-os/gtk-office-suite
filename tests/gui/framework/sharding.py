# sharding.py — split the GUI journeys across CI jobs.
# SPDX-License-Identifier: GPL-3.0-or-later
#
#   GUI_TEST_SHARD=3/16 tests/gui/run_gui_tests.sh test_smoke.py
#
# runs the fourth of sixteen shares of the collected journeys (conftest.py
# applies it). The unit is the test class, not the test: a class's
# journeys share fixtures and often an order, and a class may name another
# to travel with (`shard_with = "OtherClass"`), as the settings-isolation
# pair does — each half passes alone, but only the two in order catch a
# leak. Classes go to the shard with the fewest journeys so far, largest
# first, so the shards finish together; the split depends only on what was
# collected, so every job computes the same one.
#
# No GUI imports here, so tests/test_gui_sharding.py can test it without a
# display.


def shard_spec(spec):
    """`(index, count)` from "K/N", or None for an empty spec."""
    spec = (spec or "").strip()
    if not spec:
        return None
    index, _, count = spec.partition("/")
    index, count = int(index), int(count)
    if not 0 <= index < count:
        raise ValueError(f"GUI_TEST_SHARD={spec}: want K/N with 0 <= K < N")
    return index, count


def shard_unit(item):
    """What travels together: the item's class (or the one it names), else
    its module."""
    module = item.nodeid.split("::")[0]
    cls = getattr(item, "cls", None)
    if cls is None:
        return module
    return f"{module}::{getattr(cls, 'shard_with', cls.__name__)}"


def shard_items(items, index, count):
    """The items shard `index` of `count` runs, in their collected order."""
    units = {}
    for item in items:
        units.setdefault(shard_unit(item), []).append(item)
    loads = [0] * count
    owner = {}
    for unit, members in sorted(units.items(), key=lambda u: (-len(u[1]), u[0])):
        k = min(range(count), key=lambda s: (loads[s], s))
        owner[unit] = k
        loads[k] += len(members)
    return [item for item in items if owner[shard_unit(item)] == index]
