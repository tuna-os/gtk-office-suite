"""The GUI harness may only kill processes it started (#241).

These run in the plain-Python lane with no display, so they import the
ownership module directly rather than `framework.base`, which pulls in GTK,
AT-SPI and imaging dependencies.
"""

import importlib.util
import os
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


if __name__ == "__main__":
    unittest.main()
