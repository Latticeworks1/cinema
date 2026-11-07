# springtime

This folder is a complete cinema project: everything that belongs to one piece and to no other. The packet `packets/p_stems.json` extends the shipped default, names the stems as its input, and carries the authored cues, lyric timing and collage plates as its direction. The program and its tests do not depend on this folder.

The recording is not included. Place the exported stems and a master mix in `audio/` (the packet expects `audio/p_stems_Master.wav` and the per-stem files that `data/p_stems_map.json` maps to instruments), or link the folder that holds them to `audio`. The folder `audio` is excluded from version control.

The pictures in `media/` are the nine images chosen for this piece. Seven have a `cut_` companion made by the Vision subject lift (`tools/subject_cutout.py`), and the other two are used whole because their subject is the entire frame. The originals are in `data/springtime/`. `data/p_stems_media.json` records which pictures are used and which stay whole. The pinned plates in `data/p_stems_plates.json` are the hand-placed pictures. Every other plate in the packet is generated from the music by `tools/make_collage.py` with the settings in `vocab/collage.json` and the per-piece overlay `data/p_stems_collage.json`, so changing the seed or the images and running the collage step again rewrites them.

From the repository root, the whole sequence of analysis, staging, collage and render runs as

    python3 tools/produce.py examples/springtime/packets/p_stems.json

and `--small` adds a compressed 1080p copy. Output goes to `out/p_stems/`. The collage design is described in `docs/COLLAGE.md` and the image anchors in `docs/ANCHORS.md`.
