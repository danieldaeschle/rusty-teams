# Design

## Context

Siehe `proposal.md` für den Anlass. Die App-UI in `crates/app` (Views, Settings, Notify/Tray) und Nutzerhinweise in `crates/calling` (u. a. `CallUpdate::Notice` mit englischen Literalen) sind hardcodiert Englisch. Message-Translation und Caption-Sprachen existieren separat und bleiben unberührt.

`vendor/gpui-component` nutzt bereits `rust_i18n` (`locales/ui.yml`, `set_locale` / `locale` in `vendor/gpui-component/src/lib.rs`). Appearance-Persistenz liegt in `crates/app-base/src/appearance.rs` (Store-Meta). Notification-Settings analog in `crates/app-state/src/notify/settings.rs`.

## Goals / Non-Goals

**Goals:**

- App-eigener Locale-Katalog `en` + `de` (Deutschland-Orthografie im Produktkatalog) mit Fallback `en`.
- Ableitung erster Start aus OS-Locale (`de*` → `de`, sonst `en`), Override persistiert, Live-Umschalten über Laufzeit-Lookup.
- Stabile Hinweis-IDs aus Calling/App-State; Anzeigetext erst an der UI-Grenze.
- Phasenweise Abdeckung bis Endzustand C (volle produktseitige UI), ohne halb leere Labels dank Fallback.

**Non-Goals:**

- `de-CH`-Katalog, weitere Sprachen in diesem Change.
- Server-Content, Wire-Protokoll, Captions spoken language, Message-Translation.
- Upstream-Riesen-PR; Lieferung auf Forgejo-`origin`. Vendor-Locale-YAML der Komponenten-Bibliothek nicht mit App-Strings mischen.

## Decisions

### `rust-i18n` für die App, eigener Katalog

Dieselbe Bibliothek wie `gpui-component`, eigener Katalog (z. B. unter `crates/app` oder kleinem Locale-Modul), Fallback `en`. Beim Start und bei Preference-Änderung: App-Locale setzen und `gpui_kit`/`gpui-component` `set_locale` mitziehen, damit Widget-Strings (DatePicker, Dialog-Defaults) nicht Englisch bleiben während die App Deutsch zeigt.

Alternative verworfen: Mozilla Fluent / gettext — zusätzlicher Stack ohne Nutzen, solange der Vendor schon `rust-i18n` hat.

### Preference: Auto vs fest

Gespeicherter Wert: `auto` | `en` | `de`. Default ohne Meta-Eintrag = `auto` (entspricht Spec «keine gespeicherte Einstellung»). Persistenz analog Appearance (Store-Meta). UI-Einstellung unter Appearance oder General; Anzeige der effektiven Sprache bei `auto` klar kennzeichnen.

Effektive Locale:

```
meta fehlt / auto --> OS beginnt mit "de"? --> de : en
meta en|de       --> genau dieser Wert
```

OS-Locale: Plattform-übliche Abfrage (Linux `LANG`/`LC_ALL`/`LC_MESSAGES`, Windows/macOS über vorhandene System-APIs im Repo bzw. schlanke Ergänzung). Nur Prefix `de` zählt; `de_CH`, `de_DE`, `de-AT` → Deutsch (Produkttexte dennoch Deutschland-Orthografie).

### Strings beim Render, nicht als `'static` Bootstrap

Sichtbare Labels über Lookup zur Render-Zeit (`t!` oder Wrapper), damit Locale-Wechsel ohne Neustart wirkt. Const-`&'static str` die einmal bei Init gebacken werden, nur wo unvermeidbar und dann nach Locale-Wechsel invalidieren/neu binden (Tray-Menü neu syncen).

### Call-Notices: IDs statt englischer Sätze

`CallUpdate::Notice(String)` trägt heute fertigen Englisch-Text. Für Lokalisierung:

```
calling / app-state                 UI (crates/app)
------------------                  ---------------
NoticeId + optionale Args    -->    t!("call.notice....", args)
format!("Camera...: {error}") -->   Key + error als Arg (error intern Englisch ok)
```

Stabile IDs für bekannte Notices (`ShareRefused`, `OnHold`, …). Dynamische Fehler: Template-Key + Argument. Tests in `calling` prüfen IDs/Enums, nicht übersetzte Sätze. UI-Tests können Locale setzen und DE/EN-Text asserten.

Alternative verworfen: `t!` direkt in `calling` — zieht Locale-Katalog in ein Nicht-UI-Crate und koppelt Unit-Tests an Übersetzungen.

### Lieferphasen (ein Change, Endzustand C)

1. Gerüst: Katalog, Preference, OS-Default, Settings-Steuerung, `set_locale`, Fallback.
2. Chrome: Rail, Settings-Nav/Texte, häufige Dialoge (Cancel/Save/…).
3. Kernflächen: Chat, Calls, Calendar, Composer, Shortcuts-Beschriftungen.
4. Notices/Tray/Agents/Fenster-Titel/Rest-Fehlertexte; Calling auf Notice-IDs.

Jede Phase darf mit Fallback `en` shippen; Spec «gesamte UI» ist erfüllt, wenn Phase 4 fertig und Verify grün ist.

### OpenSpec vs Produktorthografie

OpenSpec-Artefakte und Agent-Texte: Schweizer Schreibweise (kein Eszett). Produktkatalog `de`: Deutschland-Deutsch inklusive Eszett. Kein Widerspruch — unterschiedliche Textsorten.

## Risks / Trade-offs

- [Riesen-Diff / Layout-Tests mit englischen Strings] → Phasen; Tests locale setzen oder auf Keys/Rollen prüfen; `layout_guard` und sichtbare String-Asserts gezielt anpassen.
- [Tray/OS-Menüs cachen Labels] → nach Locale-Wechsel `sync_tray` und vergleichbare Rebuilds.
- [Längere DE-Strings sprengen Layout] → kritische Buttons in DE prüfen; wo nötig kürzere Formulierungen, nicht Schrift skalieren.
- [Notice-ID-Migration lässt Rest-Literale] → Inventar der `CallUpdate::Notice`-Pfade; CI oder Test «keine neuen englischen Notice-Literale» optional später.
- [gpui-component ohne `de` in `locales/ui.yml`] → fehlende Widget-Keys fallen auf `en`; bei Bedarf `de`-Zeilen im Vendor nachziehen oder akzeptieren bis Folge-Change.

## Migration Plan

Kein Server-State. Bestehende Installationen ohne Meta-Key verhalten sich wie `auto` (OS). Rollback: Preference entfernen / auf `en` setzen; Code-Revert auf dem Fork-Branch. Lieferung nur nach Forgejo-`origin`.
