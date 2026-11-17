//! Shared iCalendar parse / expand / write helpers for CalDAV and ICS subscriptions.

mod expand;
mod join;
mod parse;
mod timezone;
mod write;

pub use expand::expand_events;
pub use join::teams_join_url;
pub use parse::{ParsedEvent, parse_calendar};
pub use timezone::resolve_tzid;
pub use write::{CalDavEventDraft, build_vevent, exclude_occurrence_from_ics};
