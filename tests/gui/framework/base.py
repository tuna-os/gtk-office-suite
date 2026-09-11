import os
import sys
import time
import json
import base64
import re
import subprocess
import unittest
from io import BytesIO

import mss
import requests
from PIL import Image
from dogtail import tree, rawinput

from . import recorder as screen_recorder
from .owned_processes import (
    register as _register_launched,
    terminate_owned as _terminate_owned,
)

_dogtail_click = rawinput.click
_dogtail_key_combo = rawinput.keyCombo
_dogtail_type_text = rawinput.typeText

class _SecondApp:
    """A second application running beside the primary one."""

    def __init__(self, name, process, app, win_id):
        self.name = name
        self.process = process
        self.app = app
        self.win_id = win_id

    def __repr__(self):
        return f"<_SecondApp {self.name} pid={self.process.pid} win={self.win_id}>"


class BaseGUITestCase(unittest.TestCase):
    app_name = None  # to be overridden by subclasses

    @classmethod
    def setUpClass(cls):
        if not cls.app_name:
            raise unittest.SkipTest("app_name not set")

    # ── Shared test-isolation helpers (#104) ────────────────────────────
    # Call these from a subclass's setUp() *before* calling super().setUp()
    # (launch_env has to be in place before the app process launches).
    # Cleanup is registered via addCleanup, which unittest runs after
    # tearDown regardless of pass/fail — subclasses using these don't need
    # their own tearDown just to remove a temp dir.

    def temp_dir(self, prefix="gtk-office-test-"):
        """A fresh temp directory, auto-removed after the test."""
        import tempfile
        import shutil
        d = tempfile.mkdtemp(prefix=prefix)
        self.addCleanup(shutil.rmtree, d, ignore_errors=True)
        return d

    def isolate_autosave_state(self, prefix="autosave-state-"):
        """Points XDG_STATE_HOME at a fresh temp dir so crash-recovery
        snapshot tests don't collide with each other or a real session."""
        d = self.temp_dir(prefix)
        self.launch_env = {**getattr(self, "launch_env", {}), "XDG_STATE_HOME": d}
        return d

    def isolate_xdg(self, prefix="xdg-"):
        """Put all user XDG stores in one disposable per-test directory."""
        d = self.temp_dir(prefix)
        self.launch_env = {
            **getattr(self, "launch_env", {}),
            "XDG_CONFIG_HOME": os.path.join(d, "config"),
            "XDG_DATA_HOME": os.path.join(d, "data"),
            "XDG_CACHE_HOME": os.path.join(d, "cache"),
            "XDG_STATE_HOME": os.path.join(d, "state"),
        }
        for child in ("config", "data", "cache", "state"):
            os.makedirs(os.path.join(d, child), exist_ok=True)
        return d

    def isolate_snapshot(self, prefix="snapshot-"):
        """Sets up the #104 test-only state-snapshot interface: registers
        the app's GTK_OFFICE_TEST_MODE-gated snapshot action and points it
        at a fresh temp file, auto-removed after the test. Returns the
        path — trigger the action with `gapplication action <id>
        test-snapshot`, then read/json.load() the path afterward."""
        d = self.temp_dir(prefix)
        path = os.path.join(d, "snapshot.json")
        self.launch_env = {
            **getattr(self, "launch_env", {}),
            "GTK_OFFICE_TEST_MODE": "1",
            "GTK_OFFICE_SNAPSHOT_PATH": path,
        }
        return path

    def isolate_gsettings(self, prefix="gsettings-cfg-"):
        """The default GSettings backend is dconf, a shared per-user D-Bus
        daemon whose storage isn't controlled by env vars — the keyfile
        backend plus a fresh XDG_CONFIG_HOME is what actually isolates a
        test's settings from a real session or other tests."""
        d = self.temp_dir(prefix)
        self.launch_env = {
            **getattr(self, "launch_env", {}),
            "GSETTINGS_BACKEND": "keyfile",
            "XDG_CONFIG_HOME": d,
        }
        return d

    def setUp(self):
        # Resolve directories
        self.framework_dir = os.path.dirname(os.path.abspath(__file__))
        self.gui_dir = os.path.dirname(self.framework_dir)
        self.workspace_dir = os.path.dirname(os.path.dirname(self.gui_dir))

        # Keep rendering and locale-sensitive output stable across developer
        # desktops and CI runners. Individual tests may override a value by
        # populating launch_env before calling super().setUp().
        self.configure_deterministic_environment()

        # Path to compiled binary. CARGO_TARGET_DIR is honoured so a run
        # inside the test container can keep its artifacts out of a host
        # checkout's target/ (different toolchain, different glibc).
        target_dir = os.environ.get("CARGO_TARGET_DIR") or os.path.join(
            self.workspace_dir, "target")
        self.bin_path = os.path.join(target_dir, "debug", self.app_name)
        if not os.path.exists(self.bin_path):
            raise RuntimeError(f"Binary not found at {self.bin_path}. Run 'cargo build' first.")

        # Clear leftovers this harness itself launched. Never `pkill -x`:
        # that matched by name across the whole machine, so a journey could
        # kill a developer's own running copy of the app — unsaved work and
        # all — or another run's process on a different display (#241).
        _terminate_owned(self.app_name)

        # Start recording before the app launches: a startup crash or a
        # window that never appears is exactly the failure whose video is
        # worth having, and it happens in the first second.
        self._start_recording()

        # Launch app under GDK_BACKEND=x11
        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        env.update(getattr(self, "launch_env", {}))
        self.process = subprocess.Popen(
            [self.bin_path] + list(getattr(self, "launch_args", [])),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True
        )
        _register_launched(self.app_name, self.process)

        # Wait for application node in AT-SPI tree
        self.app = self.wait_for_app(self.app_name)
        self._input_trace = []
        self._activate_window()
        self.last_screenshot = None

    def configure_deterministic_environment(self):
        """Set shared, test-only launch defaults for reproducible journeys.

        This is deliberately environment-only: production binaries do not
        gain a test mode or a diagnostic action merely because the harness
        exists. Tests that need isolated settings/state can layer
        isolate_gsettings or isolate_autosave_state on top.
        """
        defaults = {
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
            "TZ": "UTC",
            "GTK_THEME": "Adwaita",
            "GDK_SCALE": "1",
            "GDK_DPI_SCALE": "1",
            "GTK_ENABLE_ANIMATIONS": "0",
            "SOURCE_DATE_EPOCH": "0",
        }
        font_config = os.path.join(self.framework_dir, "fonts.conf")
        if os.path.exists(font_config):
            defaults["FONTCONFIG_FILE"] = font_config
        self.launch_env = {**defaults, **getattr(self, "launch_env", {})}

    # ── Video evidence ─────────────────────────────────────────────────
    # A journey asserts what the app did; the recording shows it. CI keeps
    # the video for failures (what went wrong) and, when asked, for passes
    # (feature-verification evidence attached to a pull request).
    #
    #   GUI_TEST_VIDEO=off|failures|all   default: off, or all when
    #                                     GUI_TEST_VIDEO_DIR is set
    #   GUI_TEST_VIDEO_DIR=<dir>          default: tests/gui/videos
    #
    # Recording never affects a verdict: a missing ffmpeg or a recorder
    # that fails to start prints a warning and the journey runs unchanged.

    def _video_mode(self) -> str:
        mode = os.environ.get("GUI_TEST_VIDEO", "").strip().lower()
        if not mode:
            return "all" if os.environ.get("GUI_TEST_VIDEO_DIR") else "off"
        if mode in ("1", "true", "yes", "on"):
            return "all"
        if mode in ("0", "false", "no", "none"):
            return "off"
        if mode not in ("off", "failures", "all"):
            print(f"Warning: unknown GUI_TEST_VIDEO={mode!r}; recording disabled")
            return "off"
        return mode

    def _video_dir(self) -> str:
        return os.environ.get("GUI_TEST_VIDEO_DIR",
                              os.path.join(self.gui_dir, "videos"))

    def _start_recording(self):
        self._recorder = None
        self._recording_started = None
        if self._video_mode() == "off":
            return
        name = f"{type(self).__name__}.{self._testMethodName}"
        path = os.path.join(self._video_dir(), f"{name}.mp4")
        rec = screen_recorder.ScreenRecorder(path)
        if not rec.start():
            print(f"Warning: screen recording unavailable ({rec.error})")
            return
        self._recorder = rec
        self._recording_started = time.monotonic()
        # unittest skips tearDown when setUp raises, but still runs
        # cleanups — without this, an app that never reaches the AT-SPI
        # tree would leave ffmpeg running and lose the very recording
        # that shows why.
        self.addCleanup(self._finish_recording_if_running)

    def _finish_recording_if_running(self):
        """Setup-failure path: keep the clip, the run never got a verdict."""
        if getattr(self, "_recorder", None) is not None:
            self._finish_recording(failed=True)

    def _finish_recording(self, failed: bool):
        rec = getattr(self, "_recorder", None)
        if rec is None:
            return
        self._recorder = None
        keep = failed or self._video_mode() == "all"
        path = rec.stop(keep=keep)
        if not path:
            if keep and rec.error:
                print(f"Warning: no video retained ({rec.error})")
            return

        # Sidecar metadata so the evidence collector can label each clip
        # with its journey and outcome without re-running anything.
        meta = {
            "test": f"{type(self).__name__}.{self._testMethodName}",
            "app": self.app_name,
            "outcome": "failed" if failed else "passed",
            "video": os.path.basename(path),
            "wall_seconds": round(time.monotonic() - (self._recording_started or time.monotonic()), 2),
            "revision": os.environ.get("GITHUB_SHA", ""),
        }
        try:
            with open(os.path.splitext(path)[0] + ".json", "w") as f:
                json.dump(meta, f, indent=2)
        except OSError as e:
            print(f"Warning: could not write video metadata: {e}")
        print(f"Recorded {meta['outcome']} journey to {path}")

        if failed:
            artifacts_dir = os.path.join(
                self.gui_dir, "failure_artifacts",
                f"{type(self).__name__}.{self._testMethodName}")
            try:
                import shutil
                os.makedirs(artifacts_dir, exist_ok=True)
                shutil.copy(path, os.path.join(artifacts_dir, "journey.mp4"))
            except OSError as e:
                print(f"Warning: could not copy failure video: {e}")

    def wait_for_condition(self, predicate, timeout=10.0, interval=0.05,
                           description="condition"):
        """Poll an observable readiness condition and return its value."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                value = predicate()
                if value:
                    return value
            except Exception:
                pass
            time.sleep(min(interval, max(0.0, deadline - time.monotonic())))
        self.fail(f"Timed out after {timeout}s waiting for {description}")

    def wait_until(self, observe, matches, timeout=10.0, interval=0.05,
                   description="state"):
        """Poll `observe()` until `matches(value)`, then return that value.

        Unlike `wait_for_condition`, a timeout here reports the **last
        value actually observed** (#354). That is the difference between
        "Timed out waiting for the recalculated sum" and knowing it was
        still showing `Sum 50 · Count 2` — one sends you looking at the
        formula engine, the other tells you the third cell had not
        committed yet.

        Prefer this over `time.sleep(n); assertIn(...)`. A fixed wait is a
        bet on the app being finished, and a bet that pays off on an idle
        laptop loses on a loaded runner: measured with
        `GUI_TEST_SLEEP_SCALE=0.25`, three journeys failed purely because
        their waits were too short, each with a message that read like a
        product defect.
        """
        deadline = time.monotonic() + timeout
        last = None
        while True:
            try:
                last = observe()
                if matches(last):
                    return last
            except Exception as exc:
                last = f"<{type(exc).__name__}: {exc}>"
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            time.sleep(min(interval, remaining))
        self.fail(f"Timed out after {timeout}s waiting for {description}; "
                  f"last observed: {last!r}")

    def wait_for_node(self, **criteria):
        """Wait until an AT-SPI child matching criteria is exposed."""
        return self.wait_for_condition(
            lambda: self.app.child(**criteria),
            description=f"AT-SPI node {criteria!r}",
        )

    def wait_for_file(self, path, timeout=5.0):
        """Wait for a file to be created by a save/portal/recovery action."""
        return self.wait_for_condition(
            lambda: path if os.path.isfile(path) else None,
            timeout=timeout,
            description=f"file {path}",
        )

    def gapplication_action(self, action_id, action, timeout=5.0):
        """Run an app action and wait for its observable effect when needed."""
        result = subprocess.run(
            ["gapplication", "action", action_id, action],
            check=True, capture_output=True, text=True, timeout=timeout,
        )
        return result

    def trigger_snapshot(self, action_id, timeout=5.0):
        """Trigger the test-only snapshot action and return normalized JSON."""
        snapshot_path = getattr(self, "launch_env", {}).get(
            "GTK_OFFICE_SNAPSHOT_PATH",
            os.environ.get("GTK_OFFICE_SNAPSHOT_PATH"),
        )
        if not snapshot_path:
            self.fail("GTK_OFFICE_SNAPSHOT_PATH is not configured")
        try:
            os.remove(snapshot_path)
        except FileNotFoundError:
            pass
        subprocess.run(
            ["gapplication", "action", action_id, "test-snapshot"],
            check=True, capture_output=True, text=True,
        )

        def read_snapshot():
            if not os.path.exists(snapshot_path) or os.path.getsize(snapshot_path) == 0:
                return None
            try:
                with open(snapshot_path) as f:
                    return json.load(f)
            except (OSError, json.JSONDecodeError):
                return None

        return self.wait_for_condition(
            read_snapshot, timeout=timeout,
            description=f"snapshot at {snapshot_path}",
        )

    def _callTestMethod(self, method):
        # unittest.TestCase.run() calls setUp -> _callTestMethod -> tearDown
        # as one synchronous sequence with no pytest hook in between (unlike
        # plain pytest tests, unittest tests don't split into separate
        # setup/call/teardown pytest phases) — so tearDown must learn
        # whether the test failed from inside this same call, not from a
        # pytest_runtest_makereport hook, which fires too late to matter.
        try:
            method()
            self._test_failed = False
        except Exception:
            self._test_failed = True
            raise

    def tearDown(self):
        failed = getattr(self, "_test_failed", False)
        if failed:
            self._capture_failure_artifacts()
        self._finish_recording(failed)
        rawinput.click = _dogtail_click
        rawinput.keyCombo = _dogtail_key_combo
        rawinput.typeText = _dogtail_type_text
        if hasattr(self, "process") and self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()

    def _capture_failure_artifacts(self):
        """Retain enough to debug a failure without re-running it:
        screenshot, app stdout/stderr, AT-SPI tree dump, the input trace
        (every synthetic click/key/type call this test made), and the
        state snapshot file if the test used GTK_OFFICE_SNAPSHOT_PATH."""
        artifacts_dir = os.path.join(self.gui_dir, "failure_artifacts",
                                      f"{type(self).__name__}.{self._testMethodName}")
        try:
            os.makedirs(artifacts_dir, exist_ok=True)
        except OSError as e:
            print(f"Warning: could not create artifacts dir: {e}")
            return

        try:
            self.take_screenshot("failure", crop=False)
            if self.last_screenshot and os.path.exists(self.last_screenshot):
                import shutil
                shutil.copy(self.last_screenshot, os.path.join(artifacts_dir, "screenshot.png"))
        except Exception as e:
            print(f"Warning: failure screenshot capture failed: {e}")

        try:
            with open(os.path.join(artifacts_dir, "atspi_tree.txt"), "w") as f:
                f.write(self._dump_atspi_tree())
        except Exception as e:
            print(f"Warning: AT-SPI tree dump failed: {e}")

        try:
            with open(os.path.join(artifacts_dir, "input_trace.json"), "w") as f:
                json.dump(getattr(self, "_input_trace", []), f, indent=2)
        except Exception as e:
            print(f"Warning: input trace dump failed: {e}")

        try:
            if hasattr(self, "process") and self.process and self.process.poll() is None:
                self.process.terminate()
            if hasattr(self, "process") and self.process:
                out, err = self.process.communicate(timeout=2)
                with open(os.path.join(artifacts_dir, "app.log"), "w") as f:
                    f.write("--- stdout ---\n")
                    f.write(out or "")
                    f.write("\n--- stderr ---\n")
                    f.write(err or "")
        except Exception as e:
            print(f"Warning: app log capture failed: {e}")

        snapshot_path = os.environ.get("GTK_OFFICE_SNAPSHOT_PATH")
        if snapshot_path and os.path.exists(snapshot_path):
            try:
                import shutil
                shutil.copy(snapshot_path, os.path.join(artifacts_dir, "state_snapshot.json"))
            except Exception as e:
                print(f"Warning: snapshot artifact copy failed: {e}")

        print(f"Failure artifacts retained at {artifacts_dir}")

    def _dump_atspi_tree(self, max_depth: int = 12) -> str:
        lines = []

        def walk(node, depth):
            if depth > max_depth:
                return
            try:
                pos = node.position
                size = node.size
            except Exception:
                pos = size = None
            lines.append(f"{'  ' * depth}{node.roleName} {node.name!r} {pos} {size}")
            try:
                for child in node.children:
                    walk(child, depth + 1)
            except Exception:
                pass

        try:
            walk(self.app, 0)
        except Exception as e:
            lines.append(f"(tree dump failed: {e})")
        return "\n".join(lines)

    def wait_for_process_exit(self, timeout: float = 5.0) -> "int | None":
        """Poll self.process until it exits or the timeout elapses. A fixed
        sleep before checking poll() is a common source of flaky failures
        under load (Xvfb/AT-SPI daemon contention slows real exits down);
        this waits only as long as actually needed."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            code = self.process.poll()
            if code is not None:
                return code
            time.sleep(0.1)
        return self.process.poll()

    def _activate_window(self):
        """dogtail.rawinput's keyboard/mouse synthesis goes through AT-SPI's
        DeviceEventController (Registry.generateKeyboardEvent/
        generateMouseEvent), which is a D-Bus call asking the AT-SPI
        registry daemon to synthesize XTEST events on the caller's behalf.
        In this harness that call succeeds without error but the events
        never arrive at the app — confirmed by direct probing: xdotool's
        own XTest calls (which bypass the AT-SPI registry entirely) work
        correctly against the exact same window in the exact same
        session. So this replaces the 3 rawinput entry points test_smoke
        actually uses (click/keyCombo/typeText) with xdotool-backed
        equivalents scoped to this test's window, rather than trying to
        fix the AT-SPI daemon's XTest bridge itself.

        Re-activating the window before every call (not just once here)
        matters too: a test that clicks "New Document" or opens a dialog
        after setUp can end up with a widget that never got real X focus,
        since GTK's internal focus-follows-widget-creation doesn't
        re-assert top-level X focus on its own."""
        win_id = None
        try:
            result = subprocess.run(
                ["xdotool", "search", "--pid", str(self.process.pid)],
                capture_output=True, text=True, timeout=5,
            )
            win_ids = [w for w in result.stdout.split() if w]
            if win_ids:
                win_id = win_ids[-1]
                subprocess.run(["xdotool", "windowactivate", "--sync", win_id],
                                capture_output=True, timeout=5)
        except (FileNotFoundError, subprocess.TimeoutExpired) as e:
            print(f"Warning: window activation failed ({e}); rawinput tests may not receive input")
            return
        if win_id is None:
            return
        # `_win_id` is the *current* input target, not a constant: a
        # cross-application journey moves it between two running apps with
        # `focus_app`. The closures below read the attribute on every call
        # rather than capturing it, so switching focus needs no re-patching.
        self._win_id = win_id
        self._primary_win_id = win_id

        def reactivate():
            try:
                subprocess.run(
                    ["xdotool", "windowactivate", "--sync", self._win_id],
                    capture_output=True, timeout=2)
            except (FileNotFoundError, subprocess.TimeoutExpired):
                pass

        def trace(kind, **fields):
            trace_list = getattr(self, "_input_trace", None)
            if trace_list is not None:
                trace_list.append({"time": time.monotonic(), "type": kind, **fields})

        def click(x, y, button=1, check=True):
            trace("click", x=x, y=y, button=button)
            reactivate()
            subprocess.run(
                ["xdotool", "mousemove", "--sync", str(int(x)), str(int(y)),
                 "click", str(button)],
                capture_output=True, timeout=5,
            )

        def key_combo(combo_string):
            trace("key_combo", combo=combo_string)
            # Dogtail's own syntax: '<Control>k' -> tokens ['Control', 'k'].
            # Reuse its alias table (control -> Control_L etc.) so the
            # tokens are valid X keysym names, then join for xdotool.
            tokens = []
            for s in combo_string.split('<'):
                if s:
                    for tok in s.split('>'):
                        if tok:
                            tokens.append(rawinput.keyNameAliases.get(tok.lower(), tok))
            reactivate()
            subprocess.run(
                ["xdotool", "key", "--window", self._win_id, "+".join(tokens)],
                capture_output=True, timeout=5,
            )

        def type_text(text):
            trace("type_text", text=text)
            reactivate()
            subprocess.run(
                ["xdotool", "type", "--window", self._win_id, "--", text],
                capture_output=True, timeout=10,
            )

        rawinput.click = click
        rawinput.keyCombo = key_combo
        rawinput.typeText = type_text

    # ── A second application (#442 cross-app clipboard) ────────────────
    # The X11 clipboard is a negotiation between two live processes: one
    # owns the selection, the other asks it for a format. Nothing about
    # that is exercised by one app copying and pasting to itself, which is
    # what every clipboard journey did before this. The fragment
    # *conversions* are pure functions with their own unit tests; what
    # needed a second process was the transfer.

    def launch_second_app(self, app_name: str, launch_args=()):
        """Launch another suite application beside the primary one.

        Returns a handle with `.name`, `.process`, `.app` (AT-SPI node) and
        `.win_id`. Registered for cleanup, so a failing journey does not
        leave a window on the display for the next test to type into.
        """
        target_dir = os.environ.get("CARGO_TARGET_DIR") or os.path.join(
            self.workspace_dir, "target")
        bin_path = os.path.join(target_dir, "debug", app_name)
        if not os.path.exists(bin_path):
            raise RuntimeError(f"Binary not found at {bin_path}. Run 'cargo build' first.")

        _terminate_owned(app_name)

        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        env.update(getattr(self, "launch_env", {}))
        process = subprocess.Popen(
            [bin_path] + list(launch_args), env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        _register_launched(app_name, process)

        def terminate():
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
        self.addCleanup(terminate)

        app = self.wait_for_app(app_name)
        win_id = self.wait_for_condition(
            lambda: self._window_for_pid(process.pid),
            description=f"an X window for {app_name}",
        )

        handle = _SecondApp(app_name, process, app, win_id)
        self._second_apps = getattr(self, "_second_apps", [])
        self._second_apps.append(handle)
        return handle

    def _window_for_pid(self, pid: int):
        try:
            result = subprocess.run(
                ["xdotool", "search", "--pid", str(pid)],
                capture_output=True, text=True, timeout=5,
            )
        except (FileNotFoundError, subprocess.TimeoutExpired):
            return None
        ids = [w for w in result.stdout.split() if w]
        return ids[-1] if ids else None

    def focus_app(self, target=None):
        """Point synthetic input at `target` (a `launch_second_app` handle),
        or back at the primary app when called with no argument.

        Returns the AT-SPI node for the focused app, so a journey reads as
        `editor = self.focus_app(letters).child(roleName="text")`.
        """
        win_id = target.win_id if target is not None else getattr(
            self, "_primary_win_id", getattr(self, "_win_id", None))
        if win_id is None:
            self.fail("no window to focus: input was never bound to one")
        self._win_id = win_id
        subprocess.run(["xdotool", "windowactivate", "--sync", win_id],
                        capture_output=True, timeout=5)
        return target.app if target is not None else self.app

    def assert_still_running(self, *targets):
        """Every app involved is still alive. A clipboard transfer that
        crashes the *owner* is as much a failure as one that loses data,
        and the receiving app's assertion would not notice."""
        crashed = []
        if self.process.poll() is not None:
            crashed.append(f"{self.app_name} (exit {self.process.poll()})")
        for t in targets:
            if t.process.poll() is not None:
                crashed.append(f"{t.name} (exit {t.process.poll()})")
        self.assertFalse(crashed, f"process(es) died: {', '.join(crashed)}")

    def drag(self, x1: float, y1: float, x2: float, y2: float, button: int = 1):
        """Press-move-release from (x1, y1) to (x2, y2), window-local
        coordinates — same xdotool-backed approach as the click/keyCombo/
        typeText replacements in _activate_window (AT-SPI's own synthetic
        input doesn't reach the app here, see #129). No rawinput
        equivalent exists to shadow, so this is a plain method rather
        than another monkey-patched rawinput entry point."""
        win_id = getattr(self, "_win_id", None)
        if win_id is None:
            return
        trace_list = getattr(self, "_input_trace", None)
        if trace_list is not None:
            trace_list.append({
                "time": time.monotonic(), "type": "drag",
                "x1": x1, "y1": y1, "x2": x2, "y2": y2,
            })
        subprocess.run(["xdotool", "windowactivate", "--sync", win_id],
                        capture_output=True, timeout=2)
        subprocess.run(
            ["xdotool", "mousemove", "--sync", str(int(x1)), str(int(y1))],
            capture_output=True, timeout=5,
        )
        subprocess.run(["xdotool", "mousedown", str(button)], capture_output=True, timeout=5)
        # A couple of intermediate points so GTK's GestureDrag sees real
        # motion, not a single jump — matches how a human drag arrives.
        for frac in (0.34, 0.67, 1.0):
            mx = x1 + (x2 - x1) * frac
            my = y1 + (y2 - y1) * frac
            subprocess.run(
                ["xdotool", "mousemove", "--sync", str(int(mx)), str(int(my))],
                capture_output=True, timeout=5,
            )
            time.sleep(0.05)
        subprocess.run(["xdotool", "mouseup", str(button)], capture_output=True, timeout=5)

    def wait_for_app(self, name: str, timeout: float = 15.0) -> "tree.Node":
        return self.wait_for_condition(
            lambda: tree.root.application(name),
            timeout=timeout,
            description=f"application '{name}' in the AT-SPI registry",
        )

    def get_window_geometry(self) -> tuple[int, int, int, int]:
        """Locates the main frame window of the app and returns (x, y, w, h)."""
        try:
            frame = self.app.child(roleName='frame')
            if frame:
                return frame.position[0], frame.position[1], frame.size[0], frame.size[1]
        except Exception as e:
            print(f"Warning: Failed to get frame geometry via Dogtail: {e}")
        return 0, 0, 1920, 1080

    # ── Vision verification ──────────────────────────────────────────────
    VLM_BACKEND = os.environ.get("VLM_BACKEND", "gemini")
    LEMONADE_URL = "https://lemonade.manatee-basking.ts.net/v1/chat/completions"
    LEMONADE_MODEL = os.environ.get("VLM_LEMONADE_MODEL", "Gemma-4-31B-it-GGUF")
    GEMINI_API_KEY = os.environ.get("GEMINI_API_KEY", "")
    GEMINI_API_KEY_2 = os.environ.get("GEMINI_API_KEY_2", "")
    GEMINI_MODEL = os.environ.get("VLM_GEMINI_MODEL", "gemini-2.5-flash")
    _gemini_key_tried = False

    def _vlm_request(self, image_b64: str, prompt: str, model: str = None) -> str:
        """Send an image + prompt to the configured VLM backend and return the text response."""
        backend = self.VLM_BACKEND

        if backend == "lemonade":
            model = model or self.LEMONADE_MODEL
            resp = requests.post(
                self.LEMONADE_URL,
                json={
                    "model": model,
                    "messages": [{
                        "role": "user",
                        "content": [
                            {"type": "text", "text": prompt},
                            {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{image_b64}"}},
                        ]
                    }],
                    "max_tokens": 512,
                },
                timeout=120,
            )
            resp.raise_for_status()
            data = resp.json()
            msg = data["choices"][0]["message"]
            return msg.get("reasoning_content") or msg.get("content") or ""

        elif backend == "gemini":
            if not self.GEMINI_API_KEY:
                self.skipTest("GEMINI_API_KEY not set")
            model = model or self.GEMINI_MODEL
            keys = [self.GEMINI_API_KEY]
            if self.GEMINI_API_KEY_2:
                keys.append(self.GEMINI_API_KEY_2)
            last_error = None
            for key in keys:
                url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}"
                resp = requests.post(url, json={
                    "contents": [{"parts": [
                        {"inline_data": {"mime_type": "image/jpeg", "data": image_b64}},
                        {"text": prompt},
                    ]}],
                }, timeout=30)
                if resp.status_code == 429:
                    last_error = "Rate limited (429), trying fallback key"
                    continue
                if resp.status_code == 503:
                    last_error = "Model overloaded (503), trying fallback"
                    continue
                resp.raise_for_status()
                data = resp.json()
                candidates = data.get("candidates", [])
                if candidates:
                    parts = candidates[0].get("content", {}).get("parts", [])
                    return "".join(p.get("text", "") for p in parts)
                return ""
            raise RuntimeError(f"Gemini API failed: {last_error}")

        else:
            raise ValueError(f"Unknown VLM_BACKEND: {backend}. Use 'lemonade' or 'gemini'.")

    def assertVision(
        self,
        checks: list,
        screenshot_path: str = None,
        model: str = None,
    ):
        """
        Assert visual UI state against a list of checks using a VLM.

        Each check is either:
          - a string (auto-named assertion)
          - a dict {"name": "...", "prompt": "..."}
        """
        # Normalise checks to list of dicts
        normalised = []
        for i, c in enumerate(checks):
            if isinstance(c, str):
                normalised.append({"name": f"check-{i}", "prompt": c})
            else:
                normalised.append(c)

        # Capture screenshot if not provided
        if screenshot_path is None:
            self.take_screenshot("vlm")
            screenshot_path = os.path.join(self.gui_dir, f"{self.app_name}_screenshot_vlm.png")

        if not os.path.exists(screenshot_path):
            self.fail(f"Screenshot not found: {screenshot_path}")

        # Resize and encode
        img = Image.open(screenshot_path)
        img.thumbnail((800, 600), Image.LANCZOS)
        buf = BytesIO()
        img.save(buf, format="JPEG", quality=70)
        image_b64 = base64.b64encode(buf.getvalue()).decode()

        # Build structured prompt
        checks_json = json.dumps([
            {"id": c["name"], "assertion": c["prompt"]}
            for c in normalised
        ], indent=2)

        prompt = (
            "You are a GUI testing assistant. Verify each assertion about the screenshot.\n\n"
            f"Assertions:\n{checks_json}\n\n"
            "For each assertion, say \"Result: Pass.\" or \"Result: Fail.\" with brief evidence."
        )

        response = self._vlm_request(image_b64, prompt, model=model)

        # ── Parse VLM response ──────────────────────────────────────────
        # The reasoning model outputs structured text like:
        #   **Assertion check-0: ...**
        #   ... evidence ...
        #   Result: Pass.
        # We parse this directly.

        results = []
        for c in normalised:
            cid = c["name"]
            # Search for this check in response
            idx = response.lower().find(cid.lower())
            if idx < 0:
                # Try the first 20 chars of the assertion text
                short = c["prompt"][:20].lower()
                idx = response.lower().find(short)
            if idx < 0:
                idx = 0

            start = max(0, idx - 30)
            para = response[start:start+600]

            passed = None
            if re.search(r'Result\s*[:.]\s*Pass', para, re.IGNORECASE):
                passed = True
            elif re.search(r'Result\s*[:.]\s*Fail', para, re.IGNORECASE):
                passed = False
            elif re.search(r'Status\s*[:.]\s*Pass', para, re.IGNORECASE):
                passed = True
            elif re.search(r'Status\s*[:.]\s*Fail', para, re.IGNORECASE):
                passed = False
            elif re.search(r'\bPASS\b', para):
                passed = True
            elif re.search(r'\bFAIL\b', para):
                passed = False

            # Extract evidence
            ev = para.strip()
            ev = re.sub(r'^[\d.\s*\-`#_~]+', '', ev).strip()
            evidence = ev[:250]

            if passed is not None:
                results.append({"id": cid, "pass": passed, "evidence": evidence})
            else:
                print(f"  ? {cid}: ambiguous VLM output, defaulting to FAIL")
                results.append({"id": cid, "pass": False, "evidence": "Could not determine pass/fail from VLM"})

        if not results:
            print(f"? VLM response could not be parsed. Raw:\n{response[:500]}")
            self.fail(f"VLM assertion failed: could not parse response for {[c['name'] for c in normalised]}")
            return

        # ── Log results and assert ──────────────────────────────────────
        all_pass = True
        for r in results:
            cid = r.get("id", "?")
            passed = r.get("pass", False)
            evidence = r.get("evidence", "")
            status = "PASS" if passed else "FAIL"
            icon = "+" if passed else "x"
            print(f"  [{icon}] {cid}: {status} — {evidence}")
            if not passed:
                all_pass = False

        reported_ids = {r.get("id") for r in results}
        for c in normalised:
            if c["name"] not in reported_ids:
                print(f"  [?] {c['name']}: not evaluated by VLM")
                all_pass = False

        self.assertTrue(all_pass, f"{len([r for r in results if not r.get('pass', False)])} visual assertion(s) failed")

    # ── Screenshot helpers ─────────────────────────────────────────────────

    def take_screenshot(self, suffix: str, crop: bool = True) -> str | None:
        """Takes a screenshot, stores path in self.last_screenshot, returns it."""
        output_filename = f"{self.app_name}_screenshot"
        if suffix:
            output_filename += f"_{suffix}"
        output_filename += ".png"

        output_path = os.path.join(self.gui_dir, output_filename)

        display = os.environ.get("DISPLAY")
        if not display:
            print("Warning: DISPLAY not set, skipping screenshot.")
            return None

        try:
            with mss.mss() as sct:
                sct.shot(output=output_path)

            if crop:
                x, y, w, h = self.get_window_geometry()
                if w > 0 and h > 0:
                    img = Image.open(output_path)
                    cropped_img = img.crop((x, y, x + w, y + h))
                    cropped_img.save(output_path)
                    print(f"Saved window-cropped screenshot to {output_path}")
                else:
                    print(f"Saved full-screen screenshot to {output_path} (invalid geometry: {x},{y},{w},{h})")
            else:
                print(f"Saved full-screen screenshot to {output_path}")
            self.last_screenshot = output_path
            return output_path
        except Exception as e:
            print(f"Failed to capture screenshot: {e}")
            return None
