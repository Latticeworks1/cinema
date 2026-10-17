# ff-cinema

ff-cinema turns a transcribed recording into a short film in which every camera move, cut, colour, shape and display texture is derived from the music. The input is one recording and its multitrack MIDI transcription. The output is a 1920x1080 video with the original audio, rendered by fframes on Skia.

## Principles

The picture may depend on the track and on nothing else. Each visual parameter is therefore a function of measured features of the stems, and anything that is a matter of taste lives in one of five authored data files instead of in code, and every value in those files can be overridden for a single run by a project packet. A change of taste is an edit to a file or a packet, and a change of music is a new packet. Any frame can be computed on its own, because every stage is a pure function of the frame index; this allows frames to render in parallel and makes every result reproducible.

The track carries a transcription with separate stems: drums (split here into kick, snare and hat by General MIDI pitch), bass and piano. Each stem is treated as an independent signal. The drums set timing, the bass sets the slow and heavy motion, and the piano sets the fine motion and the colour.

## Architecture

The workspace is a chain of six crates, each depending only on those before it.

`packet` reads a project packet, the one file that names a run's inputs and configuration (see below), and resolves where every input and output of the run lives.

`track` reads `data/analysis.json` (schema 2), which `python/export_analysis.py` writes after verifying the SHA-256 of the audio and the transcription. It refuses a file that is malformed, out of order or inconsistent, and exposes stems, tempo, the structure (the drop, the impact, the bar length), chords and the loudness curve.

`signals` provides the bus and the feel model. The bus answers, for any stem and time, six numbers in 0..1: the note impulse (`hit`), whether a note is held (`gate`), the register of the latest event (`pitch`), the interval from the one before (`leap`), the playing density over the last bar (`density`) and the time since the stem last sounded (`quiet`). The mix adds its loudness and its position in the bar and the beat. The feel model measures two axes for any span of the track, edge (short, high, fast notes are sharp and long, low, slow notes are round) and energy (loud and busy against quiet and sparse). Both are standardised against the distribution over every bar of the same song, so a value describes how a section departs from the rest of the track. The bus also exposes rhythmic oscillators inferred from the onset times alone (`signals::rhythm`), without reading the tempo or bar length: recurring lags between events, kept when they exceed every jittered surrogate of the track, become oscillators ranked from the shortest period, each with a phase, a folded pulse, a local strength and a confidence, addressed in the matrix as `rhythm.<rank>.<phase|pulse|amp|conf>`.

`direct` derives the film. Scene boundaries come from the drop, the impact, phrases counted in bars from the drop and the entrance or exit of any stem after a silence; they are snapped to a drum onset and thinned so no scene is shorter than a minimum number of bars. The stem whose playing departs most upward from its own average leads the scene. For every other choice, the director takes the entry of the vocabulary (`vocab/vocabulary.json`) whose position on the feel axes is nearest to the scene, which selects the palette, the camera behaviour, the look, the layout, and, per stem, the shape of its notes and the physical display it is filmed on. A modulation matrix (`vocab/matrix.json`) then drives the camera and look parameters continuously from the bus, with optional smoothing and with a lead that reads the score ahead of the frame. `Plan::frame` returns the complete state of one frame.

`cinema-flat` draws the stage in white on black at 2x. Each stem is a panel in which its notes pass a playhead or circle a clock face, the ends of each mark are sharp or round according to the scene's shape, and every onset throws the shape's particles as closed-form functions of the note and its age. The only colour is pure red, which marks a lock on the leading stem.

`cinema-lens` films that image with a virtual camera. The shader applies perspective, depth of field focused on the note being tracked, the scene's palette, bloom, chromatic aberration, the tear on hits and a display model in which each panel has its own subpixel geometry (stripe, dot, delta or block), then the exposure, vignette and grain of the scene's look. The camera's own tracker is drawn sharp on top.

The two passes exist because a fframes shader can sample only preloaded images, so the stage is rendered to a video first and the lens shader reads it back.

## Authored files

