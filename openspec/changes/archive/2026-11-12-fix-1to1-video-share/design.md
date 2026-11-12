# Design

## Context

Siehe `proposal.md`. Call-Report `20261112T130854Z-1to1`: Escalation-Answer 14 Zeilen auf Offer mit 13 (`mid 3-0` extra), Same-peer-Kamera-Answer ~5 s nach 40-s-Timeout, Kamera-Open mit `CameraFormat: Failed to Fufill`.

`from_teams_offer` entfaltet `a=x-multi-stream` auf mid 3 in Browser-Mids `3-0` und `3`. `to_teams_answer` ruft `to_teams_offer` mit dem Remote-`OfferPlan` auf und schreibt beide Mids als eigene m-lines.

## Goals / Non-Goals

**Goals:**

- Signalisierte Answers: eine m-line pro Teams-Offer-Zeile, Fold zurück zu `x-multi-stream`.
- Same-peer: kurze Grace nach Timeout für passende Kamera-Answers.
- Kamera-Open: Property-Fehler → Fallback-Kette.

**Non-Goals:**

- Command-Queue, Portal-Lifetime, PipeWire-BGRA (erst nach Retest).
- Share-Grace ohne Answer (Server-`408`).

## Decisions

### Fold nur in der signalisierten Answer

Browser-Mids bleiben intern eindeutig (`3-0`, `3`). Beim Schreiben der Teams-Answer: Gallery-Carrier wie bei Offers — Nicht-Carrier-Mids auslassen, Carrier mit `x-multi-stream`-Count. `to_teams_answer` kann denselben `to_teams_offer`-Pfad nutzen, sobald der Plan die Faltung korrekt steuert; Regressionstest gegen Offer mit 13 Zeilen → Answer 13 Zeilen, kein `a=mid:3-0`.

### Late Answer: kurze Grace, nicht Timeout verlängern

Nach `SAME_PEER_ANSWER_LIMIT` Rollback + `undo_change` wie bisher. Zusätzlich die letzte Same-peer-Offer-Identität (reply_tags / signaled / change) für wenige Sekunden behalten. Trifft eine passende Answer ein: `apply_answer`, Kamera wieder an, `recvonly` auf der Gegenseite als Send-Erfolg. Kein generelles Anheben auf 60+ s (Share hatte keine Answer).

### Kamera: Liste optional

`open_picked_format` darf bei `compatible_camera_formats`-Fehler sofort in die bestehende `open_camera`-Fallback-Kette (Closest NV12/YUYV, AbsoluteHighest) gehen, statt den Fehler nach oben zu reichen.

## Risks / Trade-offs

- [Late Answer nach Undo startet Capture zweimal] → `start_camera` idempotent bzw. nur UI/SDP anwenden, wenn Capture schon läuft; sonst Capture neu.
- [Falsche Answer in der Grace] → nur `answers_offer` mit gespeicherten reply_tags.
- [Fold ändert Conference-Offers] → Test mit Multi-Stream-Offer und bestehendem Fold-Test für Browser-Mids.

## Migration Plan

Kein Daten-Migration. Deploy: Fork-Build / lokale Installation. Rollback: vorheriger Tag.
