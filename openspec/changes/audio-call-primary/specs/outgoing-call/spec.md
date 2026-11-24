# Spec Delta

## Purpose

The one-click call from a chat is a normal audio call. Video is a separate choice, matching Teams.

## ADDED Requirements

### Requirement: One-to-one primary call is audio

In a one-to-one chat the primary call control MUST start an audio call and MUST show the phone. It MUST NOT start the camera. Video call MUST remain available from that control's menu and MUST start with the camera.

#### Scenario: Primary click

- **WHEN** the user activates the primary call control in a one-to-one chat
- **THEN** an audio call starts and the camera stays off

#### Scenario: Video from the menu

- **WHEN** the user chooses video call from that control's menu
- **THEN** a video call starts with the camera

### Requirement: Group Meet now stays video

A group Meet now control MUST keep starting a video meeting. This change MUST NOT turn it into an audio call.

#### Scenario: Meet now

- **WHEN** the user activates Meet now in a group chat
- **THEN** the meeting starts with video
