# ff-cinema

ff-cinema turns a transcribed recording into a short film in which every camera move, cut, colour, shape and display texture is derived from the music. The input is one recording and its multitrack MIDI transcription. The output is a 1920x1080 video with the original audio, rendered by fframes on Skia.

## Principles

The picture may depend on the track and on nothing else. Each visual parameter is therefore a function of measured features of the stems, and anything that is a matter of taste lives in one of four authored data files instead of in code. A change of taste is an edit to a file, and a change of music is a new analysis. Any frame can be computed on its own, because every stage is a pure function of the frame index; this allows frames to render in parallel and makes every result reproducible.

The track carries a transcription with separate stems: drums (split here into kick, snare and hat by General MIDI pitch), bass and piano. Each stem is treated as an independent signal. The drums set timing, the bass sets the slow and heavy motion, and the piano sets the fine motion and the colour.

## Architecture

The workspace is a chain of five crates, each depending only on those before it.

`track` reads `data/analysis.json`, which `python/export_analysis.py` writes after verifying the SHA-256 of the audio and the transcription. It refuses a file that is malformed, out of order or inconsistent, and exposes stems, tempo, the structure (the drop, the impact, the bar length), chords and the loudness curve.

`signals` provides the bus and the feel model. The bus answers, for any stem and time, six numbers in 0..1: the note impulse (`hit`), whether a note is held (`gate`), the register of the latest event (`pitch`), the interval from the one before (`leap`), the playing density over the last bar (`density`) and the time since the stem last sounded (`quiet`). The mix adds its loudness and its position in the bar and the beat. The feel model measures two axes for any span of the track, edge (short, high, fast notes are sharp and long, low, slow notes are round) and energy (loud and busy against quiet and sparse). Both are standardised against the distribution over every bar of the same song, so a value describes how a section departs from the rest of the track.

`direct` derives the film. Scene boundaries come from the drop, the impact, phrases counted in bars from the drop and the entrance or exit of any stem after a silence; they are snapped to a drum onset and thinned so no scene is shorter than a minimum number of bars. The stem whose playing departs most upward from its own average leads the scene. For every other choice, the director takes the entry of the vocabulary (`vocab/vocabulary.json`) whose position on the feel axes is nearest to the scene, which selects the palette, the camera behaviour, the look, the layout, and, per stem, the shape of its notes and the physical display it is filmed on. A modulation matrix (`vocab/matrix.json`) then drives the camera and look parameters continuously from the bus, with optional smoothing and with a lead that reads the score ahead of the frame. `Plan::frame` returns the complete state of one frame.

`cinema-flat` draws the stage in white on black at 2x. Each stem is a panel in which its notes pass a playhead or circle a clock face, the ends of each mark are sharp or round according to the scene's shape, and every onset throws the shape's particles as closed-form functions of the note and its age. The only colour is pure red, which marks a lock on the leading stem.

`cinema-lens` films that image with a virtual camera. The shader applies perspective, depth of field focused on the note being tracked, the scene's palette, bloom, chromatic aberration, the tear on hits and a display model in which each panel has its own subpixel geometry (stripe, dot, delta or block), then the exposure, vignette and grain of the scene's look. The camera's own tracker is drawn sharp on top.

The two passes exist because a fframes shader can sample only preloaded images, so the stage is rendered to a video first and the lens shader reads it back.

## Authored files

`vocab/feel.json` sets how the two axes are computed. `vocab/vocabulary.json` lists the shapes, palettes, camera behaviours, looks, display materials and layouts, each placed on the feel axes. `vocab/matrix.json` lists the routes from signals to parameters. `vocab/direction.json` holds the scene rules, such as phrase length and the minimum scene length. Each loader rejects unknown fields, unknown parameter names, stems that the vocabulary does not list and malformed values.

## Running

Run everything from the workspace root.

    tools/render.sh

writes `out/cinema.mp4`. On an M3 Pro the whole pipeline takes about 2.5 minutes for the 31 s recording: roughly 20 s for the flat pass and 125 s for the lens pass. `cargo test -p <crate>` runs the tests of one crate, and `cargo run -p direct --bin plan` prints the scene plan and writes `out/plan.json`.

The flat and lens crates are built in separate cargo invocations, as the script does. fframes needs its `compile-time-svgtree` feature to place a shader layer in a frame, which the lens crate enables, and a frame built from a string, which the flat crate uses, cannot be parsed with that feature on.

Build products are shared with a sibling `../ff-unmute` checkout through `.cargo/config.toml`; delete that file to use a local `target/` directory instead.

## Provenance

`python/export_analysis.py`, the Catmull-Rom spline, the keyed noise and the JetBrains Mono font come from the author's earlier ff-unmute and ff-tracking studies. The font is under the SIL Open Font License (`licenses/`). The rest of the code is MIT licensed.
