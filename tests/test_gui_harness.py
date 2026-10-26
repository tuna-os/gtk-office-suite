"""The GUI harness may only kill processes it started (#241).

These run in the plain-Python lane with no display, so they import the
ownership module directly rather than `framework.base`, which pulls in GTK,
AT-SPI and imaging dependencies.
"""

import importlib.util
import os
import re
import shutil
import subprocess
import tempfile
import time
import unittest

# Loaded by path: `framework/__init__.py` imports the AT-SPI harness, and
# the point of keeping ownership in its own module is that this lane needs
# none of that.
_MODULE = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "gui", "framework", "owned_processes.py"
)
_spec = importlib.util.spec_from_file_location("owned_processes", _MODULE)
owned_processes = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(owned_processes)

_INSTANCES = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "gui", "framework", "instances.py"
)
_spec = importlib.util.spec_from_file_location("gui_instances", _INSTANCES)
instances = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(instances)


def _executable(directory, name):
    """A copy of /bin/sleep under `name`, so name-based killing would be
    observable: `pkill -x` matches the executable name exactly."""
    path = os.path.join(directory, name)
    shutil.copy(shutil.which("sleep"), path)
    return path


class ProcessOwnership(unittest.TestCase):
    def setUp(self):
        self._dir = tempfile.mkdtemp(prefix="gui-harness-")
        self.addCleanup(shutil.rmtree, self._dir, ignore_errors=True)
        # The registry is module state shared with any other test in this
        # process; each test works under its own app name and clears up.
        self._app = f"harness-probe-{os.getpid()}"
        # One copy, launched as many times as a test needs: copying over a
        # running executable fails with ETXTBSY.
        self._exe = _executable(self._dir, self._app)
        self.addCleanup(owned_processes.terminate_owned, self._app)

    def _launch(self):
        return self._reap(subprocess.Popen([self._exe, "30"]))

    def _reap(self, process):
        self.addCleanup(process.kill)
        return process

    def test_a_registered_process_is_terminated(self):
        process = self._launch()
        owned_processes.register(self._app, process)

        self.assertEqual(owned_processes.terminate_owned(self._app), 1)
        self.assertIsNotNone(process.poll(), "a process the harness started must be cleaned up")

    def test_an_unregistered_process_of_the_same_name_survives(self):
        """The defect this replaced: `pkill -x letters` killed every process
        named `letters` on the machine, including a developer's own open
        document. Nothing but ownership decides."""
        mine = self._launch()
        owned_processes.register(self._app, mine)
        foreign = self._launch()

        self.assertEqual(owned_processes.terminate_owned(self._app), 1)
        self.assertIsNotNone(mine.poll(), "the owned process should be gone")
        # Give the kernel the same moment the owned one got, so this cannot
        # pass merely because the check ran too soon to notice a kill.
        time.sleep(0.2)
        self.assertIsNone(
            foreign.poll(),
            "a process this harness did not start must not be killed",
        )

    def test_an_already_exited_process_is_not_counted_or_retained(self):
        process = subprocess.Popen([shutil.which("true")])
        process.wait(timeout=5)
        owned_processes.register(self._app, process)

        self.assertEqual(owned_processes.terminate_owned(self._app), 0)
        self.assertEqual(owned_processes.owned(self._app), [])

    def test_terminating_with_nothing_registered_is_a_no_op(self):
        self.assertEqual(owned_processes.terminate_owned("never-launched"), 0)


