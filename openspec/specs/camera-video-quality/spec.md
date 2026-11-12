# camera-video-quality Specification

## Purpose

Steuert, in welcher Aufloesung die eigene Kamera erfasst und dem Call angeboten wird, und welche Qualitaet fuer Stage-Kamera-Empfang angefordert wird — waehlbar als Auto, 720p oder 1080p.

## Requirements

### Requirement: Camera capture prefers the selected quality ceiling

The client SHALL capture the local camera at the highest supported widescreen format at least 30 fps that does not exceed the active quality ceiling (720p or 1080p). Auto SHALL use a 1080p ceiling when the camera can deliver it, otherwise 720p. The client SHALL NOT offer or use a 4K camera quality mode.

#### Scenario: Camera supports 1080p with Auto

- **WHEN** video quality is Auto and the selected camera exposes a suitable 1920×1080 (or equivalent 16:9) format at ≥30 fps
- **THEN** capture uses that 1080p-class format (not forced down to 720p before encoding)

#### Scenario: Camera only supports 720p

- **WHEN** video quality is Auto or 1080p and the camera cannot provide a suitable 1080p format
- **THEN** capture falls back to a suitable 720p format without failing camera start solely for that reason

#### Scenario: User locks 720p

- **WHEN** video quality is set to 720p and a higher camera format is available
- **THEN** capture stays at or below 720p

### Requirement: Send capabilities match capture ceiling

After the local camera size for sending is known, the client SHALL advertise max video send capabilities whose width and height reflect that capture size (up to the active quality ceiling), so the meeting media path can request layers up to that size.

#### Scenario: 1080p capture is advertised

- **WHEN** the outbound camera frame size is 1920×1080
- **THEN** the send-capabilities update reports max width and height of at least 1920 on the largest side consistent with that frame

### Requirement: Stage receive requests use 1080p-class limits when allowed

For non-screen camera tiles shown as stage detail, the client SHALL request receive limits at 1080p-class macroblock and throughput budgets when the active quality ceiling is Auto or 1080p. When the ceiling is 720p, stage requests SHALL remain at 720p-class budgets. Small gallery tiles and screen-share receive limits SHALL keep their existing budgets.

#### Scenario: Stage at Auto or 1080p

- **WHEN** the user is subscribed to a participant camera on the stage and video quality is Auto or 1080p
- **THEN** the stage receive request uses 1080p-class `max-fs` / `max-mbps` budgets (same order of magnitude as today’s screen-share camera request, not the previous 720p stage budget)

#### Scenario: Stage locked to 720p

- **WHEN** video quality is 720p and the user is subscribed to a stage camera tile
- **THEN** the stage receive request uses 720p-class budgets

### Requirement: Video quality setting is user-visible and persistent

The product SHALL expose a video quality choice of Auto, 720p, and 1080p in the call/device camera controls, persist the choice across app restarts, and apply it to new captures and new stage subscriptions. Changing the setting while a call is active SHALL take effect for subsequent capture restarts and subsequent stage request updates without requiring an app relaunch.

#### Scenario: Persist across restart

- **WHEN** the user selects 1080p and later restarts the app
- **THEN** video quality remains 1080p

#### Scenario: Default is Auto

- **WHEN** no prior video quality choice exists
- **THEN** the effective choice is Auto
