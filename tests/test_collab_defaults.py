"""Collaboration is off by default, and the product lockfile has no network stack.

RFC-0005 (option B) puts everything that talks to a server in a sidecar process,
`collabd`, whose cargo workspace (`collab/`) and `Cargo.lock` are not the
product's. What the apps may carry is `suite-collab`, the protocol crate, which
depends on serde and serde_json only. Two promises follow, and a promise nothing
checks is one the next change breaks without noticing (RFC-0001's last
conformance row, "an ordinary build is unaffected until collaboration ships"):

- D1  The root `Cargo.lock` has no `matrix-sdk`, `ruma`, `rustls`, `hyper`, and
      none of the other crates a network client brings (`tokio`, `reqwest`,
      `ring`, `h2`, `openssl`, and the Secret-portal crates `oo7` and `zbus`).
      The one new package of this work is `suite-collab`.
- D2  `suite-collab` depends on serde and serde_json and nothing else.
- D3  Nothing in the manifests of `tables`, `decks` or `letters` turns `collab`
      on by default: no default feature reaches a `collab` feature, and no
      dependency of theirs is declared with it. (When the shells grow a `collab`
      feature it must be off by default, which this keeps true.)
- D4  The graph those apps build with default features, as `cargo tree`
      resolves it, holds neither `loro` nor `suite-collab`. The lockfile cannot
      say this (it records an optional edge unconditionally); the resolver can.
      Skipped, and says so, when cargo is unavailable.
- D5  The sidecar is not a member of the product workspace and has its own
      lockfile, so D1 is a property of the layout and not luck.
"""

import os
import subprocess
import tomllib
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Crates a network client or the sidecar brings. None may be in the root lock.
NETWORK_CRATES = (
    "matrix-sdk",
    "rustls",
    "hyper",
    "tokio",
    "reqwest",
    "ring",
    "h2",
    "openssl",
    "oo7",
    "zbus",
)
# Matched by prefix: ruma, ruma-common, ruma-events, ... and matrix-sdk-*.
NETWORK_PREFIXES = ("ruma", "matrix-sdk")

APPS = ("tables", "decks", "letters")


def load(path: str) -> dict:
    with open(os.path.join(REPO_ROOT, path), "rb") as handle:
        return tomllib.load(handle)


def lock_packages() -> list[dict]:
    packages = load("Cargo.lock").get("package", [])
    assert packages, "Cargo.lock has no packages, so every check below would pass vacuously"
    return packages


def feature_closure(features: dict, start: list[str]) -> set[str]:
    """Every feature name and `dep/feature` string reachable from `start`."""
    seen: set[str] = set()
    todo = list(start)
    while todo:
        item = todo.pop()
        if item in seen:
            continue
        seen.add(item)
        if item in features:
            todo.extend(features[item])
    return seen


class TheProductLockfileHasNoNetworkStack(unittest.TestCase):
    def test_d1_no_network_crate_is_in_the_root_lockfile(self):
        names = {p["name"] for p in lock_packages()}
        present = sorted(
            n
            for n in names
            if n in NETWORK_CRATES or n.startswith(NETWORK_PREFIXES)
        )
        self.assertEqual(
            present,
            [],
            "the product Cargo.lock holds a network crate: collaboration belongs in the "
            "sidecar workspace (collab/), not in the apps' dependency graph (RFC-0005 option B)",
        )

    def test_d1_the_check_can_fail(self):
        """The root lock does hold crates; the names above are matched against them."""
        names = {p["name"] for p in lock_packages()}
        self.assertIn("serde_json", names)
        self.assertIn("loro", names, "loro is the one collaboration crate the apps carry (feature `collab`)")

    def test_d2_suite_collab_depends_on_serde_and_serde_json_only(self):
        manifest = load("suite-collab/Cargo.toml")
        self.assertEqual(sorted(manifest["dependencies"]), ["serde", "serde_json"])
        self.assertNotIn("dev-dependencies", manifest)
        self.assertNotIn("build-dependencies", manifest)
        package = next(p for p in lock_packages() if p["name"] == "suite-collab")
        self.assertEqual(
            sorted(d.split(" ")[0] for d in package.get("dependencies", [])),
            ["serde", "serde_json"],
        )

    def test_d6_the_headless_lock_stays_on_the_versions_the_product_builds_with(self):
        """`collab/headless` builds tables-core, which a fresh resolution would not build.

        The first resolution of that workspace picked a newer `rdocx` than the product
        lock holds, and `letters-core` did not compile against it. The lock is therefore
        seeded from the product's: for every package both locks name, the headless lock
        keeps at least one version the product lock has (the Windows-only `windows*`
        target shims are the exception: nothing here builds on Windows).
        """
        product, headless = {}, {}
        for table, into in ((lock_packages(), product), (load("collab/headless/Cargo.lock")["package"], headless)):
            for p in table:
                into.setdefault(p["name"], set()).add(p["version"])
        shared = [n for n in headless if n in product and not n.startswith("windows")]
        self.assertGreater(len(shared), 100, "the locks barely overlap, so this checks nothing")
        drifted = sorted(
            f"{n}: product {sorted(product[n])}, headless {sorted(headless[n])}"
            for n in shared
            if not product[n] & headless[n]
        )
        self.assertEqual(
            drifted,
            [],
            "collab/headless/Cargo.lock resolved a crate of tables-core's graph to a version the "
            "product does not build with. Reseed: cp Cargo.lock collab/headless/ && "
            "(cd collab/headless && cargo metadata --format-version 1 > /dev/null)",
        )