class NoGlobalKilling(unittest.TestCase):
    """A source check, because the failure is a call that is easy to
    reintroduce and impossible to notice in a passing run: the machine-wide
    kill would have succeeded silently."""

    # How such a call is actually spelled: an argv element in Python, or
    # the bare command in a shell script. Prose and backticked comments
    # explaining why it is gone do not match.
    FORBIDDEN = ('"pkill"', "'pkill'", '"killall"', "'killall'")
    FORBIDDEN_SHELL = ("pkill ", "killall ")

    def test_the_harness_does_not_shell_out_to_a_name_based_killer(self):
        harness = os.path.join(os.path.dirname(os.path.abspath(__file__)), "gui")
        offenders = []
        for root, _dirs, files in os.walk(harness):
            if "__pycache__" in root:
                continue
            for name in files:
                if not name.endswith((".py", ".sh")):
                    continue
                path = os.path.join(root, name)
                with open(path, encoding="utf-8", errors="replace") as handle:
                    for number, line in enumerate(handle, 1):
                        # Comments explaining why it is gone are welcome;
                        # a call is not.
                        code = line.split("#", 1)[0]
                        forbidden = self.FORBIDDEN + (
                            self.FORBIDDEN_SHELL if name.endswith(".sh") else ()
                        )
                        if any(command in code for command in forbidden):
                            offenders.append(f"{os.path.relpath(path)}:{number}: {line.strip()}")
        self.assertEqual(
            offenders,
            [],
            "the GUI harness must clean up only the processes it started:\n"
            + "\n".join(offenders),
        )


class XvfbNeverResets(unittest.TestCase):
    """Every Xvfb the test lanes start keeps serving between clients (#652).

    Without `-noreset` the server resets each time its last client leaves,
    and a client that connects during the reset is refused. nextest opens one
    X connection per widget test process, so the count keeps touching zero
    and a random GTK widget test failed to initialise against a live display.
    A source check, because the flag is easy to drop and a run without it
    still passes most of the time."""

    SCRIPTS = ("scripts/with-display.sh", "tests/gui/run_gui_tests.sh")

    def test_every_xvfb_invocation_passes_noreset(self):
        repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        for rel in self.SCRIPTS:
            with self.subTest(script=rel):
                with open(os.path.join(repo, rel), encoding="utf-8") as handle:
                    code = [line.split("#", 1)[0] for line in handle.read().splitlines()]
                # An invocation can continue over backslash lines; join them.
                joined = "\n".join(code).replace("\\\n", " ")
                calls = [line for line in joined.splitlines()
                         if re.match(r"\s*Xvfb\b", line)]
                self.assertTrue(calls, f"{rel} no longer starts Xvfb; update this test")
                for call in calls:
                    self.assertIn("-noreset", call, f"{rel}: {call.strip()}")


class ReadinessBudget(unittest.TestCase):
    """Setup's waits must be sized for the slowest machine, not the one
    that happened to run them.

    The first version of the display handshake polled 100 times at 0.1s and
    called it a failure after 10s. It passed every run it was written
    against and then failed a cold CI runner, which printed Xvfb's own
    startup banner three seconds *after* setup had given up — a timeout
    just long enough to pass once is indistinguishable from a correct one
    until it blocks somebody's pull request. Each wait breaks as soon as
    its condition holds, so a large budget costs a fast machine nothing;
    only a hardcoded small one costs anything at all.
    """

    MINIMUM_SECONDS = 30
    SCRIPT = os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "gui", "run_gui_tests.sh"
    )

    def _script(self):
        with open(self.SCRIPT, encoding="utf-8") as handle:
            return handle.read()

    BUDGETS = ("GUI_TEST_READY_SECONDS", "GUI_TEST_XVFB_SECONDS")

    def test_every_default_budget_is_not_marginal(self):
        script = self._script()
        for name in self.BUDGETS:
            with self.subTest(budget=name):
                found = re.findall(
                    r'%s="\$\{%s:-(\d+)\}"' % (name, name), script
                )
                self.assertEqual(len(found), 1, f"expected exactly one default for {name}")
                self.assertGreaterEqual(
                    int(found[0]),
                    self.MINIMUM_SECONDS,
                    "a wait that expires in seconds is a flake waiting to happen",
                )

    def test_the_budgets_are_independent(self):
        """Starting a server and probing one are different costs, and a test
        that shortens one must not starve the other — which is exactly how
        `test_a_display_that_never_answers_reaps_the_server_we_started`
        failed on a cold runner while passing everywhere else."""
        script = self._script()
        self.assertNotIn(
            'GUI_TEST_XVFB_SECONDS:-${GUI_TEST_READY_SECONDS',
            script,
            "the server-start budget must not default to the readiness budget",
        )

    def test_no_wait_carries_its_own_hardcoded_count(self):
        offenders = [
            f"{number}: {line.strip()}"
            for number, line in enumerate(self._script().splitlines(), 1)
            if re.search(r"seq 1 \d", line.split("#", 1)[0])
        ]
        self.assertEqual(
            offenders,
            [],
            "every setup wait must share the one configurable budget:\n"
            + "\n".join(offenders),
        )


