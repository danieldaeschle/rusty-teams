## ADDED Requirements

### Requirement: Answered H.264 level covers the peer frame limits

When an H.264 answer carries both `max-fs` and `max-mbps`, the client SHALL raise `profile-level-id` to the lowest H.264 Table A-1 level that covers both limits. The client MUST NOT lower a level. An answer without both limits SHALL keep its level. A level-3.0 answer with 720p limits SHALL become level 3.1. A 1080p answer that already required level 4.0 SHALL stay at level 4.0.

#### Scenario: Level 3.0 with 720p limits

- **WHEN** the answer contains `profile-level-id` ending in level 3.0 and `max-fs=3600` with `max-mbps=108000`
- **THEN** the sent level is 3.1
- **AND** the `max-fs` and `max-mbps` values are unchanged

#### Scenario: Missing limits stay put

- **WHEN** the answer has a profile level and no `max-fs` or no `max-mbps`
- **THEN** the profile level is unchanged
