# Screen recording for GUI journeys.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Journeys already prove behavior to a machine (AT-SPI assertions) — this
# proves it to a human: a video of the real application doing the thing,
# recorded from the same Xvfb display the assertions ran against, so the
# recording cannot drift from what was tested.
#
# ffmpeg's x11grab captures the whole virtual screen. It is deliberately
# optional: a missing ffmpeg degrades to "no video", never to a failed or
# silently skipped test. Recording is not an assertion and must not gate.

import os
import shutil
import subprocess
import sys
import time

DEFAULT_SIZE = "1920x1080"
DEFAULT_FPS = 15

# GIF is what renders inline in a pull request comment; keep it small
# enough to load and wide enough to read a toolbar label.
GIF_FPS = 8
GIF_WIDTH = 960


def ffmpeg_path() -> "str | None":
    return shutil.which("ffmpeg")


def available() -> bool:
    return ffmpeg_path() is not None


def screen_size(display: "str | None" = None) -> str:
    """Geometry of the X display, as ffmpeg's -video_size wants it.

    x11grab has to be told the size up front; asking X is more reliable
    than assuming the harness default, because a caller may have started
    Xvfb at another resolution.
    """
    explicit = os.environ.get("GUI_TEST_SCREEN_SIZE")
    if explicit:
        return explicit
    xdpyinfo = shutil.which("xdpyinfo")
    if xdpyinfo:
        env = os.environ.copy()
        if display:
            env["DISPLAY"] = display
        try:
            out = subprocess.run([xdpyinfo], capture_output=True, text=True,
                                 env=env, timeout=10).stdout
            for line in out.splitlines():
                if "dimensions:" in line:
                    return line.split()[1]
        except (OSError, subprocess.SubprocessError):
            pass
    return DEFAULT_SIZE


class ScreenRecorder:
    """Records an X display to H.264 for the lifetime of one journey."""

    def __init__(self, output_path, display=None, size=None, fps=DEFAULT_FPS):
        self.output_path = output_path
        self.display = display or os.environ.get("DISPLAY")
        self.size = size
        self.fps = fps
        self.process = None
        self.error = None

    def start(self) -> bool:
        """Begin recording. Returns False (with self.error set) if the
        recorder cannot run; callers continue testing regardless."""
        if self.process is not None:
            return True
        if not available():
            self.error = "ffmpeg not installed"
            return False
        if not self.display:
            self.error = "DISPLAY not set"
            return False

        os.makedirs(os.path.dirname(os.path.abspath(self.output_path)) or ".",
                    exist_ok=True)
        size = self.size or screen_size(self.display)
        cmd = [
            ffmpeg_path(), "-hide_banner", "-loglevel", "error", "-y",
            "-f", "x11grab",
            "-draw_mouse", "1",
            "-framerate", str(self.fps),
            "-video_size", size,
            "-i", self.display,
            # x264 needs even dimensions; an odd Xvfb geometry would
            # otherwise fail the encode at the very end of a run, when
            # the evidence is already gone.
            "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "28",
            "-pix_fmt", "yuv420p",
            self.output_path,
        ]
        try:
            self.process = subprocess.Popen(
                cmd, stdin=subprocess.PIPE,
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        except OSError as e:
            self.error = f"could not start ffmpeg: {e}"
            self.process = None
            return False

        # A recorder that died immediately (bad display, missing encoder)
        # must be reported now, not discovered as a zero-byte file later.
        time.sleep(0.5)
        if self.process.poll() is not None:
            _, err = self.process.communicate()
            self.error = (err or "").strip() or "ffmpeg exited immediately"
            self.process = None
            return False
        return True

    def stop(self, keep=True) -> "str | None":
        """Finish the recording and return the file path, or None if there
        is nothing usable. `keep=False` discards it (a passing test whose
        video nobody asked for)."""
        if self.process is None:
            return None
        proc, self.process = self.process, None
        try:
            # 'q' is ffmpeg's clean shutdown: it flushes and writes the
            # moov atom. Killing it instead leaves an unplayable file.
            proc.communicate(input="q", timeout=15)
        except subprocess.TimeoutExpired:
            proc.terminate()
            try:
                proc.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.communicate()

        if not keep:
            _remove(self.output_path)
            return None
        if not os.path.exists(self.output_path) or os.path.getsize(self.output_path) == 0:
            self.error = "recording produced no output"
            _remove(self.output_path)
            return None
        return self.output_path

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, exc_type, exc, tb):
        self.stop(keep=True)
        return False


