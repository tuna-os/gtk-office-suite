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

    def test_the_default_budget_is_not_marginal(self):
        found = re.findall(
            r'GUI_TEST_READY_SECONDS="\$\{GUI_TEST_READY_SECONDS:-(\d+)\}"', self._script()
        )
        self.assertEqual(len(found), 1, "expected exactly one default readiness budget")
        self.assertGreaterEqual(
            int(found[0]),
            self.MINIMUM_SECONDS,
            "a readiness wait that expires in seconds is a flake waiting to happen",
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
        # One second, because every one of these runs is *supposed* to
        # exhaust its budget: the point is the report, not the wait.
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


if __name__ == "__main__":
    unittest.main()
