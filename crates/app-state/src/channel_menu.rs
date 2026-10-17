use std::future::Future;

use gpui_kit::*;

use crate::app_state::AppState;
use crate::sidebar_model::channel_shown;

pub const COPIED_TO_CLIPBOARD: &str = "Copied to clipboard";
pub const PINNED_CHANNEL_CANNOT_HIDE: &str = "Pinned channels can't be hidden.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMenuItem {
    Notifications,
    Pin,
    Unpin,
    Show,
    Hide,
    CopyLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamMenuItem {
    Show,
    Hide,
    CopyLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry<Item> {
    Item(Item),
    Separator,
}

impl ChannelMenuItem {
    pub fn label(self) -> &'static str {
        match self {
            ChannelMenuItem::Notifications => "Notifications",
            ChannelMenuItem::Pin => "Pin",
            ChannelMenuItem::Unpin => "Unpin",
            ChannelMenuItem::Show => "Show",
            ChannelMenuItem::Hide => "Hide",
            ChannelMenuItem::CopyLink => "Copy link",
        }
    }
}

impl TeamMenuItem {
    pub fn label(self) -> &'static str {
        match self {
            TeamMenuItem::Show => "Show",
            TeamMenuItem::Hide => "Hide",
            TeamMenuItem::CopyLink => "Copy link",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelMenuState {
    pub pinned: bool,
    pub shown: bool,
}

impl ChannelMenuState {
    pub fn disables(self, item: ChannelMenuItem) -> bool {
        item == ChannelMenuItem::Hide && self.pinned
    }
}

pub fn channel_menu_entries(state: ChannelMenuState) -> Vec<MenuEntry<ChannelMenuItem>> {
    vec![
        MenuEntry::Item(ChannelMenuItem::Notifications),
        MenuEntry::Item(if state.pinned {
            ChannelMenuItem::Unpin
        } else {
            ChannelMenuItem::Pin
        }),
        MenuEntry::Item(if state.shown {
            ChannelMenuItem::Hide
        } else {
            ChannelMenuItem::Show
        }),
        MenuEntry::Separator,
        MenuEntry::Item(ChannelMenuItem::CopyLink),
    ]
}

pub fn team_menu_entries(hidden: bool) -> Vec<MenuEntry<TeamMenuItem>> {
    vec![
        MenuEntry::Item(if hidden {
            TeamMenuItem::Show
        } else {
            TeamMenuItem::Hide
        }),
        MenuEntry::Separator,
        MenuEntry::Item(TeamMenuItem::CopyLink),
    ]
}

pub fn pin_failure(pinned: bool) -> &'static str {
    if pinned {
        "We couldn't pin the channel."
    } else {
        "We couldn't unpin the channel."
    }
}

pub fn channel_visibility_failure(shown: bool) -> &'static str {
    if shown {
        "We couldn't show the channel."
    } else {
        "We couldn't hide the channel."
    }
}

pub fn team_visibility_failure(shown: bool) -> &'static str {
    if shown {
        "We couldn't show the team."
    } else {
        "We couldn't hide the team."
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostPinDialog {
    pub title: &'static str,
    pub body: &'static str,
    pub confirm: &'static str,
}

pub fn post_pin_dialog(pin: bool) -> PostPinDialog {
    if pin {
        PostPinDialog {
            title: "Pin in channel",
            body: "The message will be pinned for everyone who views the channel.",
            confirm: "Pin",
        }
    } else {
        PostPinDialog {
            title: "Unpin this message?",
            body: "The message will be unpinned for everyone who views the channel.",
            confirm: "Unpin",
        }
    }
}

pub fn post_pin_notice(pin: bool) -> &'static str {
    if pin { "Post pinned" } else { "Post unpinned" }
}

pub fn post_pin_failure(pin: bool) -> &'static str {
    if pin {
        "We couldn't pin the post."
    } else {
        "We couldn't unpin the post."
    }
}

impl AppState {
    pub fn channel_menu_state(&self, channel_id: &str) -> ChannelMenuState {
        ChannelMenuState {
            pinned: self.channel_pinned(channel_id),
            shown: self
                .sidebar
                .teams
                .iter()
                .find(|team| team.channels.iter().any(|channel| channel.id == channel_id))
                .is_none_or(|team| channel_shown(team, channel_id)),
        }
    }

