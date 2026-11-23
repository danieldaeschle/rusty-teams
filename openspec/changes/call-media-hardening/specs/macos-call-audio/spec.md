## ADDED Requirements

### Requirement: Only Low uses the classical noise-suppression track

On the device track, Low SHALL publish the track with classical noise suppression. Auto, Off, and music mode SHALL publish the track without that stage. A failed software-path start MUST NOT restore the classical noise-suppression track for Off or music mode.

#### Scenario: Music mode does not restore noise suppression

- **WHEN** music mode is on and the software path cannot start
- **THEN** the restored device track is the track without classical noise suppression

### Requirement: A named microphone matches the listed name

A named input on the software path SHALL match the name from the current device list, ignoring ASCII case. A stored endpoint id MUST NOT match a capture device by substring. An unmatched name SHALL fail the capture with an error or the existing notice.

#### Scenario: Overlapping names do not match

- **WHEN** the requested name is a shorter string contained in a longer capture-device name
- **THEN** that longer device is not selected

### Requirement: A device switch counts only when audio starts

Selecting an input or output SHALL be reported as applied only when the device is selected and recording or playout has started.

#### Scenario: Start fails

- **WHEN** the device index is accepted and recording or playout does not start
- **THEN** the selection is reported as not applied
