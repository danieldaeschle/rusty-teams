use super::sidebar::SidebarTab;
use crate::app_state::Selection;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LastViews {
    chat: Option<String>,
    channel: Option<String>,
}

impl LastViews {
    pub fn remember(&mut self, selection: &Selection) {
        match selection {
            Selection::Chat(chat_id) => self.chat = Some(chat_id.clone()),
            Selection::Channel(channel_id) => self.channel = Some(channel_id.clone()),
        }
    }

    pub fn restore(
        &self,
        tab: SidebarTab,
        current: Option<&Selection>,
        first_chat_id: Option<&str>,
        first_channel_id: Option<&str>,
    ) -> Option<Selection> {
        match (tab, current) {
            (SidebarTab::Chats, Some(Selection::Chat(_))) => None,
            (SidebarTab::Channels, Some(Selection::Channel(_))) => None,
            (SidebarTab::Chats, _) => self
                .chat
                .clone()
                .or_else(|| first_chat_id.map(str::to_owned))
                .map(Selection::Chat),
            (SidebarTab::Channels, _) => self
                .channel
                .clone()
                .or_else(|| first_channel_id.map(str::to_owned))
                .map(Selection::Channel),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_app_gets_back_its_own_conversation() {
        let mut last = LastViews::default();
        let chat = Selection::Chat("19:chat".to_owned());
        let channel = Selection::Channel("19:channel".to_owned());
        last.remember(&chat);
        last.remember(&channel);
        assert_eq!(
            last.restore(SidebarTab::Chats, Some(&channel), Some("19:first"), None),
            Some(chat.clone())
        );
        assert_eq!(
            last.restore(SidebarTab::Channels, Some(&chat), None, Some("19:general")),
            Some(channel.clone())
        );
        assert_eq!(
            last.restore(SidebarTab::Chats, Some(&chat), None, None),
            None
        );
        assert_eq!(
            last.restore(SidebarTab::Calendar, Some(&channel), None, None),
            None
        );
    }

    #[test]
    fn each_app_falls_back_to_its_first_conversation() {
        let last = LastViews::default();
        let channel = Selection::Channel("19:channel".to_owned());
        assert_eq!(
            last.restore(SidebarTab::Chats, Some(&channel), Some("19:first"), None),
            Some(Selection::Chat("19:first".to_owned()))
        );
        let chat = Selection::Chat("19:chat".to_owned());
        assert_eq!(
            last.restore(SidebarTab::Channels, Some(&chat), None, Some("19:general")),
            Some(Selection::Channel("19:general".to_owned()))
        );
        assert_eq!(
            last.restore(SidebarTab::Channels, Some(&chat), None, None),
            None
        );
    }
}
