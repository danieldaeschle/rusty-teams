---
status: proposed
date: 2026-11-16
superseded-by:
---

# ADR 0018: Regional format is separate from the app language

## Context

Dates and times followed what `sys_locale` returns. That is the preferred UI language on every OS, not the regional format: `CFLocaleCopyPreferredLanguages` on macOS, `GetUserPreferredUILanguages` on Windows, `LC_MESSAGES` and `LANG` on Linux. An English UI in Austria (`en_US@rg=atzzzz` on macOS) got a 12-hour clock, `11/16/2026` and Sunday as the first day. Several labels also used chrono's English `%a` and `%b` in a German UI.

Teams desktop formats with the OS regional format and has no account setting for it. ADR 0016 put a first-day override in the calendar settings, but date pickers and other weeks did not read it.

The options were: keep following the language, read the OS regional format only, or read it and let the user override it in the app.

## Decision

- The regional format is read from the OS: `AppleLocale` with its `rg` override, `AppleICUForce24HourTime`/`AppleICUForce12HourTime` and `AppleFirstWeekday` on macOS, `GetUserDefaultLocaleName` and `GetLocaleInfoEx` on Windows, `LC_ALL`/`LC_TIME`/`LANG` on Linux.
- Settings > Appearance > Regional format offers region (follow system or one of a curated list), time format (automatic, 24-hour, 12-hour) and first day of week (automatic, Monday, Sunday, Saturday). The choice is local, as Teams has no account setting for it.
- The region decides the patterns and decimal separator. The app language decides the words. Weekday and month names always come from the locale catalog.
- First day of week: the choice, else the mailbox language as ADR 0016 says, else the region. This replaces the first-day bullet of ADR 0016 and its calendar setting; work hours and time scale stay as 0016 decided. A first day saved in the calendar settings carries over once.

## Consequences

- One first-day value drives the calendar grid, mini month and date pickers.
- The region list only holds regions whose patterns `format::date_style` knows. A new region needs its patterns there first.
- Custom date patterns from the OS (macOS `AppleICUDateFormatStrings`, Windows short date) are not read.
