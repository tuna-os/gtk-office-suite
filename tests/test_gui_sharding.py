"""tests/gui/framework/sharding.py: CI splits the GUI journeys across jobs
by GUI_TEST_SHARD. Every journey must run in exactly one shard — one left
out is coverage lost without a red run — and a class must keep the
classes it names with it, in order."""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "gui", "framework"))
import sharding  # noqa: E402


class _Item:
    def __init__(self, nodeid, cls=None):
        self.nodeid, self.cls = nodeid, cls


def _classes():
    made = {}

    def cls(name, **attrs):
        return made.setdefault(name, type(name, (), attrs))
    return cls


class Sharding(unittest.TestCase):
    def items(self):
        cls = _classes()
        out = []
        for c in range(20):
            for t in range(1 + c % 4):
                out.append(_Item(f"test_smoke.py::C{c}::test_{t}", cls(f"C{c}")))
        out.append(_Item("test_smoke.py::Leader::test", cls("Leader")))
        out.append(_Item("test_smoke.py::Follower::test", cls("Follower", shard_with="Leader")))
        out.append(_Item("test_edit_render.py::test_function"))
        return out

    def test_every_item_runs_in_exactly_one_shard(self):
        items = self.items()
        for count in (1, 3, 16, 40):
            runs = [i.nodeid for k in range(count) for i in sharding.shard_items(items, k, count)]
            self.assertEqual(sorted(runs), sorted(i.nodeid for i in items), count)

    def test_a_class_travels_with_the_one_it_names_in_order(self):
        items = self.items()
        for k in range(16):
            ids = [i.nodeid for i in sharding.shard_items(items, k, 16)]
            if any("Leader" in i for i in ids):
                self.assertEqual([i for i in ids if "Leader" in i or "Follower" in i],
                                 ["test_smoke.py::Leader::test", "test_smoke.py::Follower::test"])
                return
        self.fail("the pair ran nowhere")

    def test_shards_are_balanced_and_keep_collected_order(self):
        items = self.items()
        sizes = [len(sharding.shard_items(items, k, 8)) for k in range(8)]
        self.assertLessEqual(max(sizes) - min(sizes), 4, sizes)
        order = {i.nodeid: n for n, i in enumerate(items)}
        for k in range(8):
            got = [order[i.nodeid] for i in sharding.shard_items(items, k, 8)]
            self.assertEqual(got, sorted(got))

    def test_the_spec_is_k_of_n(self):
        self.assertIsNone(sharding.shard_spec(""))
        self.assertEqual(sharding.shard_spec("3/16"), (3, 16))
        for bad in ("16/16", "-1/4", "2"):
            with self.assertRaises(ValueError):
                sharding.shard_spec(bad)


if __name__ == "__main__":
    unittest.main()
