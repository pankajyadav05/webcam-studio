# Live cutout tuning

All three values are normalized 0..1; UI sliders display 0..100. The default is
0.5. Values are clamped; non-finite/non-numeric values use the default. Partial
`setTuning` calls merge with current choices. Piecewise linear interpolation joins
these anchors (the midpoint returns literals to preserve exact defaults):

| Control | 0 | 0.5 (default) | 1 |
| --- | --- | --- | --- |
| Cut uncertain areas: curve low | .025 | .22 | .4 |
| Curve high | .975 | .88 | .8 |
| Curve mix | .25 | .85 | 1 |
| Edge softness: guided output radius | 2 px | 6 px | 12 px |
| Guided epsilon | .0002 | .001 | .008 |
| Steadiness: still-region time constant | 20 ms | 80 ms | 240 ms |

The alpha shader evaluates `mix(a, smoothstep(low, high, a), mix)` once after
upsampling. The separable seven-tap guided filter scales its tap spacing by
radius/6; its half-resolution targets and all other default operations stay the
same. Motion keeps the existing continuous .04–.18 confidence-change ramp toward
a 2 ms time constant. The filter also compares a grey copy of each frame (mask
size / 4) with the previous one. Where the picture is still, a large mask change
is the model changing its mind (a chair behind the shoulders, for example), so the
pixel joins the person over 25× and leaves over 3× the steadiness time constant.
Motion opens this gate within two cells, softened and held for about 50 ms. Tuning updates retain temporal history and immutable frames.

`setQuality('balanced'|'accurate')` updates the native provider's next request.
A resolution change rebuilds the matte filter when necessary, without restarting
the provider or clearing the frame ring. MediaPipe ignores this native-only
choice. `setView('composite'|'mask')` schedules a redraw, including paused clips.
Invalid enum values throw RangeError. Show mask is meaningful with Blur/Replace.

The existing v2 store merges new background fields with defaults for old state.
Advanced disclosure state is not saved; it starts closed. Source selection,
recorded media and blob URLs are never persisted. Camera is selected on launch.
Sample commands are invoked only after an explicit Sample selection. Rust uses
one fixed HTTPS base, no redirects, bounded reads, exact byte counts and SHA-256.
Verified clips and a validated manifest live in the app cache; files are verified
again before offline reuse. Camera and microphone are independent of downloads.
