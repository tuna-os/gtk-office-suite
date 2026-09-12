#!/usr/bin/env python3
"""Collect every skipped test and the reason it was skipped, and refuse the unexplained ones.

The readiness item this closes asks CI to "collect skip reasons". The
obvious way to do that does not work, and the reason is worth stating
before the code: **a JUnit report does not contain the skips that matter
here.** nextest omits `#[ignore]` tests from its JUnit output entirely —
not as `<skipped>`, but as no `<testcase>` at all. Measured on this
workspace:

    <testsuites tests="848" skipped="0" ...>   # with 7 ignored tests

So a collector built on JUnit would report a clean sheet while seven tests
sat out the run, which is precisely the failure mode the capability ledger
exists to prevent: a check that reports success without having checked.

The inventory therefore comes from the test *lister*, which does report
them:

    cargo nextest list --workspace --run-ignored all --message-format json

That says which tests are ignored. It does not say why: libtest drops the
string from `#[ignore = "reason"]` on the floor, and neither `--list` nor
nextest's JSON carries it. Verified on this workspace's own binaries --
`--list --ignored` prints `seed_campaign: test` and nothing else.

So the reason has to come from the source, and the listing is what keeps
the source scan honest. Each side catches the other's blind spot:

  * the listing says a test is ignored, so a bare `#[ignore]` cannot hide
    behind a reason written somewhere else, and deleting the attribute is
    the only way to leave the report;
  * the source says what the reason is, so a test that is ignored and
    unexplained is an error rather than a silent omission;
  * an `#[ignore]` in the source that the listing never reports means the
    reason is stale -- the test was renamed, deleted, or compiled out --
    and a stale reason is worse than none, because it reads as current.

Runtime skips (pytest's `skipTest`, nextest's `<skipped>`) *are* in JUnit
with their message, so `--junit` folds those into the same report rather
than leaving two half-reports to reconcile.

Usage:
  cargo nextest list --workspace --run-ignored all --message-format json > list.json
  python3 conformance/collect_skip_reasons.py --nextest-list list.json \\
      --junit junit.xml --out skips.json
"""

import argparse
import json
import os
import re
import sys
import xml.etree.ElementTree as ET

# `#[ignore]` or `#[ignore = "reason"]`, with the attribute's own spacing
# free. The reason is optional *here* on purpose: the whole point is to
# find the ones that omit it, so the parse must accept what it rejects.
IGNORE_ATTR = re.compile(
    r"""^\s*\#\[\s*ignore
        (?:\s*=\s*"(?P<reason>(?:[^"\\]|\\.)*)")?
        \s*\]\s*$""",
    re.VERBOSE,
)
# The test function an attribute block belongs to. `async fn` and
# visibility modifiers appear in test functions elsewhere in the tree, so
# they are accepted rather than silently skipped over.
FN_DECL = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(?P<name>\w+)")


class ScanError(Exception):
    """A source file could not be read, which is never a reason to pass."""


def package_dir(package_id: str) -> str:
    """The directory of a nextest `package-id` like `path+file:///x/y#0.1.0`."""
    path = package_id.split("#", 1)[0]
    if path.startswith("path+file://"):
        return path[len("path+file://"):]
    return ""


def ignored_tests(listing: dict) -> list:
    """Every ignored test in a nextest JSON listing, with its binary's metadata.

    The listing is the authority on *which* tests are ignored. Its suite
    metadata also says where to look for the reason: an integration target
    named `stateful` in package `letters-core` is `letters-core/tests/stateful.rs`,
    and a lib target's tests are somewhere under that package's `src/`.
    """
    found = []
    for binary_id, suite in sorted(listing.get("rust-suites", {}).items()):
        cases = suite.get("testcases") or suite.get("test-cases") or {}
        for name, case in sorted(cases.items()):
            if not case.get("ignored"):
                continue
            found.append({
                "id": f"{binary_id}::{name}",
                "binary_id": binary_id,
                "package": suite.get("package-name", ""),
                "package_dir": package_dir(suite.get("package-id", "")),
                "kind": suite.get("kind", ""),
                "binary_name": suite.get("binary-name", ""),
                "test": name,
            })
    return found


