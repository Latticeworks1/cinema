"""Command line: python3 -m cinema COMMAND (or bin/cinema COMMAND)."""

import argparse
import json
import sys

from .project import CinemaError, Project


def main(argv=None):
    ap = argparse.ArgumentParser(prog="cinema", description="Make a music-driven video from a recording.")
    sub = ap.add_subparsers(dest="cmd", required=True)
    n = sub.add_parser("new", help="start a project from an audio file of any name and format")
    n.add_argument("audio")
    n.add_argument("--notes")
    n.add_argument("--name")
    sub.add_parser("list", help="list projects")
    for c, h in (("status", "show what exists for a project"), ("prepare", "transcribe and analyse"), ("plan", "print the scene plan as JSON")):
        sub.add_parser(c, help=h).add_argument("name")
    s = sub.add_parser("set", help="set one field, e.g. cinema set song direction.fit 0.9")
    s.add_argument("name"); s.add_argument("path"); s.add_argument("value", help="JSON, or plain text")
    r = sub.add_parser("render", help="render the video; unchanged scenes are reused")
    r.add_argument("name"); r.add_argument("-o", "--out"); r.add_argument("--small", action="store_true")
    r.add_argument("--scenes", help="comma-separated scene numbers to force")
    f = sub.add_parser("frame", help="write one frame as a PNG")
    f.add_argument("name"); f.add_argument("index", type=int); f.add_argument("-o", "--out", required=True)
    f.add_argument("--stage", choices=["flat", "lens"], default="lens")
    v = sub.add_parser("serve", help="start the local HTTP interface")
    v.add_argument("--port", type=int, default=8780)
    a = ap.parse_args(argv)
    try:
        if a.cmd == "serve":
            from .server import serve
            return serve(a.port)
        if a.cmd == "new":
            out = Project.create(a.audio, a.notes, a.name).status()
        elif a.cmd == "list":
            out = Project.names()
        else:
            p = Project(a.name)
            if a.cmd == "status":
                out = p.status()
            elif a.cmd == "prepare":
                out = p.prepare()
            elif a.cmd == "plan":
                out = p.plan()
            elif a.cmd == "set":
                try:
                    value = json.loads(a.value)
                except json.JSONDecodeError:
                    value = a.value
                out = p.set(a.path, value)
            elif a.cmd == "render":
                scenes = [int(x) for x in a.scenes.split(",")] if a.scenes else None
                out = str(p.render(a.out, scenes, a.small))
            else:
                open(a.out, "wb").write(p.frame(a.index, a.stage))
                out = a.out
        print(json.dumps(out, indent=2))
    except CinemaError as e:
        sys.exit(f"[!] Error: {e}")


if __name__ == "__main__":
    main()
