# Spec Delta

## ADDED Requirements

### Requirement: Write-with identity brands assistant surfaces

When «Write with» selects a chat profile, the composer rewrite control and the chat-header assistant island SHALL present that profile's name and a non-Copilot icon instead of Microsoft Copilot branding. When «Write with» is empty, those surfaces SHALL keep Microsoft Copilot branding.

#### Scenario: Local writer rebrands composer and island

- **WHEN** the user has chosen a chat profile under Settings > Agents > Write with
- **THEN** the composer rewrite tooltip, working and failure copy name that profile
- **AND** the chat-header island title, open/close labels, placeholder and working/failure copy name that profile
- **AND** neither surface shows the Copilot colour logo as its identity mark

#### Scenario: Empty write-with keeps Copilot branding

- **WHEN** Write with is left empty (Microsoft Copilot)
- **THEN** composer rewrite and the chat-header island keep Copilot labels and the Copilot logo where they already appear today

### Requirement: Chat island asks through the write-with profile

When «Write with» selects a chat profile, questions from the chat-header assistant island SHALL be answered by that profile. The prompt SHALL include up to the newest twenty non-deleted messages of the open chat as plain text plus the user's question. The island SHALL NOT call Microsoft Copilot for those asks. Empty Write with keeps the Microsoft ask path.

#### Scenario: Local ask uses recent chat messages

- **WHEN** Write with is a chat profile and the user asks a question in the chat-header island
- **THEN** the answer comes from that profile
- **AND** the model input includes at most the newest twenty non-deleted messages of that chat plus the question
- **AND** Microsoft Copilot is not invoked for that ask

#### Scenario: Empty write-with still uses Microsoft ask

- **WHEN** Write with is empty and the user asks in the chat-header island
- **THEN** the ask follows the existing Microsoft Copilot path

### Requirement: Local writer surfaces work without a Teams engine

When «Write with» selects a chat profile, the composer rewrite control and the chat-header assistant island SHALL remain available for an open chat even if the Teams sync engine is absent, so users without a Microsoft Copilot licence can still use the local writer.

#### Scenario: Island opens without engine when a local writer is set

- **WHEN** Write with is a chat profile, a chat is open, and no Teams sync engine is connected
- **THEN** the user can still open the chat-header assistant island and send a question to that profile