    pub fn channel_pinned(&self, channel_id: &str) -> bool {
        self.directory
            .pinned_channels
            .iter()
            .any(|id| id == channel_id)
    }

    pub fn set_channel_pinned(&mut self, channel_id: &str, pinned: bool, cx: &mut Context<Self>) {
        if self.channel_pinned(channel_id) == pinned {
            return;
        }
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let previous = self.directory.pinned_channels.clone();
        let mut changed = previous.clone();
        changed.retain(|id| id != channel_id);
        if pinned {
            changed.push(channel_id.to_owned());
        }
        self.apply_pinned_channels(changed, cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = channel_id.to_owned();
        self.run_channel_action(
            pin_failure(pinned),
            async move { engine.set_channel_pinned(&owned_id, pinned).await },
            move |state, cx| state.apply_pinned_channels(previous, cx),
            None,
            cx,
        );
    }

    fn apply_pinned_channels(&mut self, channel_ids: Vec<String>, cx: &mut Context<Self>) {
        let _ = self.store.replace_pinned_channels(&channel_ids);
        self.directory.pinned_channels = channel_ids;
        cx.emit(crate::app_state::AppEvent::Sidebar);
        cx.notify();
    }

    pub fn set_channel_shown(&mut self, channel_id: &str, shown: bool, cx: &mut Context<Self>) {
        if !shown && self.channel_pinned(channel_id) {
            self.raise_notice(PINNED_CHANNEL_CANNOT_HIDE.to_owned(), None, cx);
            return;
        }
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let _ = self.store.set_channel_hidden(channel_id, !shown);
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = channel_id.to_owned();
        let revert_id = owned_id.clone();
        self.run_channel_action(
            channel_visibility_failure(shown),
            async move { engine.set_channel_shown(&owned_id, shown).await },
            move |state, cx| {
                let _ = state.store.set_channel_hidden(&revert_id, shown);
                state.reload_sidebar(cx);
            },
            None,
            cx,
        );
    }

    pub fn set_team_shown(&mut self, team_id: &str, shown: bool, cx: &mut Context<Self>) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let _ = self.store.set_team_hidden(team_id, !shown);
        self.reload_sidebar(cx);
        let Some(engine) = engine_or_demo else {
            return;
        };
        let owned_id = team_id.to_owned();
        let revert_id = owned_id.clone();
        self.run_channel_action(
            team_visibility_failure(shown),
            async move { engine.set_team_shown(&owned_id, shown).await },
            move |state, cx| {
                let _ = state.store.set_team_hidden(&revert_id, shown);
                state.reload_sidebar(cx);
            },
            None,
            cx,
        );
    }

