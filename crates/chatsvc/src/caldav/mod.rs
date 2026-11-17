//! CalDAV client: discovery, calendar-query, PUT/DELETE, RSVP.

mod client;
mod discovery;
mod xml;
#[cfg(test)]
mod xml_tests;

pub use client::{
    CalDavClient, CalDavEvent, ListedCalendar, href_from_object_id, new_event_uid, object_id_for,
    partstat_for_reply,
};
pub use discovery::{DiscoverResult, discover};
