## 1. Formatwahl 30 fps

- [x] 1.1 `pick_format` lässt MJPEG zu; Raw (NV12>YUYV) nur bei gleicher Höhe/30 fps bevorzugen; C930e-Fixture-Test
- [x] 1.2 Open: Exact-Kandidaten (MJPG HD → Raw 30 fps), nach Open `frame_rate() >= 30` prüfen; AbsoluteHighest nur Last Resort

## 2. MJPEG-Decode

- [x] 2.1 `zune-jpeg` → RGBA → `abgr_to_i420` mit Buffer-Reuse in `start_camera` / `direct_i420`
- [x] 2.2 Budget-Test (ignored, release): 1080p Decode+Convert < 20 ms

## 3. UVC + Telemetrie

- [x] 3.1 Nach Open: `exposure_dynamic_framerate=0`, `power_line_frequency=50` best effort
- [x] 3.2 Timeline: gewähltes Format; Stats: Capture-fps / Decode-ms wo angebunden

## 4. Verify

- [x] 4.1 `cargo test -p calling --lib camera` + ignored Hardware-Open
- [x] 4.2 Release `0.3.0-joe.4` bauen und nach `/usr/local/bin/rusty-teams` installieren
