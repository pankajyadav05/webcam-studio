# Parked bitmap presentation trial

These two files are an archive, not a runnable independent engine. They relied
on sibling modules from ui/engine. Do not import them into the shipped app.

Preparing immutable ImageBitmap camera snapshots and transferring an
OffscreenCanvas WebGL surface passed Chrome's orientation/privacy/context tests,
but the real WKWebView stalled during a mode/visibility transition. The native
engine now retains owned HTML canvas captures and uses JPEG binary IPC, avoiding
this extra ImageBitmap dependency. The ordinary canvas presentation can have
9–12ms tails on this Mac; that missed performance gate is documented rather than
shipping the unstable optimization.
