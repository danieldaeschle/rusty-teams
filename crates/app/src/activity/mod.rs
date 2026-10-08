mod feed;
mod reactions;
mod service;

pub use feed::{Actor, Entry, Filter, Kind};
pub use service::ActivityCenter;

use crate::notify::Preview;

const IMAGE_LABEL: &str = "Image";

fn preview_label(preview: &Preview) -> String {
    match preview {
        Preview::Text(text) => text.clone(),
        Preview::Image => IMAGE_LABEL.to_owned(),
    }
}