class CollaborationIsOffByDefault(unittest.TestCase):
    def test_d3_no_default_feature_of_an_app_reaches_collab(self):
        for app in APPS:
            features = load(f"{app}/Cargo.toml").get("features", {})
            reached = feature_closure(features, list(features.get("default", [])))
            offenders = sorted(r for r in reached if "collab" in r)
            self.assertEqual(
                offenders,
                [],
                f"{app}: a default feature turns on collaboration: {offenders}",
            )

    def test_d3_no_dependency_of_an_app_is_declared_with_collab(self):
        for app in APPS:
            manifest = load(f"{app}/Cargo.toml")
            for table in ("dependencies", "dev-dependencies", "build-dependencies"):
                for name, spec in manifest.get(table, {}).items():
                    if isinstance(spec, dict):
                        with_collab = [f for f in spec.get("features", []) if "collab" in f]
                        self.assertEqual(
                            with_collab,
                            [],
                            f"{app}: {name} is declared with {with_collab}, which enables it in every build",
                        )
                        if name == "suite-collab":
                            self.assertTrue(
                                spec.get("optional"),
                                f"{app}: suite-collab must be an optional dependency, behind a feature",
                            )
                    elif name == "suite-collab":
                        self.fail(f"{app}: suite-collab is a plain dependency; it must be optional")

    def test_d3_the_cores_keep_collab_off_by_default(self):
        for core in ("tables-core", "decks-core", "letters-core"):
            features = load(f"{core}/Cargo.toml").get("features", {})
            reached = feature_closure(features, list(features.get("default", [])))
            self.assertFalse(
                any("collab" in r for r in reached),
                f"{core}: collab is reachable from its default features",
            )

    def test_d4_the_default_graph_of_the_apps_has_neither_loro_nor_suite_collab(self):
        command = ["cargo", "tree", "--locked", "-e", "normal", "--prefix", "none"]
        for app in APPS:
            command += ["-p", app]
        try:
            done = subprocess.run(
                command, cwd=REPO_ROOT, capture_output=True, text=True, timeout=600
            )
        except (OSError, subprocess.SubprocessError) as error:
            self.skipTest(f"cargo is unavailable, so feature resolution cannot be checked: {error}")
        if done.returncode != 0:
            self.fail(f"cargo tree failed: {done.stderr.strip()}")
        nodes = {line.split(" ")[0] for line in done.stdout.splitlines() if line.strip()}
        self.assertTrue({"tables-core", "serde"} <= nodes, "the tree is empty or wrong; nothing was checked")
        for crate in ("loro", "suite-collab"):
            self.assertNotIn(
                crate,
                nodes,
                f"{crate} is in the graph the apps build by default: collaboration is no longer opt-in",
            )

    def test_d5_the_sidecar_is_not_a_member_of_the_product_workspace(self):
        workspace = load("Cargo.toml")["workspace"]
        self.assertIn("suite-collab", workspace["members"])
        self.assertIn("collab", workspace.get("exclude", []))
        for member in workspace["members"]:
            self.assertFalse(
                member == "collab" or member.startswith("collab/"),
                f"{member} is a member of the product workspace",
            )
        sidecar = load("collab/Cargo.toml")
        self.assertIn("workspace", sidecar, "collab/ must be a workspace root of its own")
        self.assertTrue(os.path.exists(os.path.join(REPO_ROOT, "collab", "Cargo.lock")))
        # The sidecar lock is the one that holds the network stack.
        names = {p["name"] for p in load("collab/Cargo.lock")["package"]}
        self.assertTrue({"matrix-sdk", "rustls", "tokio"} <= names)


if __name__ == "__main__":
    unittest.main()
