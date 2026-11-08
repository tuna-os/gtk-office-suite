"""docs/TABLES-DYNAMIC-ARRAYS.md names exactly the functions the tests
exercise (#1659).

The tests in tables-core/tests/dynamic_arrays.rs pin each admitted
function against the engine; the page is the contract users and reviewers
read. A function on the page the tests do not exercise is a claim nothing
checks, and one the tests exercise but the page omits is support nobody
was told about.
"""

import os
import re
import unittest

ROOT = os.path.join(os.path.dirname(__file__), "..")
SUPPORT = os.path.join(ROOT, "tables-core", "tests", "support", "mod.rs")
PAGE = os.path.join(ROOT, "docs", "TABLES-DYNAMIC-ARRAYS.md")


def read(path):
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def admitted():
    body = re.search(r"pub const ADMITTED: &\[&str\] = &\[(.*?)\];", read(SUPPORT), re.S).group(1)
    return set(re.findall(r'"([A-Z]+)"', body))


def documented():
    section = read(PAGE).split("## Admitted functions", 1)[1].split("\n## ", 1)[0]
    return set(re.findall(r"^\| `([A-Z]+)` \|", section, re.M))


class DynamicArrayInventory(unittest.TestCase):
    def test_the_page_names_exactly_the_admitted_functions(self):
        self.assertEqual(documented(), admitted())

    def test_every_case_function_is_admitted(self):
        cases = set(re.findall(r'Case \{ function: "([A-Z]+)"', read(SUPPORT)))
        self.assertTrue(cases)
        self.assertLessEqual(cases, admitted())

    def test_the_page_names_the_tests_that_pin_it(self):
        page = read(PAGE)
        for name in ("tables-core/tests/dynamic_arrays.rs", "a_calc_24_2_rewrite_keeps_the_formulas_and_loses_the_cached_values"):
            self.assertIn(name, page)
        self.assertIn("a_calc_24_2_rewrite_keeps_the_formulas_and_loses_the_cached_values", read(os.path.join(ROOT, "tables-core", "tests", "soffice_oracle.rs")))


if __name__ == "__main__":
    unittest.main()
