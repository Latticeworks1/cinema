# Reusable fframes audiovisual production system

This is the standing architectural directive for new productions built from the
ff-tracking, ff-unmute, and cinema work. It describes what to reuse and what to
derive from each creative brief. The [README](../README.md) describes cinema's
current implementation; this document does not claim that every parameter is
already configurable in code.

## Core principle

Use the existing fframes rendering architecture wherever applicable. Inspect
the implementation before adding functionality. Reuse established components,
configure their behavior, and extend only what the brief cannot reasonably
express. Preserve tested functionality unless a demonstrated limitation
justifies replacing it.

The engine should retain the ability to render substantially different
audiovisual works without rewriting its camera, shader, projection, media,
synchronization, or verification infrastructure. Project-specific creative
choices are inputs to that infrastructure, not architectural requirements.

## Stable engine and variable production

| Component | Reusable responsibility | Derived from each brief |
| --- | --- | --- |
| fframes | Frame generation, media handling, encoding | Duration, resolution, frame rate |
| Rust workspace | Rendering infrastructure and shared logic | Scene modules and project configuration |
| SVG renderer | Procedural scene construction | Subject, geometry, typography, composition |
| SkSL shader | Camera projection and image processing | Optical treatment and effect intensity |
| Camera | Continuous motion and projection | Path, framing, pacing, focus |
| Projection utilities | Coordinate transforms and overlay alignment | Tracked subjects and annotations |
| Synchronization | Time-aligned events and measurements | Event vocabulary, mappings, triggers |
| Audio pipeline | Generation, analysis, validation | Composition, instrumentation, sound design |
| Verification | Technical tests and visual inspection | Project-specific acceptance criteria |

Preserve the established `hud`, `flat`, and `lens` structure when extending
ff-unmute. These names identify working components, not creative concepts that
every new production must depict. Existing cinema crates may serve the same
responsibilities under their own names. Extend or configure them instead of
duplicating their functionality.

## Camera

Reuse the continuous parametric camera and its keyframe motion, Catmull–Rom
interpolation, position and orientation controls, zoom, perspective, tilt,
roll, offsets, focus, blur, temporal interpolation, and camera-space
projection. Keep shader transforms and CPU-rendered overlays aligned.

Define paths through configurable keyframes or generated trajectories. Let the
brief determine their count, timing, parameters, and whether motion responds to
events. Do not encode ff-unmute's camera route as the engine's default behavior.

## Shader

Preserve the SkSL pipeline's perspective and image-plane projection, depth of
field, color transformation, bloom, grain, texture, distortion, screen-space
effects, time modulation, and external controls. Treat these as independently
selectable capabilities. A production may use only some of them.

Configure palettes, distortion, lighting, and audio responses per project. For
substantially different effects, extend the shader or add a processing stage
while preserving coordinate conventions and CPU projection compatibility.

## Scene renderer

Reuse procedural SVG frame generation for primitives, paths, vector art, text,
images, generated assets, repeated structures, layers, animated transforms,
data-driven geometry, and time-dependent states. Scene content must not assume
a timeline, interface, or diagram. Keep scene construction independent of
camera and post-processing where practical, so a scene can be filmed in more
than one way.

## Rendering pipeline

Keep the established two-pass approach as the default when working from the
original codebase:

1. Generate the scene's geometry, layout, animation, and content as SVG.
2. Film the first-pass output through SkSL for camera projection, depth,
   optical treatment, and compositing.
3. Combine processed imagery with required overlays, annotations, and audio.

Reuse existing media providers and intermediate files where compatible. Add a
pass when a technical requirement warrants it, not for organizational symmetry.

The same-frame SVG-to-shader input described in
[fframes #210](https://github.com/dmtrKovalenko/fframes/issues/210) was proposed
to remove the intermediate 4K video round trip. It is a possible future
optimization, not an assumed capability of the installed fframes version.
Preserve the working two-pass path until the actual API supports an alternative
and that alternative passes the production's visual and encoding checks.

## Synchronization and audio

Keep deterministic frame-to-time mapping and consistent interpolation. Support
arbitrary time-indexed events and continuous measurements: audio onsets,
intensity, animation triggers, camera targets, object states, or external
data. Event types and visual consequences belong in project mappings. Do not
duplicate timing by hand across independent components. Use generated shared
data or constants where appropriate.

Reuse programmatic audio generation, signal processing, event generation, and
analysis when applicable. Tempo, key, instruments, arrangement, rhythm,
loudness target, spectrum, and effects belong to the production brief. Preserve
the audio-first approval gate when the project calls for it. Once approved,
keep the audio sample-identical through later stages.

## Configuration boundary

A new production should primarily supply a creative brief, project
configuration, source assets or generators, scene definitions, camera and
shader parameters, synchronization mappings, and acceptance criteria. Derive
values from the brief and production data when reliable; expose overrides for
precise control. Avoid a broad plugin framework or options for values the
system can derive. Start with a thin boundary around elements that actually
vary between productions.

## Established engineering constraints

Carry forward verified fixes from the existing productions:

- Respect SkSL reserved identifiers.
- Return a valid root SVG from every frame render; never return `Svgr::empty()`.
- When a frame fails late, report the failure promptly and check open encoder
  segments. [fframes #209](https://github.com/dmtrKovalenko/fframes/issues/209)
  documents a case where draining unfinished x264 segments delayed the visible
  `NoRootNode` error, making the render appear hung.
- Keep unsupported files out of `include_media_dir!`.
- Preserve the established loading of intermediate video and audio.
- Keep shader camera transforms compatible with CPU projection.
- Retain deterministic timing and event synchronization.
- Check the installed fframes API before changing implementation details.

These are compatibility constraints, not artistic instructions.

## Procedure for each production

1. Inspect the existing implementation and identify reusable capabilities.
2. Configure scene, camera, shader, and synchronization behavior that already
   exists.
3. Extend only the capabilities the brief cannot reasonably express.
4. Preserve compatibility with existing productions unless a change is
   justified by a demonstrated limitation.
5. Verify rendering, synchronization, camera behavior, optical processing,
   audio integrity, and final encoding against the project's acceptance
   criteria.

The objective is maximum reuse without narrowing the creative brief. The
engine should remember how to make things; it should not dictate what gets
made.
