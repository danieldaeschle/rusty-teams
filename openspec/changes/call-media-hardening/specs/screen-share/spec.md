## ADDED Requirements

### Requirement: Screen frames are reduced before I420 conversion

A shared screen or window frame SHALL be scaled with libyuv so its macroblock count is at most the 1080p send budget before it is converted to I420. A buffer shorter than the claimed stride and height MUST NOT be converted.

#### Scenario: A wide frame fits the send budget

- **WHEN** a 2560 by 1440 frame is captured for screen share
- **THEN** the I420 frame that is delivered has at most 8160 macroblocks

#### Scenario: A short buffer is dropped

- **WHEN** the capturer reports a size whose byte length exceeds the buffer
- **THEN** no frame is delivered
