# Audio Transcription and Notes Ingestion in cinema

## Overview

`cinema` does not synthesize or guess choreography; every cut, camera motion, visual mark, and modulation oscillator is driven deterministically by musical events (note onsets, offsets, pitches, and stem assignments).

To render a project, the pipeline requires:
1. An audio soundtrack (`.wav` or `.mp3`).
2. A transcription file (`notes.json`) or a transcriber command configured in the project packet.

---

## Tooling Context & Recommendations

* **NeuralNote**: Located locally at `/Users/m1a4xnetworkprobe./Applications/NeuralNote.app`. NeuralNote is a modern neural audio-to-MIDI transcriber that provides superior polyphonic and onset tracking compared to older, stale utilities (such as basic-pitch).
* **Whisper / Specialized Transcribers**: Modern neural audio models (and whisper-based audio/speech/alignment variants) provide robust feature extraction.
* **Separation of Concerns**: Because NeuralNote operates as an interactive standalone app and audio plugin (AU/VST3), MIDI files exported from NeuralNote or a DAW can be converted to the `notes.json` schema to drive `cinema`.

---

## The `notes.json` Schema

The pipeline (`python/export_analysis.py`) expects a JSON document with the following structure:

```json
{
  "sample_rate": 16000,
  "seconds": 31.0,
  "notes": [
    {
      "onset": 0.04,
      "offset": 0.05,
      "pitch": 36,
      "program": 128,
      "is_drum": true,
      "instrument": "drums"
    },
    {
      "onset": 6.22,
      "offset": 7.77,
      "pitch": 35,
      "program": 33,
      "is_drum": false,
      "instrument": "electric_bass"
    },
    {
      "onset": 6.22,
      "offset": 6.41,
      "pitch": 54,
      "program": 2,
      "is_drum": false,
      "instrument": "electric_piano"
    }
  ]
}
```

### Stem Mapping (`vocab/export.json`)
Notes are routed to stems during export analysis:
- **Drums** (`is_drum: true` or `program: 128`):
  - `kick`: General MIDI pitch 35, 36.
  - `snare`: General MIDI pitch 37, 38, 39, 40.
  - `hat`: General MIDI pitch 42, 44, 46.
  - Rest: `perc`.
- **Melodic Instruments**:
  - Filtered through `stems.aliases` in `vocab/export.json` (e.g. `electric_bass` -> `bass`, `electric_piano` -> `piano`).

---

## Packet Configuration for Audio & Transcription

Packets under `packets/` define run configurations:

### Option A: Audio + Provided Notes
```json
{
  "extends": "default.json",
  "name": "spicyface",
  "inputs": {
    "audio": "../data/SPICYFACE.mp3",
    "notes": "../data/spicyface_notes.json"
  }
}
```

### Option B: Audio-Only with External Transcriber Command
```json
{
  "extends": "default.json",
  "name": "spicyface",
  "inputs": {
    "audio": "../data/SPICYFACE.mp3",
    "notes": null
  },
  "transcriber": ["python3", "tools/midi_to_notes.py", "{audio}", "{notes}"]
}
```
When `packet prepare` runs, `{audio}` is passed to the command and it must produce the `{notes}` file before `export_analysis.py` continues.