def _remove(path):
    try:
        os.remove(path)
    except OSError:
        pass


def to_gif(video_path, gif_path=None, fps=GIF_FPS, width=GIF_WIDTH) -> "str | None":
    """Convert a recording to a GIF that GitHub renders inline.

    Two passes: a palette generated from the actual frames, then applied.
    A single pass uses a fixed 256-color web palette and turns GTK's
    subtle greys into visible banding.
    """
    if not available() or not video_path or not os.path.exists(video_path):
        return None
    gif_path = gif_path or os.path.splitext(video_path)[0] + ".gif"
    scale = f"fps={fps},scale={width}:-1:flags=lanczos"
    palette = gif_path + ".palette.png"
    try:
        subprocess.run([ffmpeg_path(), "-hide_banner", "-loglevel", "error", "-y",
                        "-i", video_path, "-vf", f"{scale},palettegen=stats_mode=diff",
                        palette], check=True, timeout=600)
        subprocess.run([ffmpeg_path(), "-hide_banner", "-loglevel", "error", "-y",
                        "-i", video_path, "-i", palette,
                        "-lavfi", f"{scale}[x];[x][1:v]paletteuse=dither=bayer",
                        gif_path], check=True, timeout=600)
    except (OSError, subprocess.SubprocessError) as e:
        print(f"Warning: GIF conversion failed: {e}")
        _remove(gif_path)
        return None
    finally:
        _remove(palette)
    return gif_path


def to_poster(video_path, poster_path=None, seek="-1") -> "str | None":
    """Grab a still from near the end of a recording — the end state is
    what a reviewer wants to see first."""
    if not available() or not video_path or not os.path.exists(video_path):
        return None
    poster_path = poster_path or os.path.splitext(video_path)[0] + ".png"
    try:
        subprocess.run([ffmpeg_path(), "-hide_banner", "-loglevel", "error", "-y",
                        "-sseof", seek, "-i", video_path, "-frames:v", "1",
                        poster_path], check=True, timeout=120)
    except (OSError, subprocess.SubprocessError) as e:
        print(f"Warning: poster frame extraction failed: {e}")
        _remove(poster_path)
        return None
    return poster_path


def duration_seconds(video_path) -> "float | None":
    ffprobe = shutil.which("ffprobe")
    if not ffprobe or not video_path or not os.path.exists(video_path):
        return None
    try:
        out = subprocess.run(
            [ffprobe, "-v", "error", "-show_entries", "format=duration",
             "-of", "default=noprint_wrappers=1:nokey=1", video_path],
            capture_output=True, text=True, check=True, timeout=60).stdout
        return float(out.strip())
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def _main(argv):
    usage = ("usage: recorder.py gif <video> [out.gif]\n"
             "       recorder.py poster <video> [out.png]\n"
             "       recorder.py check")
    if not argv:
        print(usage, file=sys.stderr)
        return 2
    command = argv[0]
    if command == "check":
        print(f"ffmpeg: {ffmpeg_path() or 'not found'}")
        return 0 if available() else 1
    if command in ("gif", "poster"):
        if len(argv) < 2:
            print(usage, file=sys.stderr)
            return 2
        out = (to_gif if command == "gif" else to_poster)(
            argv[1], argv[2] if len(argv) > 2 else None)
        if not out:
            return 1
        print(out)
        return 0
    print(usage, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
