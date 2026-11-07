# WKWebView layout repro (the engine of the macOS and iOS apps)

Safari, Chrome and WebKitGTK did not show the 2.19.x "last row of a grid painted over the next folder" bug; an embedded
`WKWebView` did. This runs the built UI (`dist/`) in one, against a fake server, and screenshots the window's own pixels.

On the Mac (mabu: `ssh mabu-wg`, Xcode needed), with `dist/` from `npx vite build` copied next to these files:

```sh
xcrun swiftc -O wkrepro.swift -o wkrepro
python3 mock.py 9300 dist &                     # /api/me, /api/libraries, a 124-video folder tree, slow webp thumbnails
./wkrepro http://localhost:9300/library/1 /tmp/shot 0.75 9      # url, out prefix, page zoom, scroll steps
# optional 5th / 6th arguments: a CSS file / a JS file injected before the page runs (to try a fix without a build)
python3 blank_bands.py /tmp/shot_*.png          # > ~100 = a row is missing from where it is laid out
```

Zoom 0.75 on a 1536 pt screen gives a 2048 px viewport at DPR 2: six grid columns, so a folder of 7 videos has one card
alone in its second row (the one that was mispainted). A window shows on the Mac for about a minute.

`screencapture` over ssh cannot capture windows (no screen-recording permission, it returns the wallpaper only):
the harness uses `CGWindowListCreateImage` on its own window instead.