def _is_serving(display):
    """Whether anything answers X requests on `display`.

    Asked of the display rather than of the process table on purpose: the
    harness is forbidden from identifying processes by name (#241), and so
    is its own test. A missing xdpyinfo is reported rather than read as "no
    display", because that would turn every assertion here into a
    tautology.
    """
    try:
        finished = subprocess.run(
            ["xdpyinfo", "-display", display],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
        )
    except FileNotFoundError as missing:
        raise AssertionError(
            "xdpyinfo is needed to tell a live display from a dead one "
            "(Debian/Ubuntu: x11-utils)"
        ) from missing
    return finished.returncode == 0


def _a_display_nobody_is_serving():
    """A display number nothing answers on, so connecting to it must fail."""
    for number in range(90, 120):
        if not _is_serving(f":{number}"):
            return f":{number}"
    raise AssertionError("no free display number between :90 and :119")


class InjectedSetupFailures(unittest.TestCase):
    """Setup must fail visibly and clean up after itself (#354).

    The failure this guards against is not a crash — it is setup deciding
    everything is fine and handing the journeys a display that is not
    there. That used to be a blind `sleep 1`, after which an Xvfb that
    never started left DISPLAY pointing at nothing and the journeys failed
    minutes later with timeouts that said nothing about the cause.

    Each case breaks one specific part of setup and asserts three things:
    the runner exits nonzero, its message names the actual cause, and the
    EXIT trap still ran — the roadmap asks for cleanup on setup failure
    specifically, which is the path most likely to leak because it is the
    path nobody exercises on purpose.
    """

    RUNNER = os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "gui", "run_gui_tests.sh"
    )

    def setUp(self):
        # The runner's temporary schema directory and display-number file
        # are both mktemp'd into TMPDIR, so a private TMPDIR turns "did the
        # trap run?" into a directory listing instead of a hunt for
        # process names.
        self.tmp = tempfile.mkdtemp(prefix="setup-failure-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.helpers = os.path.join(self.tmp, "bin")
        os.mkdir(self.helpers)

    def _stub(self, name, script):
        """Put an executable earlier on PATH than the real one."""
        path = os.path.join(self.helpers, name)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(script)
        os.chmod(path, 0o755)

    def _run_setup(self, **overrides):
        env = dict(os.environ)
        env["TMPDIR"] = self.tmp
        env["PATH"] = self.helpers + os.pathsep + env["PATH"]
        # One second for the readiness probes, because every one of these
        # runs is *supposed* to exhaust that budget: the point is the
        # report, not the wait. The server-start budget is left alone —
        # shortening it too made this suite fail on a cold runner at the
        # -displayfd handshake instead of at the gate under test, which is
        # the same marginal-timeout mistake these tests exist to catch.
        env["GUI_TEST_READY_SECONDS"] = "1"
        env.update(overrides)
        # A file rather than a pipe: a leaked child inherits the runner's
        # stdout, and a pipe is not closed until every holder of it exits —
        # so the one case where setup leaks its Xvfb would look like a
        # runner that never finished, and the assertion that names the leak
        # would never get to run. The log says what happened either way.
        log = os.path.join(self.tmp, "runner.log")
        with open(log, "w", encoding="utf-8") as handle:
            finished = subprocess.run(
                [self.RUNNER, "test_smoke.py"],
                env=env,
                stdout=handle,
                stderr=subprocess.STDOUT,
                timeout=180,
            )
        with open(log, encoding="utf-8", errors="replace") as handle:
            finished.output = handle.read()
        return finished

    # Ours, not setup's: the stub directory and the files we ask it to
    # write. Anything else in TMPDIR is something setup failed to remove.
    OURS = ("bin", "runner.log", "probed-display")

    def _leftovers(self):
        """What setup left in TMPDIR, ignoring what we put there."""
        return sorted(name for name in os.listdir(self.tmp) if name not in self.OURS)

    def test_an_inherited_display_that_answers_nothing_is_reported(self):
        dead = _a_display_nobody_is_serving()
        finished = self._run_setup(GUI_TEST_REUSE_DISPLAY="1", DISPLAY=dead)
        self.assertNotEqual(finished.returncode, 0, finished.output)
        self.assertIn(f"no X display at {dead}", finished.output)
        # Naming which of the two paths was taken is the whole value of the
        # message: "DISPLAY was inherited" versus "Xvfb did not come up"
        # point at different machines' worth of debugging.
        self.assertIn("GUI_TEST_REUSE_DISPLAY is set", finished.output)
        self.assertEqual(self._leftovers(), [])

    def test_a_requested_display_number_already_in_use_is_reported(self):
        taken = _a_display_nobody_is_serving()
        occupant = subprocess.Popen(
            ["Xvfb", taken, "-screen", "0", "320x240x24"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.addCleanup(occupant.wait)
        self.addCleanup(occupant.terminate)
        deadline = time.time() + 60
        while not _is_serving(taken):
            self.assertLess(time.time(), deadline, "the occupying Xvfb never started")
            time.sleep(0.1)

        finished = self._run_setup(GUI_TEST_DISPLAY_NUM=taken.lstrip(":"))

        self.assertNotEqual(finished.returncode, 0, finished.output)
        # Before -displayfd this case did not fail at all: the second run
        # joined the first run's display and the two runs' windows shared a
        # screen, with one run's keystrokes landing in the other's editor.
        self.assertIn("our Xvfb exited before it was ready", finished.output)
        self.assertIn("is it already in use?", finished.output)
        self.assertEqual(self._leftovers(), [])

    def test_a_display_that_never_answers_reaps_the_server_we_started(self):
        # Break the readiness probe rather than the server: setup gets as
        # far as starting its own Xvfb and allocating a number, then fails.
        # That is the one failure path with something of ours left running
        # to leak.
        record = os.path.join(self.tmp, "probed-display")
        self._stub(
            "xdpyinfo",
            '#!/bin/sh\nprintf "%s\\n" "$DISPLAY" >> "{record}"\nexit 1\n'.format(record=record),
        )

        finished = self._run_setup()

        self.assertNotEqual(finished.returncode, 0, finished.output)
        self.assertIn("no X display at", finished.output)
        self.assertIn("Xvfb did not come up", finished.output)
        self.assertTrue(os.path.exists(record), "setup never probed the display")
        with open(record, encoding="utf-8") as handle:
            allocated = handle.readline().strip()
        self.assertTrue(allocated.startswith(":"), f"unexpected display {allocated!r}")

        # The server setup started must not outlive it.
        deadline = time.time() + 30
        while _is_serving(allocated):
            self.assertLess(
                time.time(),
                deadline,
                f"setup left its own Xvfb serving {allocated} after failing",
            )
            time.sleep(0.1)
        self.assertEqual(self._leftovers(), [])


    def test_an_unknown_window_manager_is_reported(self):
        """GUI_TEST_WINDOW_MANAGER picks the window manager the journeys run
        under (openbox for two windows side by side). A name the runner does
        not know is a typo, and falling back to matchbox would put the
        journey on a window manager that cannot do what it asked for."""
        finished = self._run_setup(GUI_TEST_WINDOW_MANAGER="no-such-wm")
        self.assertNotEqual(finished.returncode, 0, finished.output)
        self.assertIn("GUI_TEST_WINDOW_MANAGER=no-such-wm is not one of", finished.output)
        self.assertEqual(self._leftovers(), [])


GUI_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "gui")


class InstanceEnvironment(unittest.TestCase):
    """Two instances of one app share the display and the session bus and
    nothing else (framework/instances.py)."""

    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="instances-")
        self.addCleanup(shutil.rmtree, self.root, True)

    def _env(self, label, **kwargs):
        return instances.instance_env(os.path.join(self.root, label), "/opt/collabd", **kwargs)

    def test_no_store_is_shared(self):
        a, b = self._env("a"), self._env("b")
        stores = [k for k in a if k.startswith("XDG_") or k == "COLLABD_DATA_DIR"]
        self.assertEqual(
            sorted(stores),
            ["COLLABD_DATA_DIR", "XDG_CACHE_HOME", "XDG_CONFIG_HOME",
             "XDG_DATA_HOME", "XDG_STATE_HOME"])
        for key in stores:
            self.assertNotEqual(a[key], b[key], key)
            self.assertTrue(a[key].startswith(os.path.join(self.root, "a")), a[key])
            self.assertTrue(b[key].startswith(os.path.join(self.root, "b")), b[key])
            self.assertTrue(os.path.isdir(a[key]), f"{key} was not created")

    def test_the_sidecar_identity_lives_in_the_instances_own_directory(self):
        env = self._env("a")
        self.assertEqual(env["COLLABD_DATA_DIR"], os.path.join(self.root, "a", "collabd"))
        self.assertEqual(env["COLLABD_BIN"], "/opt/collabd")

    def test_both_variables_that_allow_a_second_process_are_set(self):
        """suite-common adds NON_UNIQUE only when both are set."""
        env = self._env("a")
        self.assertEqual(env["GTK_OFFICE_TEST_MODE"], "1")
        self.assertEqual(env["GTK_OFFICE_NON_UNIQUE"], "1")

    def test_settings_do_not_go_to_the_shared_dconf_store(self):
        self.assertEqual(self._env("a")["GSETTINGS_BACKEND"], "keyfile")

    def test_nothing_is_created_when_not_asked_to(self):
        env = self._env("lazy", create=False)
        self.assertFalse(os.path.exists(env["XDG_DATA_HOME"]))

    def test_the_sidecar_binary_is_the_environments_or_the_collab_workspaces(self):
        self.assertEqual(instances.resolve_collabd({"COLLABD_BIN": "/x/collabd"}, "/repo"), "/x/collabd")
        self.assertEqual(
            instances.resolve_collabd({}, "/repo"), "/repo/collab/target/debug/collabd")


class ProcessTree(unittest.TestCase):
    """`descendants` reads /proc, so it is checked against real processes and
    against a fabricated /proc whose names hold what a real one can."""

    def test_a_grandchild_is_found_and_an_unrelated_process_is_not(self):
        sleeper = shutil.which("sleep")
        parent = subprocess.Popen(
            ["sh", "-c", f"{sleeper} 30 & {sleeper} 31 & wait"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(parent.wait)
        self.addCleanup(parent.kill)
        unrelated = subprocess.Popen([sleeper, "32"])
        self.addCleanup(unrelated.wait)
        self.addCleanup(unrelated.kill)
        deadline = time.time() + 30
        found = []
        while time.time() < deadline:
            found = instances.descendants(parent.pid)
            if len(found) >= 2:
                break
            time.sleep(0.05)
        self.assertEqual(sorted(row["cmdline"].split()[-1] for row in found), ["30", "31"], found)
        self.assertTrue(all(row["ppid"] == parent.pid for row in found))
        self.assertNotIn(unrelated.pid, [row["pid"] for row in found])

    def _fake_proc(self, processes):
        root = tempfile.mkdtemp(prefix="fake-proc-")
        self.addCleanup(shutil.rmtree, root, True)
        for pid, comm, ppid, cmdline in processes:
            os.mkdir(os.path.join(root, str(pid)))
            with open(os.path.join(root, str(pid), "stat"), "w", encoding="utf-8") as handle:
                handle.write(f"{pid} ({comm}) S {ppid} 1 1 0 -1 0\n")
            with open(os.path.join(root, str(pid), "cmdline"), "wb") as handle:
                handle.write(cmdline.replace(" ", "\0").encode())
        os.mkdir(os.path.join(root, "self"))  # not a pid
        return root

    def test_a_name_with_spaces_and_parentheses_does_not_confuse_the_parse(self):
        root = self._fake_proc([
            (10, "letters", 1, "/bin/letters"),
            (11, "my (odd) name", 10, "/x/collabd --stdio"),
            (12, "child", 11, "/x/helper"),
            (13, "other", 99, "/bin/other"),
        ])
        found = instances.descendants(10, proc_root=root)
        self.assertEqual([(r["pid"], r["comm"]) for r in found],
                         [(11, "my (odd) name"), (12, "child")])
        self.assertEqual(found[0]["cmdline"], "/x/collabd --stdio")

    def test_a_process_with_no_children_has_no_descendants(self):
        root = self._fake_proc([(10, "letters", 1, "/bin/letters")])
        self.assertEqual(instances.descendants(10, proc_root=root), [])


class WindowPlacement(unittest.TestCase):
    def test_columns_cover_the_screen_side_by_side(self):
        self.assertEqual(instances.side_by_side(1920, 1080, 2),
                         [(0, 0, 960, 1080), (960, 0, 960, 1080)])
        self.assertEqual(instances.side_by_side(1000, 600, 3),
                         [(0, 0, 333, 600), (333, 0, 333, 600), (666, 0, 333, 600)])

    def test_there_is_no_zero_columns(self):
        with self.assertRaises(ValueError):
            instances.side_by_side(1920, 1080, 0)

    def test_overlapping_windows_are_not_side_by_side(self):
        full = (0, 0, 1920, 1080)
        self.assertFalse(instances.rects_disjoint([full, full]))
        self.assertFalse(instances.rects_disjoint([(0, 0, 1000, 1080), (960, 0, 960, 1080)]))

    def test_touching_windows_are_side_by_side(self):
        self.assertTrue(instances.rects_disjoint([(0, 0, 960, 1080), (960, 0, 960, 1080)]))
        self.assertTrue(instances.rects_disjoint([(0, 0, 960, 500), (0, 500, 960, 500)]))

    def test_xdotool_shell_geometry_is_parsed(self):
        out = "WINDOW=94371847\nX=960\nY=0\nWIDTH=958\nHEIGHT=1077\nSCREEN=0\n"
        self.assertEqual(instances.parse_shell_geometry(out), (960, 0, 958, 1077))

    def test_geometry_without_a_size_is_an_error_not_a_guess(self):
        with self.assertRaises(ValueError):
            instances.parse_shell_geometry("WINDOW=1\nX=0\nY=0\n")


class TextAttributes(unittest.TestCase):
    """What a journey concludes about formatting from AT-SPI text attributes."""

    def test_a_mapping_is_kept(self):
        self.assertEqual(instances.attributes_dict({"weight": "700", "style": "italic"}),
                         {"weight": "700", "style": "italic"})

    def test_the_string_form_is_split_and_unescaped(self):
        self.assertEqual(
            instances.attributes_dict("weight:700;family-name:Foo\\; Bar;uri:http\\://x;"),
            {"weight": "700", "family-name": "Foo; Bar", "uri": "http://x"})

    def test_nothing_is_an_empty_set_of_attributes(self):
        self.assertEqual(instances.attributes_dict(""), {})
        self.assertEqual(instances.attributes_dict(None), {})

    def test_runs_cover_the_text_and_are_clipped_to_it(self):
        answers = {0: ({"weight": "400"}, 0, 7), 7: ({"weight": "700"}, 7, 99)}
        runs = instances.text_runs(lambda offset: answers[offset], 12)
        self.assertEqual(runs, [(0, 7, {"weight": "400"}), (7, 12, {"weight": "700"})])

    def test_a_run_that_does_not_advance_cannot_loop(self):
        asked = []

        def stuck(offset):
            asked.append(offset)
            return {}, offset, offset

        runs = instances.text_runs(stuck, 3)
        self.assertEqual(asked, [0, 1, 2])
        self.assertEqual([(s, e) for s, e, _ in runs], [(0, 1), (1, 2), (2, 3)])

    def test_no_text_has_no_runs(self):
        self.assertEqual(instances.text_runs(lambda offset: ({}, 0, 0), 0), [])


class SecondInstanceContracts(unittest.TestCase):
    """Source checks on what a journey cannot see go wrong: the base class
    imports dogtail, so it is read, not imported."""

    def _tree(self, *parts):
        import ast
        with open(os.path.join(GUI_DIR, *parts), encoding="utf-8") as handle:
            return ast.parse(handle.read())

    @staticmethod
    def _function(tree, name, cls=None):
        import ast
        scope = tree
        if cls:
            scope = [n for n in ast.walk(tree)
                     if isinstance(n, ast.ClassDef) and n.name == cls][0]
        return [n for n in ast.walk(scope)
                if isinstance(n, ast.FunctionDef) and n.name == name][0]

    @staticmethod
    def _calls(node, name):
        import ast
        return [c for c in ast.walk(node) if isinstance(c, ast.Call)
                and ((isinstance(c.func, ast.Name) and c.func.id == name)
                     or (isinstance(c.func, ast.Attribute) and c.func.attr == name))]

    def test_the_sweep_that_kills_the_first_instance_is_skipped_for_the_same_name(self):
        import ast
        function = self._function(self._tree("framework", "base.py"), "launch_second_app")
        arguments = [a.arg for a in function.args.args] + [a.arg for a in function.args.kwonlyargs]
        self.assertEqual(arguments, ["self", "app_name", "launch_args", "env", "same_name"])
        sweeps = self._calls(function, "_terminate_owned")
        self.assertEqual(len(sweeps), 1)
        guards = [ast.unparse(n.test) for n in ast.walk(function)
                  if isinstance(n, ast.If) and sweeps[0] in list(ast.walk(n))]
        self.assertEqual(guards, ["not same_name"], "the only sweep must sit under `if not same_name`")

    def test_a_second_instance_is_registered_for_the_sweep_and_for_cleanup(self):
        function = self._function(self._tree("framework", "base.py"), "launch_second_app")
        self.assertTrue(self._calls(function, "_register_launched"))
        self.assertTrue(self._calls(function, "addCleanup"))

    def test_the_collab_journey_fails_without_a_server_and_skips_only_when_told_to(self):
        import ast
        setup = self._function(self._tree("test_collab_letters.py"), "setUp",
                               cls="LettersCollabStreams")
        skips = self._calls(setup, "skipTest")
        fails = self._calls(setup, "fail")
        self.assertEqual(len(skips), 1)
        self.assertEqual(len(fails), 1)
        self.assertIn("DCP_SKIP_STREAMS=1", ast.unparse(skips[0]), "the skip must say why")
        # The skip sits inside the DCP_SKIP_STREAMS test and the failure does
        # not, so an unset server variable cannot fall through to a pass.
        guard = [n for n in ast.walk(setup)
                 if isinstance(n, ast.If) and "DCP_SKIP_STREAMS" in ast.unparse(n.test)]
        self.assertEqual(len(guard), 1)
        self.assertIn(skips[0], list(ast.walk(guard[0])))
        self.assertNotIn(fails[0], list(ast.walk(guard[0])))

    def test_both_collab_journeys_exist_under_the_agreed_names(self):
        import ast
        tree = self._tree("test_collab_letters.py")
        methods = {(c.name, f.name) for c in ast.walk(tree) if isinstance(c, ast.ClassDef)
                   for f in c.body if isinstance(f, ast.FunctionDef)}
        self.assertIn(("LettersCollabStreams", "test_two_instances_converge"), methods)
        self.assertIn(("LettersCollabDefaultOff", "test_default_off"), methods)


if __name__ == "__main__":
    unittest.main()