    pub fn copy_channel_link(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            if self.mode.demo {
                let link = teams_core::channel_link(&teams_core::ChannelAddress {
                    tenant_id: crate::demo::DEMO_TENANT_ID.to_owned(),
                    group_id: self.channel_team_id(channel_id).unwrap_or_default(),
                    channel_id: channel_id.to_owned(),
                    channel_name: self.channel_name(channel_id).unwrap_or_default(),
                    membership_type: None,
                });
                self.copy_link(link, cx);
            }
            return;
        };
        let owned_id = channel_id.to_owned();
        self.copy_link_from(async move { engine.channel_link(&owned_id).await }, cx);
    }

    pub fn copy_team_link(&mut self, team_id: &str, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            if self.mode.demo {
                let team_thread_id = self.store.team_thread_id(team_id).ok().flatten();
                let link = teams_core::team_link(
                    team_thread_id.as_deref().unwrap_or(team_id),
                    team_id,
                    crate::demo::DEMO_TENANT_ID,
                );
                self.copy_link(link, cx);
            }
            return;
        };
        let owned_id = team_id.to_owned();
        self.copy_link_from(async move { engine.team_link(&owned_id).await }, cx);
    }

    fn channel_team_id(&self, channel_id: &str) -> Option<String> {
        self.sidebar.teams.iter().find_map(|team| {
            team.channels
                .iter()
                .any(|channel| channel.id == channel_id)
                .then(|| team.team.id.clone())
        })
    }

    pub fn channel_name(&self, channel_id: &str) -> Option<String> {
        self.sidebar
            .teams
            .iter()
            .flat_map(|team| &team.channels)
            .find(|channel| channel.id == channel_id)
            .map(|channel| channel.name.clone())
    }

    fn copy_link(&mut self, link: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(link));
        self.raise_notice(COPIED_TO_CLIPBOARD.to_owned(), None, cx);
    }

    fn copy_link_from(
        &mut self,
        call: impl Future<Output = teams_core::Result<String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let receiver = crate::runtime::spawn(call);
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(link)) => Ok(link),
                Ok(Err(error)) => Err(crate::notice::short_error(&error)),
                Err(_) => Err("cancelled".to_owned()),
            };
            this.update(cx, |state, cx| match outcome {
                Ok(link) => state.copy_link(link, cx),
                Err(reason) => state.raise_notice(format!("Copy link failed: {reason}"), None, cx),
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn run_channel_action(
        &mut self,
        failure: &'static str,
        call: impl Future<Output = teams_core::Result<()>> + Send + 'static,
        revert: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        success: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        let receiver = crate::runtime::spawn(call);
        cx.spawn(async move |this, cx| {
            let failed = !matches!(receiver.await, Ok(Ok(())));
            this.update(cx, |state, cx| {
                if failed {
                    revert(state, cx);
                    state.raise_notice(failure.to_owned(), None, cx);
                } else if let Some(text) = success {
                    state.raise_notice(text.to_owned(), None, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelMenuItem, ChannelMenuState, MenuEntry, TeamMenuItem, channel_menu_entries,
        channel_visibility_failure, pin_failure, post_pin_dialog, post_pin_failure,
        post_pin_notice, team_menu_entries, team_visibility_failure,
    };

    fn labels<Item: Copy>(
        entries: &[MenuEntry<Item>],
        label: impl Fn(Item) -> &'static str,
    ) -> Vec<&'static str> {
        entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item(item) => label(*item),
                MenuEntry::Separator => "-",
            })
            .collect()
    }

    #[test]
    fn channel_menu_follows_teams_order() {
        let shown = channel_menu_entries(ChannelMenuState {
            pinned: false,
            shown: true,
        });
        assert_eq!(
            labels(&shown, ChannelMenuItem::label),
            ["Notifications", "Pin", "Hide", "-", "Copy link"]
        );
        let hidden_pinned = channel_menu_entries(ChannelMenuState {
            pinned: true,
            shown: false,
        });
        assert_eq!(
            labels(&hidden_pinned, ChannelMenuItem::label),
            ["Notifications", "Unpin", "Show", "-", "Copy link"]
        );
    }

    #[test]
    fn pinned_channels_cannot_be_hidden() {
        let pinned = ChannelMenuState {
            pinned: true,
            shown: true,
        };
        assert!(pinned.disables(ChannelMenuItem::Hide));
        assert!(!pinned.disables(ChannelMenuItem::Unpin));
        let unpinned = ChannelMenuState {
            pinned: false,
            shown: true,
        };
        assert!(!unpinned.disables(ChannelMenuItem::Hide));
    }

    #[test]
    fn team_menu_switches_between_show_and_hide() {
        assert_eq!(
            labels(&team_menu_entries(false), TeamMenuItem::label),
            ["Hide", "-", "Copy link"]
        );
        assert_eq!(
            labels(&team_menu_entries(true), TeamMenuItem::label),
            ["Show", "-", "Copy link"]
        );
    }

    #[test]
    fn post_pin_dialogs_and_notices_use_teams_strings() {
        let unpin = post_pin_dialog(false);
        assert_eq!(unpin.title, "Unpin this message?");
        assert_eq!(
            unpin.body,
            "The message will be unpinned for everyone who views the channel."
        );
        assert_eq!(unpin.confirm, "Unpin");
        let pin = post_pin_dialog(true);
        assert_eq!(pin.title, "Pin in channel");
        assert_eq!(pin.confirm, "Pin");
        assert_eq!(post_pin_notice(true), "Post pinned");
        assert_eq!(post_pin_notice(false), "Post unpinned");
        assert_eq!(post_pin_failure(true), "We couldn't pin the post.");
        assert_eq!(post_pin_failure(false), "We couldn't unpin the post.");
    }

    #[test]
    fn failures_use_teams_strings() {
        assert_eq!(pin_failure(true), "We couldn't pin the channel.");
        assert_eq!(pin_failure(false), "We couldn't unpin the channel.");
        assert_eq!(
            channel_visibility_failure(false),
            "We couldn't hide the channel."
        );
        assert_eq!(team_visibility_failure(true), "We couldn't show the team.");
    }
}