`vocab/export.json` sets how the exporter reads the transcription. Every instrument the transcriber names becomes a stem (`aliases` renames, drums are split by General MIDI pitch group). The tempo is estimated from the onsets of all stems together: the period with the strongest comb score in one octave of tempi (`bpm_range`), refined on a finer grid, accepted only if it beats jittered copies of the onsets (`min_confidence`); otherwise the export stops unless `fallback_bpm` is set. The drop and the chord root default to the melodic stem lowest in register and the impact to one beat after the last drum hit. The file also holds the bar length in beats, the chord shapes and weights, and the frequency bands measured. A stem the vocabulary does not list gets its `default_ink`, though the lens shader has `MAX_REGIONS` (6) region slots and so at most six stems. `vocab/signals.json` sets how the bus and the rhythm oscillators are measured: the decay of a note impulse, the look-back windows, and the lag resolution, number of surrogates, false-positive rate and harmonics of the rhythm fit. `vocab/feel.json` sets how the two axes are computed. `vocab/vocabulary.json` lists the shapes, palettes, camera behaviours, looks, display materials and layouts, each placed on the feel axes. `vocab/matrix.json` lists the routes from signals to parameters. `vocab/direction.json` holds the scene rules, such as phrase length and the minimum scene length, and the groups that shape the picture: the camera's attention, the stage rectangle and how layouts divide it, the camera limits, the boundary priorities, the sizes of the marks in the flat pass, the tracker and the font. Each loader rejects unknown fields, unknown parameter names, stems that the vocabulary does not list and malformed values.

## Project packets

A packet is a JSON file such as `packets/default.json`. It names the inputs (`inputs.audio`, `inputs.notes`, or both), the directory of authored files (`vocab`), the fonts (`media`), the output root (`out`) and a run name, and may override any key of any authored file (feel, vocabulary, matrix, direction, signals, export) under `set`. Paths are relative to the packet. A packet may `extends` another and then states only what differs; objects merge key by key, other values replace, and `null` clears a value.

    { "extends": "default.json", "name": "calm",
      "set": { "direction": { "phrase_bars": 4, "marks": { "note_h": 12 } },
               "signals": { "rhythm": { "surrogates": 400 } } } }

With audio and notes the run is filmed with its soundtrack. With notes alone it has no soundtrack and the loudness curve counts the notes sounding in each frame (`source.level_source` in `analysis.json` says which). With audio alone the packet must give a `transcriber`, a command that writes the notes file, with `{audio}` and `{notes}` standing for the two paths; the repository ships no transcriber. See [`docs/TRANSCRIPTION.md`](docs/TRANSCRIPTION.md) for details on the `notes.json` format, stem mapping, and integration with transcribers like NeuralNote. Unknown keys, a missing input, an unnamed section and a loop in `extends` are refused. `packet show <packet>` prints what a packet resolves to.

The frame size and rate are compile-time properties of the render passes (`WIDTH`, `HEIGHT` and `FPS` in the fframes `Video` trait), and the lens shader keeps its own focal length, exposure and display constants; neither is set by a packet.

## Running

Run everything from the workspace root.

    tools/render.sh [packet.json]

without an argument it uses `packets/default.json`, and writes `out/<name>/cinema.mp4`, with the analysis, plan and intermediate video beside it. On an M3 Pro the whole pipeline takes about 2.5 minutes for the 31 s recording: roughly 20 s for the flat pass and 125 s for the lens pass. `cargo test -p <crate>` runs the tests of one crate, and `CINEMA_PACKET=<packet> cargo run -p direct --bin plan` prints the scene plan and writes `plan.json` into the run directory.

The flat and lens crates are built in separate cargo invocations, as the script does. fframes needs its `compile-time-svgtree` feature to place a shader layer in a frame, which the lens crate enables, and a frame built from a string, which the flat crate uses, cannot be parsed with that feature on.

Build products are shared with a sibling `../ff-unmute` checkout through `.cargo/config.toml`; delete that file to use a local `target/` directory instead.

## Provenance

`python/export_analysis.py`, the Catmull-Rom spline, the keyed noise and the JetBrains Mono font come from the author's earlier ff-unmute and ff-tracking studies. The font is under the SIL Open Font License (`licenses/`). The rest of the code is MIT licensed.