def scan_file(path: str) -> list:
    """Every `#[ignore]` in one file, as (line, reason-or-None, fn name).

    Attributes stack in any order, so the scan walks forward from an
    `#[ignore]` through the rest of the attribute block to the `fn` it
    decorates. A reason is whatever the attribute said, including the
    empty string -- an explicit `#[ignore = ""]` is as unexplained as a
    bare one, and saying so is the caller's job, not the parser's.
    """
    try:
        with open(path, encoding="utf-8") as handle:
            lines = handle.read().splitlines()
    except OSError as exc:
        raise ScanError(f"cannot read {path}: {exc}") from exc

    out = []
    for index, line in enumerate(lines):
        match = IGNORE_ATTR.match(line)
        if not match:
            continue
        name = None
        for following in lines[index + 1:]:
            stripped = following.strip()
            if not stripped or stripped.startswith("//") or stripped.startswith("#["):
                continue
            decl = FN_DECL.match(following)
            name = decl.group("name") if decl else None
            break
        out.append({
            "file": path,
            "line": index + 1,
            "reason": match.group("reason"),
            "fn": name,
        })
    return out


def rust_sources(roots: list) -> list:
    """Every `.rs` file under the given roots, excluding build output."""
    found = []
    for root in roots:
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in sorted(dirnames)
                           if d not in (".git", "target", "node_modules", ".venv")]
            for filename in sorted(filenames):
                if filename.endswith(".rs"):
                    found.append(os.path.join(dirpath, filename))
    return found


def candidate_files(test: dict, sources: list) -> list:
    """The source files that could hold one ignored test's `#[ignore]`.

    Narrowed by the binary the listing names, so a same-named test in
    another crate or another integration target cannot lend it a reason.
    An unrecognised target kind falls back to the whole package, which
    over-matches rather than under-matches: a wrong reason is a worse
    failure than an ambiguous one, and ambiguity is reported.
    """
    pkg = test["package_dir"]
    if not pkg:
        return sources
    if test["kind"] == "test":
        target = os.path.join(pkg, "tests", f"{test['binary_name']}.rs")
        # An integration target may split across `tests/<name>/` modules.
        inside = os.path.join(pkg, "tests", test["binary_name"]) + os.sep
        return [s for s in sources
                if os.path.abspath(s) == os.path.abspath(target)
                or os.path.abspath(s).startswith(os.path.abspath(inside))]
    if test["kind"] == "lib":
        inside = os.path.join(pkg, "src") + os.sep
        return [s for s in sources if os.path.abspath(s).startswith(os.path.abspath(inside))]
    return [s for s in sources if os.path.abspath(s).startswith(os.path.abspath(pkg) + os.sep)]


def junit_skips(text: str) -> list:
    """Runtime skips from a JUnit report, with the message the runner gave.

    This is the half of the picture JUnit *does* carry: a test that was
    collected, started, and then skipped itself. Ignored tests never
    appear here, which is why they come from the listing instead.
    """
    out = []
    root = ET.fromstring(text)
    suites = root.iter("testsuite") if root.tag != "testsuite" else [root]
    for suite in suites:
        for case in suite.iter("testcase"):
            name = case.get("name")
            if not name:
                continue
            for child in case:
                if child.tag.lower() != "skipped":
                    continue
                binary = case.get("classname") or suite.get("name") or ""
                message = (child.get("message") or (child.text or "")).strip()
                out.append({
                    "id": f"{binary}::{name}" if binary else name,
                    "kind": "runtime-skip",
                    "reason": message,
                })
    return out


