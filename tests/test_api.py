"""The library and HTTP interface, run against the real packet and planning binaries."""

import json
import shutil
import sys
import threading
import time
import urllib.error
import urllib.request
import wave
from pathlib import Path

import numpy as np
import pytest

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "python"))
import cinema  # noqa: E402
from cinema import server  # noqa: E402

NAME = "apitest_tone"


@pytest.fixture(scope="module")
def tone(tmp_path_factory):
    """A 31 s stereo wav with the duration of data/notes.json, under a name with spaces and capitals."""
    d = tmp_path_factory.mktemp("api")
    path = d / "My Tone (take 1).WAV"
    t = np.arange(31 * 48000) / 48000
    x = (0.3 * np.sin(2 * np.pi * 110 * t) * (1 + np.sin(2 * np.pi * 2 * t)) / 2 * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
        w.writeframes(np.repeat(x[:, None], 2, axis=1).tobytes())
    yield path
    shutil.rmtree(ROOT / "out" / NAME, ignore_errors=True)


@pytest.fixture(scope="module")
def project(tone, tmp_path_factory):
    root = tmp_path_factory.mktemp("projects")
    return cinema.Project.create(tone, notes=ROOT / "data" / "notes.json", name=NAME, root=root)


def test_create_accepts_any_file_name_and_refuses_a_second_project_of_the_same_name(tone, project):
    assert project.document["inputs"]["audio"] == str(tone.resolve())
    with pytest.raises(cinema.CinemaError):
        cinema.Project.create(tone, notes=ROOT / "data" / "notes.json", name=NAME, root=project.dir.parent)
    with pytest.raises(cinema.CinemaError):
        cinema.Project.create(tone.parent / "absent.mp3", root=project.dir.parent)


def test_prepare_then_plan(project):
    assert not project.status()["prepared"]
    assert project.prepare()["prepared"]
    plan = project.plan()
    assert plan["scenes"] and plan["recording"]["duration_s"] == pytest.approx(31.0, abs=0.1)


def test_set_persists_and_an_invalid_value_leaves_the_packet_unchanged(project):
    assert project.set("direction.fit", 0.9)["direction"]["fit"] == 0.9
    before = project.packet_path.read_text()
    with pytest.raises(cinema.CinemaError):
        project.set("direction.fit", 5.0)
    assert project.packet_path.read_text() == before
    with pytest.raises(cinema.CinemaError):
        project.set("nonsense.fit", 1)
    assert project.unset("direction.fit")["direction"] == {}


def test_http_interface(project):
    srv = server.make_server(0, root=project.dir.parent)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{srv.server_address[1]}"

    def call(method, path, body=None):
        req = urllib.request.Request(base + path, method=method, data=json.dumps(body).encode() if body is not None else None)
        try:
            with urllib.request.urlopen(req) as r:
                return r.status, json.loads(r.read())
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read())

    try:
        assert call("GET", "/projects") == (200, {"projects": [NAME]})
        code, st = call("GET", f"/projects/{NAME}")
        assert code == 200 and st["prepared"]
        code, plan = call("GET", f"/projects/{NAME}/plan")
        assert code == 200 and plan["scenes"]
        assert call("PUT", f"/projects/{NAME}/set", {"path": "direction.fit", "value": 0.88})[0] == 200
        assert call("GET", f"/projects/{NAME}/overrides")[1]["direction"]["fit"] == 0.88
        code, err = call("PUT", f"/projects/{NAME}/set", {"path": "direction.fit", "value": 9})
        assert code == 400 and "error" in err
        assert call("GET", "/projects/nothing_here")[0] == 400
        assert call("GET", "/elsewhere")[0] == 404
        code, job = call("POST", f"/projects/{NAME}/prepare")
        assert code == 202
        for _ in range(120):
            state = call("GET", f"/jobs/{job['job']}")[1]
            if state["state"] != "running":
                break
            time.sleep(0.5)
        assert state["state"] == "done", state
        assert call("GET", "/jobs/unknown")[0] == 404
    finally:
        srv.shutdown()
