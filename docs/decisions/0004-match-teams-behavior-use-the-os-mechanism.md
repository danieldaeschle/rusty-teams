---
status: accepted
date: 2026-11-12
---

# ADR 0004: Match Teams behavior, use the OS mechanism

## Context

Windows is where Teams desktop behavior is easiest to copy, because WebView2, window lists, and the audio stack are the ones Teams itself uses. Linux and macOS cannot call those APIs. Copying the Windows mechanism anyway produces empty share menus, a silent mic, or a sign-in window that never appears.

Options were: Windows-only, a lowest-common-denominator subset, or the same user-visible behavior with the mechanism that OS actually allows.

## Decision

What the user sees matches Teams, and matches Windows where Windows already matches Teams. The mechanism may be the native one: WebView2 on Windows, managed Chrome elsewhere, ScreenCast or ScreenCaptureKit for sharing, Pulse or CoreAudio for the mic.

A missing platform service is shown. It is not replaced with a dead button or a fake source list.

## Consequences

- A feature is not done on Linux or macOS just because the Windows code path compiled.
- ADRs 0003, 0005, and 0006 are applications of this rule, not exceptions to it.
- `GUARDRAILS.md` still wins when a change would diverge from Teams for other people in the tenant.
