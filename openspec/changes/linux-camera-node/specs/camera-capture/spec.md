# Spec Delta

## ADDED Requirements

### Requirement: Linux lists capture nodes only

On Linux the camera list MUST drop a node only when that node is known not to capture video. A node whose capture capability is unknown MUST stay listed. A metadata node that shares the capture node's name MUST NOT appear. One physical camera MUST appear once. The default camera MUST be a capture node when one is known. Windows and macOS lists MUST stay unchanged.

#### Scenario: C930e metadata node is hidden

- **WHEN** the camera offers a capture node and a metadata node under the same name
- **THEN** the list shows that name once and selecting it opens the capture node

#### Scenario: Default is the capture node

- **WHEN** the user starts the camera with the default choice and a metadata twin exists
- **THEN** the app opens the capture node, not the metadata node

#### Scenario: Unknown capability stays listed

- **WHEN** the capability query cannot open the node
- **THEN** the node stays in the camera list

#### Scenario: Capture node still fails

- **WHEN** the capture node itself cannot be opened
- **THEN** the camera stays off and the app shows the error
