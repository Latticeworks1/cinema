# MIDI transcription review tool

`tools/midi_toolkit.py` runs the local MuScriptor transcriber when requested,
checks its note events against the audio, and writes a reviewable MIDI plus
`audit.json`. It uses the already installed model through `tools/transcribe.py`;
the 16 kHz PCM conversion is temporary. The output directory contains only
`notes.json`, `refined.mid`, and `audit.json` for a new run.

From the cinema repository root, with Python, NumPy, SciPy, `mido`, and ffmpeg
available:

```sh
/usr/bin/python3 tools/midi_toolkit.py "/path/to/audio.opus" \
  --output "/path/to/output" --transcribe
```

To review an existing transcription without rerunning the large model:

```sh
/usr/bin/python3 tools/midi_toolkit.py "/path/to/audio.opus" \
  --notes "/path/to/notes.json" --output "/path/to/output" \
  --gain electric_bass=105 --gain distorted_electric_guitar=85 \
  --gain synth_lead=90 --gain drums=55
```

The tool estimates BPM by fitting melodic onsets to an eighth-note grid. It
also searches the waveform for a strongly repeated span; when the two estimates
agree, the audio repeat determines BPM and spectral onsets place the grid in
time. Notes are then snapped to that grid. If no reliable repeat is found, a
tight note-grid fit can still remove a small phase offset. Use `--bpm` or
`--shift-ms` to override automatic timing, or `--bpm-range MIN MAX` to constrain
the search. `audit.json` records the evidence and decisions, MIDI and audio
durations, onset agreement, per-note harmonic support, frequency-band power,
and the track CC7 levels used.

The spectral checks flag notes for listening review. Overlapping instruments
can support a wrong pitch, and a soft note can lack an obvious spectral onset.
MuScriptor does not predict velocity, so notes export at velocity 80. CC7 gains
are playback starting points chosen by the user; the mixed recording cannot
reveal independent instrument gain settings. The installed engine exposes an
instrument-selection setting, but no pitch threshold or velocity parameter to
fit. The tool does not claim to optimize model inference or instrument gains.
It keeps detected labels and pitches intact for review.
