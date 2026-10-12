const LAYOUT_META_KEY: &str = "call_layout";
const HIDE_NO_VIDEO_META_KEY: &str = "call_hide_no_video";
const CHAT_BUBBLES_META_KEY: &str = "call_chat_bubbles";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallLayout {
    #[default]
    Gallery,
    Speaker,
    Content,
}

impl CallLayout {
    pub fn label(self) -> &'static str {
        match self {
            CallLayout::Gallery => "Gallery",
            CallLayout::Speaker => "Speaker",
            CallLayout::Content => "Focus on content",
        }
    }

    fn meta(self) -> &'static str {
        match self {
            CallLayout::Gallery => "gallery",
            CallLayout::Speaker => "speaker",
            CallLayout::Content => "content",
        }
    }

    fn from_meta(value: &str) -> CallLayout {
        match value {
            "speaker" => CallLayout::Speaker,
            "content" => CallLayout::Content,
            _ => CallLayout::Gallery,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewSettings {
    pub layout: CallLayout,
    pub layout_before_content: CallLayout,
    pub hide_no_video: bool,
    pub chat_bubbles: bool,
    pub hide_me: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        ViewSettings {
            layout: CallLayout::Gallery,
            layout_before_content: CallLayout::Gallery,
            hide_no_video: false,
            chat_bubbles: true,
            hide_me: false,
        }
    }
}

impl ViewSettings {
    pub fn load(store: &store::Store) -> ViewSettings {
        let flag = |key: &str, default: bool| match store.meta(key).ok().flatten().as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => default,
        };
        let layout = store
            .meta(LAYOUT_META_KEY)
            .ok()
            .flatten()
            .map_or(CallLayout::Gallery, |value| CallLayout::from_meta(&value));
        ViewSettings {
            layout,
            hide_no_video: flag(HIDE_NO_VIDEO_META_KEY, false),
            chat_bubbles: flag(CHAT_BUBBLES_META_KEY, true),
            ..ViewSettings::default()
        }
    }

    pub fn save(&self, store: &store::Store) {
        let _ = store.set_meta(LAYOUT_META_KEY, self.layout.meta());
        let _ = store.set_meta(
            HIDE_NO_VIDEO_META_KEY,
            if self.hide_no_video { "1" } else { "0" },
        );
        let _ = store.set_meta(
            CHAT_BUBBLES_META_KEY,
            if self.chat_bubbles { "1" } else { "0" },
        );
    }

    pub fn choose(&mut self, layout: CallLayout) {
        if layout == CallLayout::Content && self.layout != CallLayout::Content {
            self.layout_before_content = self.layout;
        }
        self.layout = layout;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_gallery_with_bubbles_on() {
        let store = store::Store::open_in_memory().unwrap();
        let settings = ViewSettings::load(&store);
        assert_eq!(settings.layout, CallLayout::Gallery);
        assert!(settings.chat_bubbles);
        assert!(!settings.hide_no_video);
        assert!(!settings.hide_me);
    }

    #[test]
    fn choices_survive_a_restart() {
        let store = store::Store::open_in_memory().unwrap();
        let mut settings = ViewSettings::load(&store);
        settings.choose(CallLayout::Speaker);
        settings.hide_no_video = true;
        settings.chat_bubbles = false;
        settings.hide_me = true;
        settings.save(&store);
        let loaded = ViewSettings::load(&store);
        assert_eq!(loaded.layout, CallLayout::Speaker);
        assert!(loaded.hide_no_video);
        assert!(!loaded.chat_bubbles);
        assert!(!loaded.hide_me);
    }

    #[test]
    fn content_remembers_the_layout_to_fall_back_to() {
        let mut settings = ViewSettings::default();
        settings.choose(CallLayout::Speaker);
        settings.choose(CallLayout::Content);
        assert_eq!(settings.layout_before_content, CallLayout::Speaker);
        settings.choose(CallLayout::Content);
        assert_eq!(settings.layout_before_content, CallLayout::Speaker);
    }
}
