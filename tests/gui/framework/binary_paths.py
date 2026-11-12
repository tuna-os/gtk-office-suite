"""Where the journeys find the app binaries.

Pure on purpose (no AT-SPI import), so the plain-Python lane can test it.
"""

import os

# The Cargo profile directory the binaries are looked up in. Journeys run
# against debug builds; the release soak builds and soaks release binaries
# (`.github/workflows/release-soak.yml`: "the thing being soaked has to be the
# thing being shipped") and says so with GUI_TEST_PROFILE=release.
PROFILE_ENV = "GUI_TEST_PROFILE"
DEFAULT_PROFILE = "debug"


def profile() -> str:
    """The Cargo profile directory to look in: `debug` unless the environment names another."""
    return os.environ.get(PROFILE_ENV, "").strip() or DEFAULT_PROFILE


def binary_path(target_dir: str, app_name: str) -> str:
    """`<target_dir>/<profile>/<app_name>`."""
    return os.path.join(target_dir, profile(), app_name)
