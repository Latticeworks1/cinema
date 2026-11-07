# Programmatic interface

The pipeline is exposed through one library, a command line and a local HTTP service that share a single implementation, `python/cinema/project.py`. Each operation runs the same binaries and tools as the shell scripts, so a project edited through the interface renders identically to one rendered by hand.

## Projects

A project is a recording and the packet that describes its run. It is created from an audio file in any container that ffmpeg reads, under any file name; the file stays where it is and the project refers to it by absolute path. When a transcription is supplied the run is filmed from both inputs, and otherwise the packet transcribes the audio during preparation. Projects live in `projects/NAME/packet.json`, extend `packets/default.json`, and write their outputs under `out/NAME/`. The name is the file name reduced to lower-case letters, digits and underscores unless one is given.

The packet is the only state. A field set through the interface is written into the packet's `set` section, so it persists between calls and is seen by every later operation, including a render started from the shell with `tools/render_scenes.py projects/NAME/packet.json`. A field is addressed by a dotted path whose first component is one of the packet sections (`feel`, `vocabulary`, `matrix`, `direction`, `signals`, `export`, `anchors`). Each edit is checked by planning with it applied; a value the direction configuration rejects raises an error and the packet keeps its previous contents.

## Library

    import cinema
    p = cinema.Project.create("take 2.mp3", notes=None, name="take2")
    p.prepare()
    p.plan()                          # scene boundaries, palette, behaviour, look, layout
    p.set("direction.fit", 0.9)
    p.frame(300, stage="flat")        # PNG bytes
    p.render(out="take2.mp4", small=True)

`render` draws only the scenes whose inputs changed since the previous render, using the per-scene fingerprints of `tools/render_scenes.py`, and `scenes=[...]` forces particular scenes. With `small=True` it also writes a 1080p H.264 copy. A lens frame reads the flat pass video, so `frame(..., stage="lens")` requires an earlier render and otherwise raises an error that says so. Every failure raises `cinema.CinemaError` with the step that failed and the tail of its output.

## Command line

    bin/cinema new "take 2.mp3" --name take2
    bin/cinema prepare take2
    bin/cinema set take2 direction.fit 0.9
    bin/cinema frame take2 300 --stage flat -o frame.png
    bin/cinema render take2 -o take2.mp4 --small
    bin/cinema serve --port 8780

Results are printed as JSON. Values given to `set` are parsed as JSON and taken as plain text when they are not valid JSON.

## HTTP service

`bin/cinema serve` binds to 127.0.0.1 and has no authentication, because `audio_path` names a file on the machine; it is intended for the person running it. Bodies are JSON. The slow steps return a job identifier that is polled until its state is `done` or `failed`; steps on one project run one at a time, since they share its run folder.

    GET    /projects
    POST   /projects                         {"audio_path", "notes_path"?, "name"?}
    PUT    /projects/NAME/audio?filename=F   raw body is the recording
    GET    /projects/NAME                    what exists: prepared, planned, rendered, overrides
    DELETE /projects/NAME
    POST   /projects/NAME/prepare            {"job"}
    GET    /projects/NAME/plan
    GET    /projects/NAME/overrides
    PUT    /projects/NAME/set                {"path", "value"}
    POST   /projects/NAME/render             {"scenes"?, "small"?}  ->  {"job"}
    GET    /projects/NAME/frame/N?stage=lens
    GET    /projects/NAME/video
    GET    /jobs/ID

A bad request or a failed step answers 400 with `{"error": ...}`; an unknown route or job answers 404.
