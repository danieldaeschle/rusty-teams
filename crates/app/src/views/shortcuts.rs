use gpui_kit::*;

use super::composer::{
    EditLink, KEY_CONTEXT as COMPOSER_CONTEXT, ScheduleSend, ToggleBold, ToggleCode, ToggleItalic,
    ToggleStrike, ToggleSubscript, ToggleSuperscript, ToggleUnderline,
};
use super::conversation::ReplyToHovered;
use super::shell::{NewChat, OpenSwitcher, ToggleCallMute};

const TEXT_FIELD_CONTEXT: &str = "Input";
const OUTSIDE_COMPOSER_CONTEXT: &str = "!Composer";

actions!(
    teams,
    [
        OpenActivity,
        OpenChatsTab,
        OpenChannelsTab,
        OpenSaved,
        PreviousConversation,
        NextConversation,
        AcceptCall,
        DeclineCall,
        HangUpCall,
        ToggleCallCamera,
        ToggleCallShare,
        ShowShortcuts
    ]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    General,
    Navigation,
    Composer,
    Calls,
}

impl Group {
    pub const ALL: [Group; 4] = [
        Group::General,
        Group::Navigation,
        Group::Composer,
        Group::Calls,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::General => "General",
            Group::Navigation => "Navigation",
            Group::Composer => "Messages and composer",
            Group::Calls => "Calls",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    OutsideComposer,
    GlobalOverTextFields,
    Composer,
}

pub struct Shortcut {
    pub group: Group,
    pub label: &'static str,
    pub keys: &'static str,
    pub mac_keys: Option<&'static str>,
    pub scope: Scope,
    bind: fn(&str, Option<&str>) -> KeyBinding,
}

macro_rules! shortcut {
    ($group:ident, $scope:ident, $label:expr, $keys:expr, $mac_keys:expr, $action:expr) => {
        Shortcut {
            group: Group::$group,
            label: $label,
            keys: $keys,
            mac_keys: $mac_keys,
            scope: Scope::$scope,
            bind: |keys, context| KeyBinding::new(keys, $action, context),
        }
    };
}

#[rustfmt::skip]
pub const SHORTCUTS: &[Shortcut] = &[
    shortcut!(General, OutsideComposer, "Search and switch chats", "ctrl-k", Some("cmd-k"), OpenSwitcher),
    shortcut!(General, Global, "New chat", "ctrl-n", Some("cmd-n"), NewChat),
    shortcut!(General, GlobalOverTextFields, "Keyboard shortcuts", "ctrl-.", Some("cmd-."), ShowShortcuts),
    shortcut!(Navigation, GlobalOverTextFields, "Open Activity", "ctrl-1", Some("cmd-1"), OpenActivity),
    shortcut!(Navigation, GlobalOverTextFields, "Show Chats", "ctrl-2", Some("cmd-2"), OpenChatsTab),
    shortcut!(Navigation, GlobalOverTextFields, "Show Channels", "ctrl-3", Some("cmd-3"), OpenChannelsTab),
    shortcut!(Navigation, GlobalOverTextFields, "Open Saved", "ctrl-4", Some("cmd-4"), OpenSaved),
    shortcut!(Navigation, GlobalOverTextFields, "Previous chat or channel", "alt-up", None, PreviousConversation),
    shortcut!(Navigation, GlobalOverTextFields, "Next chat or channel", "alt-down", None, NextConversation),
    shortcut!(Composer, Global, "Reply to the message under the pointer", "alt-r", None, ReplyToHovered),
    shortcut!(Composer, Composer, "Bold", "ctrl-b", Some("cmd-b"), ToggleBold),
    shortcut!(Composer, Composer, "Italic", "ctrl-i", Some("cmd-i"), ToggleItalic),
    shortcut!(Composer, Composer, "Underline", "ctrl-u", Some("cmd-u"), ToggleUnderline),
    shortcut!(Composer, Composer, "Strikethrough", "ctrl-shift-x", None, ToggleStrike),
    shortcut!(Composer, Composer, "Superscript", "ctrl-shift-=", None, ToggleSuperscript),
    shortcut!(Composer, Composer, "Subscript", "ctrl-=", None, ToggleSubscript),
    shortcut!(Composer, Composer, "Code", "ctrl-shift-c", None, ToggleCode),
    shortcut!(Composer, Composer, "Insert link", "ctrl-k", Some("cmd-k"), EditLink),
    shortcut!(Composer, Composer, "Schedule send", "ctrl-shift-enter", None, ScheduleSend),
    shortcut!(Calls, GlobalOverTextFields, "Accept incoming call", "ctrl-shift-a", Some("cmd-shift-a"), AcceptCall),
    shortcut!(Calls, GlobalOverTextFields, "Decline incoming call", "ctrl-shift-d", Some("cmd-shift-d"), DeclineCall),
    shortcut!(Calls, GlobalOverTextFields, "Hang up or leave", "ctrl-shift-h", Some("cmd-shift-h"), HangUpCall),
    shortcut!(Calls, GlobalOverTextFields, "Mute or unmute", "ctrl-shift-m", None, ToggleCallMute),
    shortcut!(Calls, GlobalOverTextFields, "Turn camera on or off", "ctrl-shift-o", Some("cmd-shift-o"), ToggleCallCamera),
    shortcut!(Calls, GlobalOverTextFields, "Start or stop sharing the screen", "ctrl-shift-e", Some("cmd-shift-e"), ToggleCallShare),
];

impl Shortcut {
    pub fn keys_for_platform(&self) -> &'static str {
        match self.mac_keys {
            Some(mac_keys) if cfg!(target_os = "macos") => mac_keys,
            _ => self.keys,
        }
    }

