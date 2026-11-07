import json
import subprocess
import sys
from pathlib import Path

TOOL = Path(__file__).resolve().parent.parent / "tools" / "make_packet.py"


def run(*args):
    return subprocess.run([sys.executable, str(TOOL), *map(str, args)], capture_output=True, text=True)


def test_any_file_name_and_extension_gives_a_usable_run_name(tmp_path):
    audio = tmp_path / "My Demo (take 2).MP3"
    audio.write_bytes(b"x")
    r = run(audio, "--packets", tmp_path)
    assert r.returncode == 0, r.stderr
    doc = json.loads((tmp_path / "my_demo_take_2.json").read_text())
    assert doc["name"] == "my_demo_take_2"
    assert doc["extends"] == "default.json"
    assert doc["inputs"] == {"audio": str(audio.resolve()), "notes": None}
    assert doc["transcriber"][-2:] == ["{audio}", "{notes}"]


def test_notes_replace_the_transcriber(tmp_path):
    audio, notes = tmp_path / "a.flac", tmp_path / "n.json"
    audio.write_bytes(b"x")
    notes.write_text("{}")
    assert run(audio, "--notes", notes, "--packets", tmp_path).returncode == 0
    doc = json.loads((tmp_path / "a.json").read_text())
    assert doc["inputs"]["notes"] == str(notes.resolve())
    assert "transcriber" not in doc


def test_existing_packet_is_not_overwritten_and_missing_audio_fails(tmp_path):
    audio = tmp_path / "s.wav"
    audio.write_bytes(b"x")
    assert run(audio, "--packets", tmp_path).returncode == 0
    assert run(audio, "--packets", tmp_path).returncode != 0
    assert run(audio, "--packets", tmp_path, "--force").returncode == 0
    assert run(tmp_path / "absent.wav", "--packets", tmp_path).returncode != 0
