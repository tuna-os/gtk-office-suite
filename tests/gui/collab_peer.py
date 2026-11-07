"""A third person for the Letters collaboration journeys, with no window.

`CollabPeer` is a real `collabd` (the sidecar the app itself runs) driven over
its stdin/stdout the way the app drives it (suite-collab/src/protocol.rs, one JSON
object per line). It joins a shared document and then does what a Letters
instance that has not caught up would do: it never answers `applied` for the
genesis or any update, so its replica holds nothing, and what it saves is a
version that does not cover what the others hold. That is the "uncovered head"
of RFC-0005 2.5, the one that makes the other instances show the two-versions
banner. It is a stand-in for a person who saved from an older copy.

It also keeps every `heads` event the sidecar sent, so a journey can say what
the version graph looked like to somebody other than the two windows.
"""

import base64
import io
import json
import os
import subprocess
import threading
import time
import zipfile


def docx_bytes(*paragraphs):
    """The smallest .docx Letters opens: one paragraph per argument."""
    body = "".join(f"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>" for text in paragraphs)
    parts = {
        "[Content_Types].xml": (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Override PartName="/word/document.xml" '
            'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
            '</Types>'),
        "_rels/.rels": (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" '
            'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
            'Target="word/document.xml"/></Relationships>'),
        "word/document.xml": (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
            f'<w:body>{body}</w:body></w:document>'),
    }
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as package:
        for name, content in parts.items():
            package.writestr(name, content)
    return buffer.getvalue()


class CollabPeer:
    def __init__(self, collabd_bin, data_dir):
        os.makedirs(data_dir, exist_ok=True)
        self._proc = subprocess.Popen(
            [collabd_bin, "--data-dir", data_dir],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self._lock = threading.Condition()
        self.events = []
        self._next_req = 1
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self._proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except ValueError:
                event = {"event": "unparsed", "line": line}
            with self._lock:
                self.events.append(event)
                self._lock.notify_all()

    def _send(self, cmd, **fields):
        req = self._next_req
        self._next_req += 1
        self._proc.stdin.write(json.dumps({"req": req, "cmd": cmd, **fields}) + "\n")
        self._proc.stdin.flush()
        return req

    def _answer(self, req, timeout):
        """The event that answers `req`; an `error` for it raises."""
        deadline = time.monotonic() + timeout
        seen = 0
        with self._lock:
            while True:
                for event in self.events[seen:]:
                    if event.get("req") == req:
                        if event.get("event") == "error":
                            raise AssertionError(f"collabd refused {req}: {event}")
                        return event
                seen = len(self.events)
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise AssertionError(f"collabd never answered request {req}; saw {self.events[-5:]}")
                self._lock.wait(remaining)

    def _call(self, cmd, timeout=60.0, **fields):
        return self._answer(self._send(cmd, **fields), timeout)

    def connect(self, server):
        """Hello and connect to the Durable Streams server; this peer's key."""
        self._call("hello", protocol=1, client="gui-test-peer")
        return self._call("connect", binding="streams", config={"server": server})["principal"]

    def join(self, code):
        """Join the shared document. Nothing it is sent is ever answered
        `applied`: this peer holds none of the document."""
        self.doc = self._call("join", invite=code)["doc"]
        return self.doc

    def publish_version(self, data, message=None):
        """Save `data` (a .docx) as a version, from what this peer holds, which
        is nothing: the version covers no update."""
        fields = {"doc": self.doc, "bytes": base64.b64encode(data).decode()}
        if message:
            fields["message"] = message
        return self._call("publish_version", **fields)["id"]

    def latest_heads(self):
        """(ids of the heads, conflict) of the newest `heads` event, or None."""
        with self._lock:
            for event in reversed(self.events):
                if event.get("event") == "heads":
                    return [h["id"] for h in event["heads"]], bool(event.get("conflict"))
        return None

    def close(self):
        try:
            self._proc.stdin.close()
        except Exception:
            pass
        try:
            self._proc.wait(timeout=5)
        except Exception:
            self._proc.kill()
