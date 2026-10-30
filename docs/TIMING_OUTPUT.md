# Shared timing and output contract

`config/output.json` is the source of the currently supported renderer profile.
The track build script generates Rust constants; Python reads the same file;
the lens receives shader constants generated from those Rust values. Both
encoder entry points use its codec settings.

The installed fframes Video interface requires compile-time FPS and dimensions.
This first contract therefore supports the existing 30 fps, 1920×1080 logical
and output space, with a 2× flat intermediate. Unsupported profile edits fail
explicitly. This is not yet per-packet resolution or frame-rate selection.
Changing the JSON requires rebuilding both render binaries. Existing scene
coordinates and schema-2 analysis still constrain the supported profile.

## Time

- Frame indices are zero-based. Frame i starts at i/FPS seconds and owns the
  half-open interval [i/FPS, (i+1)/FPS).
- Events remain in seconds. Quantizing a trigger to its nearest frame rounds
  positive half-frame ties upward. It does not change the authored event time.
- Duration is quantized to f32, as in Track, before computing the nearest frame
  count. Python uses the same f32 multiplication and tie rule as Rust.
- Frame count N means valid indices 0 through N-1; video duration is N/FPS.
  Source audio duration remains authoritative metadata and can differ by up to
  half a frame. This contract does not resample or pad the source audio.
- Audio analysis frame i uses sample bounds floor(i*sample_rate/FPS) through
  floor((i+1)*sample_rate/FPS), excluding the end. This avoids cumulative drift.
- Schema 2 retains `total_frames_at_30fps` for compatibility. A different frame
  rate requires an explicit schema migration, not reinterpretation of that key.

## Coordinates and output

Stage geometry uses logical pixels. Flat dimensions are logical dimensions
multiplied by flat_scale. The shader uses that same scale to sample the flat
image. Camera projection and overlays retain the current output dimensions.
The intermediate and final encoder settings are separate entries in the profile.

Range rendering continues to use the existing CLI and CINEMA_OFFSET_S path;
this change does not redefine its seek semantics. Full production manifests,
per-packet profiles, creative defaults, and browser prototype migration remain
separate work.