def collect(listing: dict, roots: list, junits: list) -> tuple:
    """Build the skip report and the list of problems with it.

    Returns (report, errors). An empty error list means every test that
    sat out the run said why.
    """
    errors = []
    sources = rust_sources(roots)
    attrs = []
    for path in sources:
        attrs += scan_file(path)

    tests = ignored_tests(listing)

    # A listing that covers nothing would make every check below vacuous:
    # no ignored tests found, no unexplained ones, a clean report. The
    # source scan is what catches it -- `#[ignore]` in the tree with an
    # empty listing means the listing was not of this workspace.
    if not tests and attrs:
        errors.append(
            f"the listing reports no ignored tests, but {len(attrs)} #[ignore] "
            f"attribute(s) exist in the sources (first: {attrs[0]['file']}:{attrs[0]['line']}). "
            "The listing does not cover this workspace, so it cannot vouch for anything; "
            "re-run `cargo nextest list --workspace --run-ignored all`."
        )

    claimed = set()
    rows = []
    for test in tests:
        candidates = candidate_files(test, sources)
        paths = {os.path.abspath(path) for path in candidates}
        matches = [a for a in attrs
                   if os.path.abspath(a["file"]) in paths and a["fn"] == test["test"].split("::")[-1]]
        if not matches:
            errors.append(
                f"{test['id']} is ignored, but no #[ignore] attribute for it was found in "
                f"{test['package']}'s sources. An ignored test with no locatable attribute "
                "cannot carry a reason, so the run is unexplained."
            )
            continue
        reasons = {a["reason"] for a in matches}
        if len(reasons) > 1:
            where = ", ".join(f"{a['file']}:{a['line']}" for a in matches)
            errors.append(
                f"{test['id']} matches {len(matches)} #[ignore] attributes with different "
                f"reasons ({where}). Which one applies is a guess, so neither is accepted."
            )
            continue
        for attr in matches:
            claimed.add((attr["file"], attr["line"]))
        reason = matches[0]["reason"]
        if not reason:
            attr = matches[0]
            errors.append(
                f"{test['id']} is ignored with no reason ({attr['file']}:{attr['line']}). "
                'Write `#[ignore = "why, and what runs it instead"]`: a test that sits out '
                "every run without saying why is indistinguishable from one that was "
                "forgotten."
            )
            continue
        rows.append({
            "id": test["id"],
            "kind": "ignored",
            "reason": reason,
            "file": os.path.relpath(matches[0]["file"]),
            "line": matches[0]["line"],
        })

    for attr in attrs:
        if (attr["file"], attr["line"]) in claimed:
            continue
        errors.append(
            f"{os.path.relpath(attr['file'])}:{attr['line']} has an #[ignore] that no listed "
            f"test matches (fn {attr['fn'] or '?'}). A reason left behind by a renamed, "
            "deleted or cfg'd-out test reads as current and is not."
        )

    for path in junits:
        with open(path, encoding="utf-8") as handle:
            for row in junit_skips(handle.read()):
                if not row["reason"]:
                    errors.append(
                        f"{row['id']} skipped itself at runtime with no message. "
                        "A skip with no message is a hole in the report."
                    )
                    continue
                rows.append(row)

    rows.sort(key=lambda row: (row["kind"], row["id"]))
    report = {"skips": rows, "count": len(rows)}
    return report, errors


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--nextest-list", required=True, metavar="FILE",
                        help="`cargo nextest list --run-ignored all --message-format json` output")
    parser.add_argument("--junit", action="append", default=[], metavar="FILE",
                        help="JUnit report contributing runtime skips (repeatable)")
    parser.add_argument("--root", action="append", default=[], metavar="DIR",
                        help="directory to scan for #[ignore] (default: the repository root)")
    parser.add_argument("--out", help="write the skip report here as JSON")
    args = parser.parse_args(argv)

    roots = args.root or [os.path.dirname(os.path.dirname(os.path.abspath(__file__)))]
    with open(args.nextest_list, encoding="utf-8") as handle:
        listing = json.load(handle)

    try:
        report, errors = collect(listing, roots, args.junit)
    except ScanError as exc:
        print(f"SKIP REPORT FAILED: {exc}", file=sys.stderr)
        return 2

    for row in report["skips"]:
        where = f"  {row['file']}:{row['line']}" if "file" in row else ""
        print(f"{row['kind']:13} {row['id']}\n              {row['reason']}{where}")
    print(f"\n{report['count']} skipped test(s), all explained"
          if not errors else f"\n{report['count']} skipped test(s) accounted for")

    if args.out:
        with open(args.out, "w", encoding="utf-8") as handle:
            json.dump(report, handle, indent=2, sort_keys=True)
            handle.write("\n")
        print(f"report written to {args.out}")

    if errors:
        print(f"\n{len(errors)} unexplained skip(s):", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
