# Spec Delta

## Purpose

Steuert die Sprache der App-Oberfläche: Ableitung aus der System-Locale beim ersten Start, dauerhafte Nutzerwahl und Anwendung auf alle produktseitigen UI-Texte in Englisch und Deutsch.

## ADDED Requirements

### Requirement: Unterstützte UI-Sprachen

Die App MUST genau die UI-Sprachen Englisch und Deutsch anbieten. Deutsch MUST Deutschland-Orthografie im sichtbaren Produkttext verwenden.

#### Scenario: Sprachenwahl zeigt DE und EN

- **WHEN** der Nutzer die UI-Spracheinstellung öffnet
- **THEN** stehen Englisch und Deutsch zur Auswahl
- **AND** keine weitere UI-Sprache ist wählbar

### Requirement: Erster Start folgt der OS-Locale

Ohne gespeicherte UI-Spracheinstellung MUST die App die System-Locale auswerten: beginnt die Locale mit `de`, MUST die UI Deutsch sein; sonst MUST sie Englisch sein.

#### Scenario: System Deutsch

- **GIVEN** es existiert noch keine gespeicherte UI-Sprache
- **WHEN** die App startet und die System-Locale mit `de` beginnt
- **THEN** zeigt die App ihre Oberflächentexte auf Deutsch

#### Scenario: System nicht Deutsch

- **GIVEN** es existiert noch keine gespeicherte UI-Sprache
- **WHEN** die App startet und die System-Locale nicht mit `de` beginnt
- **THEN** zeigt die App ihre Oberflächentexte auf Englisch

### Requirement: Nutzerwahl überschreibt und persistiert

Der Nutzer MUST die UI-Sprache in den Einstellungen auf Englisch oder Deutsch setzen können. Die Wahl MUST gespeichert werden und MUST bei späteren Starts gelten, unabhängig von der dann aktuellen System-Locale.

#### Scenario: Explizit Deutsch speichern

- **WHEN** der Nutzer die UI-Sprache auf Deutsch setzt
- **THEN** zeigt die App Deutsch
- **AND** nach Neustart der App bleibt Deutsch aktiv, auch wenn die System-Locale nicht mit `de` beginnt

#### Scenario: Explizit Englisch speichern

- **WHEN** der Nutzer die UI-Sprache auf Englisch setzt
- **THEN** zeigt die App Englisch
- **AND** nach Neustart bleibt Englisch aktiv, auch wenn die System-Locale mit `de` beginnt

### Requirement: Umschalten ohne Neustart

Ändert der Nutzer die UI-Sprache, MUST die sichtbare Oberfläche die neue Sprache annehmen, ohne dass die App beendet werden muss.

#### Scenario: Wechsel in den Einstellungen

- **WHEN** der Nutzer in einer laufenden Sitzung von Englisch auf Deutsch wechselt
- **THEN** erscheinen Navigation, Einstellungen und Dialoge in Deutsch
- **AND** die Sitzung bleibt geöffnet

### Requirement: Gesamte produktseitige UI ist lokalisiert

Alle von der App selbst stammenden Oberflächentexte MUST in der aktiven UI-Sprache erscheinen. Das umfasst Navigation, Einstellungen, Dialoge, Menüs, Tooltips, Tastaturkürzel-Beschriftungen, Toasts, Tray-Menü, Agent-UI und Nutzerhinweise aus Gesprächen und Fehlersituationen.

#### Scenario: Settings und Chrome

- **WHEN** die UI-Sprache Deutsch ist
- **THEN** sind Einstellungsnavigation, Rail und häufige Dialogaktionen (etwa Abbrechen und Speichern) auf Deutsch

#### Scenario: Call-Hinweis

- **WHEN** die UI-Sprache Deutsch ist und ein lokal erzeugter Call-Hinweis angezeigt wird
- **THEN** erscheint der Hinweistext auf Deutsch

#### Scenario: Tray

- **WHEN** die UI-Sprache Deutsch ist und der Nutzer das Tray-Menü öffnet
- **THEN** sind die Menüeinträge auf Deutsch

### Requirement: Fehlende Übersetzung fällt auf Englisch zurück

Fehlt für einen UI-Text die Übersetzung in der aktiven Sprache, MUST die App den englischen Text zeigen statt eines leeren Labels oder eines rohen Schlüssels.

#### Scenario: Fehlender Key unter Deutsch

- **WHEN** die UI-Sprache Deutsch ist und für einen UI-Text keine deutsche Übersetzung vorliegt
- **THEN** zeigt die App den englischen Text für diesen Eintrag
- **AND** die übrigen lokalisierten Texte bleiben auf Deutsch

### Requirement: Fremder Content bleibt unverändert

Server- und Nutzerinhalte MUST die UI-Locale nicht ersetzen. Message-Translation, Captions-Spoken-Language, Adaptive-Card-Titel vom Server und Gerätenamen vom Betriebssystem bleiben eigene Pfade.

#### Scenario: Chat-Nachricht

- **WHEN** die UI-Sprache Deutsch ist und eine englische Chat-Nachricht angezeigt wird
- **THEN** bleibt der Nachrichteninhalt Englisch
- **AND** die umgebenden App-Steuerelemente sind Deutsch

#### Scenario: Message-Translation unverändert

- **WHEN** der Nutzer Message-Translation nutzt
- **THEN** gilt weiterhin die bestehende Translation-Einstellung
- **AND** die UI-Locale ersetzt diese Einstellung nicht
