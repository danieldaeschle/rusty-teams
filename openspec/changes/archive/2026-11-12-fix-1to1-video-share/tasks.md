# Tasks

## 1. Escalation-Answer ohne Extra-m-line

- [x] 1.1 `to_teams_answer` / Offer-Plan: Gallery-Faltung (`3-0` + `3`) als eine signalisierte m-line mit `x-multi-stream` zurückschreiben; Verify: Unit-Test Offer 13 Zeilen → Answer 13 Zeilen, kein `a=mid:3-0`
- [x] 1.2 Bestehenden Fold-Test für Browser-Mids grün lassen; Verify: `cargo test -p calling --lib sdp`

## 2. Same-peer Late Answer

- [x] 2.1 Nach Same-peer-Timeout kurze Grace mit reply_tags/change behalten und passende Answer noch anwenden (`recvonly` = Send ok); Verify: Unit- oder Drive-naher Test / dokumentierter Timeline-Check
- [x] 2.2 Verify: `cargo test -p calling --lib` Filter renegotiation/call wo vorhanden

## 3. Kamera-Open Fallback

- [x] 3.1 `open_picked_format` / `open_camera`: Format-Property-Fehler → Fallback-Kette statt fatal; Verify: `cargo test -p calling --lib camera`

## 4. Integration

- [x] 4.1 `cargo test -p calling --lib` grün (441 passed; `sdp_golden` 5/5; ignored Kamera-Hardware-Open ok)
- [x] 4.2 Automatisierter Kamera-Open-Smoke grün; voller Hyprland-1:1 Call-Report bleibt Operator-Retest nach Install — Queue/PipeWire nur bei noch rotem Report
