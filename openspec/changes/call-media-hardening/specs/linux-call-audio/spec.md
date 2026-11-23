## ADDED Requirements

### Requirement: Only Low uses the classical noise-suppression track

On the device track, Low SHALL publish the track with classical noise suppression. Auto, Off, and music mode SHALL publish the track without that stage. A failed software-path start MUST NOT restore the classical noise-suppression track for Off or music mode.

#### Scenario: Off does not restore noise suppression

- **WHEN** noise suppression is Off or music mode is on and the software path cannot start
- **THEN** the restored device track is the track without classical noise suppression

### Requirement: A named microphone matches the listed name

A named input on the software path SHALL match the name from the current device list, ignoring ASCII case. A stored endpoint id that is not that name MUST NOT match by substring. An unmatched name SHALL fail the capture with an error or the existing notice and MUST NOT silently capture a different microphone.

#### Scenario: Endpoint id is not a capture name

- **WHEN** the software path is asked for a named input whose key is an endpoint id and whose listed name is absent from the capture devices
- **THEN** capture fails with an error or notice

### Requirement: A device switch counts only when audio starts

Selecting an input or output SHALL be reported as applied only when the device is selected and recording or playout has started.

#### Scenario: Start fails

- **WHEN** the device index is accepted and recording or playout does not start
- **THEN** the selection is reported as not applied
