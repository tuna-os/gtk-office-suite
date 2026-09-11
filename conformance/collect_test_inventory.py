#!/usr/bin/env python3
"""Turn test-collection output into the inventory the ledger is checked against (#441).

The capability ledger names tests. For that to mean anything, the names
have to be compared against tests that *were collected*, not against
strings that happen to appear in a source file — a renamed or deleted test
leaves the old name matching nothing, and a grep would never notice.

Three sources, because the suite has two kinds of test and one of them
already reports results:

  cargo test -- --list     ->  "letters-core::structured::tests::name"
  pytest --collect-only -q ->  "tests/gui/test_smoke.py::Class::test_name"
  nextest JUnit XML        ->  the same ids, plus each one's outcome

All three are produced by the jobs that already run those tests, so the
inventory describes the run that actually happened. The JUnit source is
the strongest: it says both that a test exists and how it ended, which is
what separates "collected" from "observed passing".

Usage:
  cargo test -p letters-core --lib -- --list > cargo.txt
  python3 conformance/collect_test_inventory.py --cargo letters-core=cargo.txt \\
      --pytest tests/gui=collected.txt --out inventory.json

The cargo prefix is whatever makes the id unambiguous: "letters-core" for
that crate's lib tests, "letters-core::docx" for its tests/docx.rs
integration target, whose test names are bare.
"""

import argparse
import json
import re
import sys
import xml.etree.ElementTree as ET

# `cargo test -- --list` prints "module::path::name: test" (and ": benchmark").
CARGO_LINE = re.compile(r"^(?P<path>[A-Za-z_][A-Za-z0-9_:<>, ]*): (?:test|benchmark)$")
# `pytest --collect-only -q` prints "file.py::Class::test_name" plus a summary.
PYTEST_LINE = re.compile(r"^(?P<path>[\w./\-]+\.py)::(?P<rest>[\w:\[\]\-. ]+)$")


def parse_cargo(text: str, crate: str) -> list:
    """Test ids from one `cargo test -- --list` run, prefixed by crate."""
    ids = []
    for line in text.splitlines():
        m = CARGO_LINE.match(line.strip())
        if m:
            ids.append(f"{crate}::{m.group('path')}")
    return ids


def parse_pytest(text: str, prefix: str) -> list:
    """Test ids from one `pytest --collect-only -q` run.

    pytest prints paths relative to its rootdir (it runs inside tests/gui),
    so the prefix puts them back in repo-relative form — the shape the
    ledger uses, and the shape a reader can paste into a command.
    """
    ids = []
    prefix = prefix.rstrip("/")
    for line in text.splitlines():
        line = line.strip()
        m = PYTEST_LINE.match(line)
        if not m:
            continue
        path = m.group("path")
        if prefix and not path.startswith(prefix):
            path = f"{prefix}/{path}"
        ids.append(f"{path}::{m.group('rest')}")
    return ids


def parse_junit(text: str) -> dict:
    """Test ids and outcomes from a nextest JUnit report.

    nextest names the binary in `classname` ("letters-core" for a crate's
    lib tests, "letters-core::docx" for an integration target) and the test
    path in `name`, which is exactly the id shape the ledger uses.

    A testcase carries a child element for a non-passing outcome
    (<failure>, <error>, <skipped>); a bare testcase passed.

    Returns (outcomes, covers): the namespaces are read off the binaries
    the report actually contains, so a Rust report never claims to speak
    for the GUI journeys it has never heard of.
    """
    outcomes = {}
    covers = set()
    root = ET.fromstring(text)
    suites = root.iter("testsuite") if root.tag != "testsuite" else [root]
    for suite in suites:
        for case in suite.iter("testcase"):
            name = case.get("name")
            if not name:
                continue
            binary = case.get("classname") or suite.get("name") or ""
            test_id = f"{binary}::{name}" if binary else name
            if binary:
                # "letters-core::docx" is a target inside letters-core;
                # the namespace is the crate, so one report covers both.
                covers.add(binary.split("::")[0] + "::")
            outcome = "passed"
            for child in case:
                tag = child.tag.lower()
                if tag in ("failure", "error"):
                    outcome = "failed"
                    break
                if tag == "skipped":
                    outcome = "skipped"
                    break
            outcomes[test_id] = outcome
    return outcomes, sorted(covers)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--cargo", action="append", default=[], metavar="PREFIX=FILE",
                        help="output of `cargo test -- --list`, under an id prefix (repeatable)")
    parser.add_argument("--pytest", action="append", default=[], metavar="PREFIX=FILE",
                        help="output of `pytest --collect-only -q` (repeatable)")
    parser.add_argument("--junit", action="append", default=[], metavar="[PREFIX=]FILE",
                        help="nextest JUnit XML; contributes ids and outcomes. The covered "
                             "namespaces are read from the report unless PREFIX is given "
                             "(repeatable)")
    parser.add_argument("--results-out", help="write the id->outcome map here")
    parser.add_argument("--out", default="-", help="write the inventory here, or - for stdout")
    args = parser.parse_args(argv)

    tests = []
    covers = []
    results = {}
    for spec in args.cargo:
        crate, _, path = spec.partition("=")
        if not path:
            parser.error(f"--cargo expects PREFIX=FILE, got {spec!r}")
        covers.append(f"{crate}::")
        with open(path) as f:
            tests += parse_cargo(f.read(), crate)
    for spec in args.pytest:
        prefix, _, path = spec.partition("=")
        if not path:
            parser.error(f"--pytest expects PREFIX=FILE, got {spec!r}")
        covers.append(f"{prefix.rstrip('/')}/")
        with open(path) as f:
            tests += parse_pytest(f.read(), prefix)
    for spec in args.junit:
        prefix, _, path = spec.partition("=")
        if not path:
            parser.error(f"--junit expects PREFIX=FILE, got {spec!r}")
        with open(path) as f:
            outcomes, seen = parse_junit(f.read())
        covers += seen if not prefix else [prefix]
        tests += outcomes.keys()
        results.update(outcomes)

    # Which id namespaces this inventory can speak for. Without it a
    # validator run holding only the Rust listing would report every
    # pytest id as missing, which is a false alarm rather than a finding.
    inventory = {"covers": sorted(set(covers)), "tests": sorted(set(tests))}
    if args.results_out:
        with open(args.results_out, "w") as f:
            json.dump(results, f, indent=2, sort_keys=True)
        print(f"{len(results)} recorded results written to {args.results_out}")
    text = json.dumps(inventory, indent=2)
    if args.out == "-":
        print(text)
    else:
        with open(args.out, "w") as f:
            f.write(text + "\n")
        print(f"{len(inventory['tests'])} collected tests written to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
