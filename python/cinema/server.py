"""A local HTTP interface to projects, with JSON bodies and background jobs for the slow steps.

    GET    /projects                          names
    POST   /projects                          {"audio_path": ..., "notes_path": ..., "name": ...}  -> status
    PUT    /projects/NAME/audio?filename=F    raw body is the recording; creates the project
    GET    /projects/NAME                     status
    DELETE /projects/NAME
    POST   /projects/NAME/prepare             -> {"job": id}
    GET    /projects/NAME/plan                the scene plan
    GET    /projects/NAME/overrides
    PUT    /projects/NAME/set                 {"path": "direction.fit", "value": 0.9}
    POST   /projects/NAME/render              {"scenes": [..], "small": false} -> {"job": id}
    GET    /projects/NAME/frame/N?stage=lens  PNG
    GET    /projects/NAME/video               the rendered video
    GET    /jobs/ID                           {"state": running|done|failed, "result"|"error"}

The server binds to 127.0.0.1 and has no authentication; `audio_path` names a file on this
machine, so it is meant for the person running it and not for a network.
"""

from __future__ import annotations

import json
import re
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

from .project import PROJECTS, CinemaError, Project, slug


class Jobs:
    def __init__(self):
        self.jobs: dict[str, dict] = {}
        self.locks: dict[str, threading.Lock] = {}
        self.guard = threading.Lock()

    def start(self, project: str, fn) -> str:
        jid = uuid.uuid4().hex[:12]
        self.jobs[jid] = {"state": "running", "project": project}
        with self.guard:
            lock = self.locks.setdefault(project, threading.Lock())

        def work():
            with lock:  # one pipeline step at a time per project: they share its run folder
                try:
                    self.jobs[jid].update(state="done", result=fn())
                except Exception as e:  # reported to the caller, which polls the job
                    self.jobs[jid].update(state="failed", error=str(e))

        threading.Thread(target=work, daemon=True).start()
        return jid


def make_server(port: int = 8780, root: Path = PROJECTS) -> ThreadingHTTPServer:
    jobs = Jobs()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def _send(self, code, body, ctype="application/json"):
            data = body if isinstance(body, bytes) else json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def _body(self) -> bytes:
            return self.rfile.read(int(self.headers.get("Content-Length") or 0))

        def _json(self) -> dict:
            raw = self._body()
            return json.loads(raw) if raw else {}

        def _route(self, method):
            u = urlparse(self.path)
            parts = [p for p in u.path.split("/") if p]
            qs = parse_qs(u.query)
            try:
                result = self._dispatch(method, parts, qs)
            except CinemaError as e:
                return self._send(400, {"error": str(e)})
            except (json.JSONDecodeError, KeyError, ValueError) as e:
                return self._send(400, {"error": f"bad request: {e}"})
            if result is None:
                return self._send(404, {"error": "not found"})
            code, body, *ctype = result
            self._send(code, body, *ctype)

        def _dispatch(self, m, p, qs):
            if p == ["projects"]:
                if m == "GET":
                    return 200, {"projects": Project.names(root)}
                if m == "POST":
                    b = self._json()
                    return 201, Project.create(b["audio_path"], b.get("notes_path"), b.get("name"), root).status()
            if len(p) >= 2 and p[0] == "jobs" and m == "GET":
                j = jobs.jobs.get(p[1])
                return (200, j) if j else None
            if len(p) < 2 or p[0] != "projects":
                return None
            name = slug(p[1])
            if m == "PUT" and p[2:] == ["audio"]:
                fn = Path(qs.get("filename", ["audio"])[0]).name
                d = Path(root) / name / "upload"
                d.mkdir(parents=True, exist_ok=True)
                (d / fn).write_bytes(self._body())
                return 201, Project.create(d / fn, None, name, root, overwrite=True).status()
            proj = Project(name, root)
            rest = p[2:]
            if m == "GET" and not rest:
                return 200, proj.status()
            if m == "DELETE" and not rest:
                proj.delete()
                return 200, {"deleted": name}
            if m == "POST" and rest == ["prepare"]:
                return 202, {"job": jobs.start(name, proj.prepare)}
            if m == "GET" and rest == ["plan"]:
                return 200, proj.plan()
            if m == "GET" and rest == ["overrides"]:
                return 200, proj.overrides()
            if m == "PUT" and rest == ["set"]:
                b = self._json()
                return 200, proj.set(b["path"], b["value"])
            if m == "POST" and rest == ["render"]:
                b = self._json()
                return 202, {"job": jobs.start(name, lambda: str(proj.render(scenes=b.get("scenes"), small=bool(b.get("small")))))}
            if m == "GET" and len(rest) == 2 and rest[0] == "frame":
                png = proj.frame(int(rest[1]), qs.get("stage", ["lens"])[0])
                return 200, png, "image/png"
            if m == "GET" and rest == ["video"]:
                v = proj.status()["video"]
                return (200, Path(v).read_bytes(), "video/mp4") if v else (404, {"error": "not rendered yet"})
            return None

        def do_GET(self):
            self._route("GET")

        def do_POST(self):
            self._route("POST")

        def do_PUT(self):
            self._route("PUT")

        def do_DELETE(self):
            self._route("DELETE")

    return ThreadingHTTPServer(("127.0.0.1", port), Handler)


def serve(port: int = 8780) -> None:
    srv = make_server(port)
    print(f"cinema api on http://127.0.0.1:{srv.server_address[1]}")
    srv.serve_forever()
