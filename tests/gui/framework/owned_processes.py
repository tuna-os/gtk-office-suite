"""Process ownership for GUI journeys.

A journey may only clean up processes it started. The harness used to run
`pkill -x <app>` before each launch, which matches by name across the whole
machine: on a developer's desktop that killed their own open copy of Letters
along with its unsaved work, and on a shared runner it reached into another
run's private display. #241's exit evidence is "no global process killing",
so leftovers are tracked rather than hunted.

Cleanup at the end of a test is per-test (tearDown for the primary app,
addCleanup for a second app); this registry is the pre-launch sweep, for a
process a crashed test left behind inside the same run.

Kept free of GTK, AT-SPI and imaging dependencies so the ownership property
can be asserted in the plain-Python test lane, with no display.
"""

import subprocess

_LAUNCHED: "dict[str, list[subprocess.Popen]]" = {}


def register(app_name: str, process) -> None:
    """Record a process this harness started."""
    _LAUNCHED.setdefault(app_name, []).append(process)


def terminate_owned(app_name: str) -> int:
    """Terminate still-running `app_name` processes this harness launched.

    Returns how many were terminated. A process nobody registered is left
    alone — that is the whole point.
    """
    terminated = 0
    remaining = []
    for process in _LAUNCHED.get(app_name, []):
        if process.poll() is not None:
            continue
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                # Reported rather than escalated: a process that survives
                # SIGKILL is a kernel-level problem, and pretending it is
                # gone would make the next launch fail confusingly.
                print(f"Warning: {app_name} pid {process.pid} did not exit")
                remaining.append(process)
                continue
        terminated += 1
    _LAUNCHED[app_name] = remaining
    return terminated


def owned(app_name: str) -> "list[subprocess.Popen]":
    """The live processes this harness owns for `app_name`."""
    return [p for p in _LAUNCHED.get(app_name, []) if p.poll() is None]
