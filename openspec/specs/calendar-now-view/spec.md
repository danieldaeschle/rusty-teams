# calendar-now-view Specification

## Purpose

Hält die Tages- und Wochenansicht des Kalenders an der lokalen Uhrzeit: sichtbarer Viewport, wandernde Now-Linie und Nachführen, bis der Nutzer manuell scrollt.

## Requirements

### Requirement: Open and Today scroll to current time

When the day or week calendar grid is shown or the user chooses Today while that grid is in view, the app SHALL scroll vertically so the current local time is visible in the viewport. The app MUST NOT use a fixed morning hour such as 08:00 as the only scroll target.

#### Scenario: Open calendar on day or week

- **WHEN** the user opens the calendar in day or week span
- **THEN** the hour grid scrolls so the current local time is visible

#### Scenario: Today button

- **WHEN** the user presses Today while day or week span shows today
- **THEN** the grid scrolls again so the current local time is visible

#### Scenario: Month span unchanged

- **WHEN** the calendar is in month span
- **THEN** this requirement does not change month layout or scroll behaviour

### Requirement: Now line tracks the clock

While the day or week grid is visible, the now indicator SHALL move with local wall-clock time without waiting for a calendar data refresh.

#### Scenario: Minute advances

- **WHEN** the local clock advances while the day or week grid is open
- **THEN** the now line and marker update to the new time

### Requirement: Follow clock until manual scroll

While the day or week grid shows today and the user has not manually scrolled the hour grid, the app SHALL keep the viewport aligned to the current local time as the clock advances. After the user manually scrolls the hour grid, the app MUST keep that scroll position until Today or opening the day/week grid again requests alignment.

#### Scenario: Auto-follow without user scroll

- **WHEN** today is visible in day or week span and the user has not scrolled the hour grid
- **THEN** as time passes the viewport stays aligned so current time remains visible

#### Scenario: Manual scroll stops follow

- **WHEN** the user manually scrolls the hour grid vertically
- **THEN** the app stops auto-aligning to the clock
- **AND** keeps the user's scroll position until Today or a fresh open of the day/week grid
