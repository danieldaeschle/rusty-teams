//! Re-exports calendar merge so existing `google_calendar::merge_*` paths keep compiling.

pub use crate::{merge_calendar_events, normalize_join_url};
