# Production motion settings

`vocab/direction.json` now supplies the previously embedded motion values under
`motion`. Packets override them through `set.direction.motion`; the existing
runtime state/set API reads and edits the same resolved values.

| Group | Controls |
| --- | --- |
| intro | Pre-drop roll, tilts, starting zoom, zoom rise, and riser length/zoom/blur |
| phrase_stutter | Duration, oscillation frequencies, aim displacement, zoom, roll, tear, punch |
| ending | Lead in bars, zoom/aim blend, tilt/roll decay |
| shot_transient | Duration, strength, tear/punch gains |
| event_accents | Impact envelope decay and director-added lock tear/aberration |

The first four groups have `enabled`. Event accents have independent
`impact_enabled` and `lock_enabled` switches. Disabling lock accents removes
only the director's added tear and aberration: it does not hide the lock box or
disable the shader's separate response to the lock signal.

Example packet override:

```json
{"set":{"direction":{"motion":{
  "intro":{"enabled":false},
  "phrase_stutter":{"enabled":false},
  "ending":{"lead_bars":1.0,"aim_mix":0.5}
}}}}
```

Times ending in `_s` are seconds, riser/ending lengths are bars, frequencies are
Hz, aim displacement is logical pixels, and rotation amplitudes are radians.
Ending mixes and decays are in [0,1]. Durations must stay positive even when a
group is disabled. Unknown fields and invalid values fail direction validation.

There are no Rust fallback values for these groups: a custom standalone direction
vocabulary must supply them; packets inheriting the shipped vocabulary need no
changes. The shipped values preserve the previous camera and look states across
the bundled analysis fixture, checked against a pre-change regression fingerprint.

This step retains existing scheduling and ordering: behavior/shot, lyric camera,
intro, phrase stutter, ending, modulation, framing constraints. Ending begins at
the earlier of impact and duration minus lead_bars. It does not introduce a new
camera scheduler or migrate lyric, drawing, shader, or scene-selection defaults.