    pub fn display_keys(&self) -> String {
        display_keys(self.keys_for_platform())
    }

    fn bindings(&self) -> Vec<KeyBinding> {
        let contexts: &[Option<&str>] = match self.scope {
            Scope::Global => &[None],
            Scope::OutsideComposer => &[Some(OUTSIDE_COMPOSER_CONTEXT)],
            Scope::GlobalOverTextFields => &[None, Some(TEXT_FIELD_CONTEXT)],
            Scope::Composer => &[Some(COMPOSER_CONTEXT)],
        };
        let mut keys = vec![self.keys];
        keys.extend(self.mac_keys.filter(|_| cfg!(target_os = "macos")));
        keys.iter()
            .flat_map(|keys| contexts.iter().map(|context| (self.bind)(keys, *context)))
            .collect()
    }
}

pub fn display_keys(keys: &str) -> String {
    keys.split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_owned(),
            "cmd" => "Cmd".to_owned(),
            "alt" => "Alt".to_owned(),
            "shift" => "Shift".to_owned(),
            "up" => "Up".to_owned(),
            "down" => "Down".to_owned(),
            "enter" => "Enter".to_owned(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

pub fn bind_scopes(cx: &mut App, scopes: &[Scope]) {
    cx.bind_keys(
        SHORTCUTS
            .iter()
            .filter(|shortcut| scopes.contains(&shortcut.scope))
            .flat_map(Shortcut::bindings),
    );
}

pub fn shortcuts_in(group: Group) -> impl Iterator<Item = &'static Shortcut> {
    SHORTCUTS
        .iter()
        .filter(move |shortcut| shortcut.group == group)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gpui_kit::{KeyContext, Keystroke, TestAppContext};

    use super::{Group, SHORTCUTS, Scope, Shortcut, display_keys, shortcuts_in};
    use crate::views::shell::bind_keys;

    fn bound_action(cx: &mut TestAppContext, keys: &str, contexts: &[&str]) -> Option<String> {
        let stack: Vec<KeyContext> = contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap())
            .collect();
        let keystroke = Keystroke::parse(keys).unwrap();
        cx.update(|cx| {
            let keymap = cx.key_bindings();
            let (bindings, _) = keymap.borrow().bindings_for_input(&[keystroke], &stack);
            bindings
                .first()
                .map(|binding| binding.action().name().to_owned())
        })
    }

    fn registered(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(bind_keys);
    }

    fn action_name(shortcut: &Shortcut) -> String {
        shortcut.bindings()[0].action().name().to_owned()
    }

    #[gpui_kit::test]
    fn every_listed_shortcut_resolves_to_its_action(cx: &mut TestAppContext) {
        registered(cx);
        for shortcut in SHORTCUTS {
            let contexts: &[&str] = match shortcut.scope {
                Scope::Global | Scope::OutsideComposer => &["AppShell"],
                Scope::GlobalOverTextFields => &["AppShell"],
                Scope::Composer => &["AppShell", "Composer"],
            };
            assert_eq!(
                bound_action(cx, shortcut.keys, contexts),
                Some(action_name(shortcut)),
                "{} ({})",
                shortcut.label,
                shortcut.keys
            );
        }
    }

    #[gpui_kit::test]
    fn global_shortcuts_survive_typing_in_the_composer(cx: &mut TestAppContext) {
        registered(cx);
        for shortcut in SHORTCUTS
            .iter()
            .filter(|shortcut| shortcut.scope == Scope::GlobalOverTextFields)
        {
            assert_eq!(
                bound_action(cx, shortcut.keys, &["AppShell", "Composer", "Input"]),
                Some(action_name(shortcut)),
                "{} ({})",
                shortcut.label,
                shortcut.keys
            );
        }
    }

    #[gpui_kit::test]
    fn composer_link_shortcut_still_beats_the_switcher_while_typing(cx: &mut TestAppContext) {
        registered(cx);
        let typing = ["AppShell", "Composer", "Input"];
        assert_eq!(
            bound_action(cx, "ctrl-k", &typing),
            Some(action_name(
                SHORTCUTS
                    .iter()
                    .find(|shortcut| shortcut.label == "Insert link")
                    .unwrap()
            ))
        );
        assert_eq!(
            bound_action(cx, "ctrl-k", &["AppShell"]),
            Some(action_name(
                SHORTCUTS
                    .iter()
                    .find(|shortcut| shortcut.label == "Search and switch chats")
                    .unwrap()
            ))
        );
    }

    #[test]
    fn no_two_shortcuts_share_keys_in_the_same_scope() {
        let mut seen = HashSet::new();
        for shortcut in SHORTCUTS {
            let scope = matches!(shortcut.scope, Scope::Composer);
            assert!(seen.insert((shortcut.keys, scope)), "{}", shortcut.keys);
        }
    }

    #[test]
    fn every_group_lists_shortcuts_and_keys_read_like_teams() {
        for group in Group::ALL {
            assert!(shortcuts_in(group).next().is_some(), "{group:?}");
        }
        assert_eq!(display_keys("ctrl-shift-a"), "Ctrl+Shift+A");
        assert_eq!(display_keys("alt-up"), "Alt+Up");
        assert_eq!(display_keys("ctrl-."), "Ctrl+.");
    }
}
