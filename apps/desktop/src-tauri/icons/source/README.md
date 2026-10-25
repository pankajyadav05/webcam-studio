# Webcam Studio identity

A person in frame: the four viewfinder corners represent framing; the central aperture
contains a person cut out of the orange. The mark uses the existing #e7783f accent and
one neutral, #202124. In the toolbar the frame inherits the appearance's foreground.

- `mark.svg`: 24-unit master, also embedded inline in `ui/index.html`.
- `app-icon.svg`: 1024 px transparent canvas, centred 824 px rounded-square body,
  185.4 px corner radius, soft 16 px / 18 px shadow. The mark is intentionally flat.
- `Credits.html`: the standard macOS About panel's formatted credits and repository link.

Render `app-icon.svg` at 1024 × 1024 in headless Chrome with a transparent page and
`omitBackground: true`, saving `source/app-icon-1024.png`, then run from the repository root:

```sh
cargo tauri icon apps/desktop/src-tauri/icons/source/app-icon-1024.png --output apps/desktop/src-tauri/icons
iconutil -c iconset apps/desktop/src-tauri/icons/icon.icns -o /tmp/WebcamStudio.iconset
```

Inspect the extracted 16, 32, 128 and 512 px assets at actual size. The macOS bundle
explicitly uses `icon.icns`; Tauri also uses that icon for the Dock during development.
The About panel uses the 256 px PNG, independently of the smaller default window icon.

## Window appearance verification

`WEBCAM_STUDIO_APPEARANCE=light` or `dark` forces only this app process's appearance.
Launch the built bundle's executable with that environment variable; unset it for live
system appearance. It never changes the Mac's appearance setting. The opaque webview
preserves the preview pixels and avoids macOSPrivateApi. Tauri's native visual-effect
API requires window transparency, so the toolbar and sidebar use unified opaque surfaces.

Normal window bounds are cached on move/resize and saved atomically on close/quit to
`window-bounds.json` in the Tauri application configuration directory. Restore clamps
to a connected monitor's work area. Fullscreen and zoomed bounds do not replace the
normal bounds. This uses existing serde dependencies, with no new plugin.
