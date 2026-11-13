---
status: accepted
date: 2026-11-12
---

# ADR 0006: The mic path matches Teams in the browser

## Context

The remote party has to hear the same microphone treatment as Teams in the browser. A single software chain for every mode does not. Auto and Low in browser Teams use the platform audio device module. Music mode, Off, and computer-sound sharing do not.

Options were: one software pipeline for every mode, or the same split the browser uses.

## Decision

On Linux, Auto and Low send through the Pulse device track. On macOS, Auto and Low send through the CoreAudio device track. Music mode, Off, and computer sound use the software chain on both.

Hangup tears the audio graph down without hanging the call task. On macOS, camera preview and send use the system camera.

## Consequences

- Mic bugs are split by mode. An Auto/Low fix does not belong in the software chain, and the reverse is also true.
- A new OS needs the same split against that OS's device module, not a new policy.
