use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{ForcedAvailability, ForcedKind};

use super::Composer;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::actions::OpenSwitcher;
use crate::views::chat_menu::confirm_leave;
use crate::views::shortcuts::{OpenSettings, ShowShortcuts};

const GROUP_KIND: &str = "group";
const WIDTH: f32 = 380.;
const ROW_HEIGHT: f32 = 32.;
const HEADER_HEIGHT: f32 = 28.;
const VISIBLE_ROWS: usize = 8;
const NAME_WIDTH: f32 = 128.;
pub const HEADER: &str = "Commands";
pub const NO_RESULTS: &str = "No results";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Presence(ForcedKind),
    ResetPresence,
    Goto,
    LeaveChat,
    Mute,
    Unmute,
    ScheduleMessage,
    Settings,
    Shortcuts,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlashContext {
    pub chat: bool,
    pub notes: bool,
    pub group: bool,
    pub muted: bool,
}

impl SlashCommand {
    pub const ALL: [SlashCommand; 14] = [
        SlashCommand::Presence(ForcedKind::Available),
        SlashCommand::Presence(ForcedKind::Away),
        SlashCommand::Presence(ForcedKind::BeRightBack),
        SlashCommand::Presence(ForcedKind::Busy),
        SlashCommand::Presence(ForcedKind::DoNotDisturb),
        SlashCommand::Goto,
        SlashCommand::LeaveChat,
        SlashCommand::Mute,
        SlashCommand::Presence(ForcedKind::Offline),
        SlashCommand::ResetPresence,
        SlashCommand::ScheduleMessage,
        SlashCommand::Settings,
        SlashCommand::Shortcuts,
        SlashCommand::Unmute,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SlashCommand::Presence(ForcedKind::Available) => "available",
            SlashCommand::Presence(ForcedKind::Away) => "away",
            SlashCommand::Presence(ForcedKind::BeRightBack) => "brb",
            SlashCommand::Presence(ForcedKind::Busy) => "busy",
            SlashCommand::Presence(ForcedKind::DoNotDisturb) => "dnd",
            SlashCommand::Presence(ForcedKind::Offline) => "offline",
            SlashCommand::ResetPresence => "reset",
            SlashCommand::Goto => "goto",
            SlashCommand::LeaveChat => "leavechat",
            SlashCommand::Mute => "mute",
            SlashCommand::Unmute => "unmute",
            SlashCommand::ScheduleMessage => "schedulemessage",
            SlashCommand::Settings => "settings",
            SlashCommand::Shortcuts => "shortcuts",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Presence(ForcedKind::Available) => "Set your status to available",
            SlashCommand::Presence(ForcedKind::Away) => "Set your status to away",
            SlashCommand::Presence(ForcedKind::BeRightBack) => "Set your status to be right back",
            SlashCommand::Presence(ForcedKind::Busy) => "Set your status to busy",
            SlashCommand::Presence(ForcedKind::DoNotDisturb) => "Set your status to do not disturb",
            SlashCommand::Presence(ForcedKind::Offline) => "Set your status to offline",
            SlashCommand::ResetPresence => "Reset your presence status",
            SlashCommand::Goto => "Go right to a chat or channel",
            SlashCommand::LeaveChat => "Leave the chat",
            SlashCommand::Mute => "Mute the chat",
            SlashCommand::Unmute => "Unmute the chat",
            SlashCommand::ScheduleMessage => "Schedule a message",
            SlashCommand::Settings => "Open Settings",
            SlashCommand::Shortcuts => "Open keyboard shortcuts",
        }
    }

    pub fn confirmation(self) -> Option<&'static str> {
        match self {
            SlashCommand::Presence(ForcedKind::Available) => {
                Some("Your status is set to available.")
            }
            SlashCommand::Presence(ForcedKind::Away) => Some("Your status is set to away."),
            SlashCommand::Presence(ForcedKind::BeRightBack) => {
                Some("Your status is set to be right back.")
            }
            SlashCommand::Presence(ForcedKind::Busy) => Some("Your status is set to busy."),
            SlashCommand::Presence(ForcedKind::DoNotDisturb) => {
                Some("Your status is set to do not disturb.")
            }
            SlashCommand::Presence(ForcedKind::Offline) => Some("Your status is set to offline."),
            SlashCommand::ResetPresence => Some("Your presence status is reset."),
            _ => None,
        }
    }

    fn available_in(self, context: SlashContext) -> bool {
        match self {
            SlashCommand::LeaveChat => context.chat && context.group,
            SlashCommand::Mute => context.chat && !context.notes && !context.muted,
            SlashCommand::Unmute => context.chat && !context.notes && context.muted,
            SlashCommand::ScheduleMessage => context.chat && !context.notes,
            _ => true,
        }
    }
}

fn commands() -> impl Iterator<Item = SlashCommand> {
    SlashCommand::ALL.iter().copied()
}

/// The command query while the draft starts with "/" and the caret is still in its first word.
pub fn active_query(value: &str, cursor: usize) -> Option<(Range<usize>, String)> {
    if !value.starts_with('/') || cursor == 0 || cursor > value.len() {
        return None;
    }
    let typed = value.get(1..cursor)?;
    if typed.chars().any(char::is_whitespace) {
        return None;
    }
    let word_end = value
        .char_indices()
        .find(|(_, character)| character.is_whitespace())
        .map_or(value.len(), |(index, _)| index);
    if cursor < word_end {
        return None;
    }
    Some((0..cursor, typed.to_lowercase()))
}

pub fn matching(query: &str, context: SlashContext) -> Vec<SlashCommand> {
    let available: Vec<SlashCommand> = commands()
        .filter(|command| command.available_in(context))
        .collect();
    let starts = available
        .iter()
        .filter(|command| command.name().starts_with(query));
    let contains = available
        .iter()
        .filter(|command| !command.name().starts_with(query) && command.name().contains(query));
    let described = available.iter().filter(|command| {
        !command.name().contains(query) && command.description().to_lowercase().contains(query)
    });
    starts.chain(contains).chain(described).copied().collect()
}

pub struct SlashPopup {
    pub range: Range<usize>,
    pub query: String,
    pub commands: Vec<SlashCommand>,
    pub highlighted: usize,
    scroll: ScrollHandle,
}

impl SlashPopup {
    pub fn new(range: Range<usize>, query: String, commands: Vec<SlashCommand>) -> Self {
        SlashPopup {
            range,
            query,
            commands,
            highlighted: 0,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn move_highlight(&mut self, delta: isize) {
        let count = self.commands.len() as isize;
        if count > 0 {
            self.highlighted = (self.highlighted as isize + delta).rem_euclid(count) as usize;
            self.scroll.scroll_to_item(self.highlighted);
        }
    }

    pub fn selected(&self) -> Option<SlashCommand> {
        self.commands.get(self.highlighted).copied()
    }
}

fn row(index: usize, command: SlashCommand, selected: bool) -> Stateful<Div> {
    h_flex()
        .id(ElementId::Name(format!("slash-row-{index}").into()))
        .flex_none()
        .h(px(ROW_HEIGHT))
        .px(px(8.))
        .gap(px(8.))
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .when(selected, |row| row.bg(theme::popup_hover()))
        .hover(|row| row.bg(theme::popup_hover()))
        .child(
            div()
                .w(px(NAME_WIDTH))
                .flex_none()
                .truncate()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(format!("/{}", command.name())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child(command.description()),
        )
}

fn label(text: &'static str) -> Div {
    h_flex()
        .h(px(HEADER_HEIGHT))
        .px(px(8.))
        .items_center()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(text)
}

pub fn render(
    popup: &SlashPopup,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_pick = Rc::new(on_pick);
    let rows = popup.commands.iter().enumerate().map(|(index, command)| {
        let on_pick = on_pick.clone();
        row(index, *command, index == popup.highlighted)
            .on_click(move |_, window, cx| on_pick(index, window, cx))
    });
    let visible = popup.commands.len().clamp(1, VISIBLE_ROWS) as f32;
    v_flex()
        .id("slash-popup")
        .absolute()
        .bottom(relative(1.))
        .left_0()
        .mb(px(6.))
        .w(px(WIDTH))
        .p(px(4.))
        .rounded(px(10.))
        .flyout_surface()
        .occlude()
        .child(label(HEADER))
        .when(popup.commands.is_empty(), |panel| {
            panel.child(
                h_flex()
                    .h(px(ROW_HEIGHT))
                    .flex_none()
                    .px(px(8.))
                    .items_center()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(NO_RESULTS),
            )
        })
        .when(!popup.commands.is_empty(), |panel| {
            panel.child(
                v_flex()
                    .id("slash-rows")
                    .max_h(px(ROW_HEIGHT * visible))
                    .overflow_y_scrollbar()
                    .track_scroll(&popup.scroll)
                    .children(rows),
            )
        })
        .into_any_element()
}

impl Composer {
    fn slash_context(&self, cx: &App) -> SlashContext {
        let Some(conversation_id) = self.conversation_id.as_deref() else {
            return SlashContext::default();
        };
        let state = self.app.read(cx);
        let Some(chat) = state
            .sidebar
            .chats
            .iter()
            .find(|chat| chat.id == conversation_id)
        else {
            return SlashContext::default();
        };
        SlashContext {
            chat: true,
            notes: teams_core::is_notes_chat(conversation_id),
            group: chat.kind == GROUP_KIND,
            muted: chat.muted,
        }
    }

    pub(super) fn update_slash(&mut self, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let found = active_query(&state.value(), state.cursor()).filter(|_| self.editing.is_none());
        let Some((range, query)) = found else {
            self.slash_popup = None;
            self.slash_dismissed = false;
            return;
        };
        if self.slash_dismissed {
            return;
        }
        let unchanged = self
            .slash_popup
            .as_ref()
            .is_some_and(|popup| popup.query == query && popup.range == range);
        if unchanged {
            return;
        }
        let commands = matching(&query, self.slash_context(cx));
        self.slash_popup = Some(SlashPopup::new(range, query, commands));
    }

    pub(super) fn accept_slash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.slash_popup.take() else {
            return;
        };
        let Some(command) = popup.selected() else {
            return;
        };
        self.replace_text(popup.range, "", 0, window, cx);
        self.run_slash_command(command, window, cx);
        cx.notify();
    }

    fn run_slash_command(
        &mut self,
        command: SlashCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat_id = self.conversation_id.clone().unwrap_or_default();
        match command {
            SlashCommand::Presence(kind) => self.set_presence(
                Some(ForcedAvailability {
                    kind,
                    expires_at: None,
                }),
                command,
                cx,
            ),
            SlashCommand::ResetPresence => self.set_presence(None, command, cx),
            SlashCommand::Goto => window.dispatch_action(Box::new(OpenSwitcher), cx),
            SlashCommand::Settings => window.dispatch_action(Box::new(OpenSettings), cx),
            SlashCommand::Shortcuts => window.dispatch_action(Box::new(ShowShortcuts), cx),
            SlashCommand::ScheduleMessage => self.open_schedule_menu(cx),
            SlashCommand::Mute | SlashCommand::Unmute => {
                let muted = command == SlashCommand::Mute;
                self.app
                    .update(cx, |state, cx| state.set_chat_muted(&chat_id, muted, cx));
            }
            SlashCommand::LeaveChat => {
                let Some(chat) = self
                    .app
                    .read(cx)
                    .sidebar
                    .chats
                    .iter()
                    .find(|chat| chat.id == chat_id)
                    .cloned()
                else {
                    return;
                };
                let title = crate::app_state::chat_title(&chat);
                confirm_leave(&self.app, &chat_id, &title, chat.members.len(), window, cx);
            }
        }
    }

    fn set_presence(
        &mut self,
        forced: Option<ForcedAvailability>,
        command: SlashCommand,
        cx: &mut Context<Self>,
    ) {
        self.app.update(cx, |state, cx| {
            state.set_own_availability(forced, cx);
            if let Some(text) = command.confirmation() {
                state.raise_notice(text.to_owned(), None, cx);
            }
        });
    }

    pub(super) fn render_slash_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popup = self.slash_popup.as_ref()?;
        let composer = cx.entity().downgrade();
        Some(render(popup, move |index, window, cx| {
            composer
                .update(cx, |this, cx| {
                    if let Some(popup) = this.slash_popup.as_mut() {
                        popup.highlighted = index;
                    }
                    this.accept_slash(window, cx);
                })
                .ok();
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{SlashContext, active_query, matching};

    fn chat() -> SlashContext {
        SlashContext {
            chat: true,
            ..SlashContext::default()
        }
    }

    #[test]
    fn a_slash_at_the_start_opens_the_list() {
        assert_eq!(active_query("/", 1), Some((0..1, String::new())));
        assert_eq!(active_query("/Aw", 3), Some((0..3, "aw".to_owned())));
    }

    #[test]
    fn a_slash_later_in_the_text_or_after_a_space_does_not_open() {
        assert_eq!(active_query("hi /away", 8), None);
        assert_eq!(active_query("/away now", 9), None);
        assert_eq!(active_query("/away", 0), None);
        assert_eq!(active_query("/away", 2), None);
    }

    #[test]
    fn the_list_is_sorted_by_name_like_teams() {
        let names: Vec<&str> = matching("", chat())
            .iter()
            .map(|command| command.name())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert!(names.contains(&"mute") && !names.contains(&"unmute"));
        assert!(!names.contains(&"leavechat"));
    }

    #[test]
    fn name_prefix_matches_come_before_description_matches() {
        let found = matching("b", chat());
        assert_eq!(found[0].name(), "brb");
        assert_eq!(found[1].name(), "busy");
        assert!(found.iter().any(|command| command.name() == "available"));
        assert_eq!(matching("status", chat())[0].name(), "available");
    }

    #[test]
    fn commands_follow_the_conversation() {
        let group_muted = SlashContext {
            chat: true,
            group: true,
            muted: true,
            notes: false,
        };
        let names: Vec<&str> = matching("", group_muted)
            .iter()
            .map(|command| command.name())
            .collect();
        assert!(
            names.contains(&"leavechat") && names.contains(&"unmute") && !names.contains(&"mute")
        );
        let notes = SlashContext {
            chat: true,
            notes: true,
            ..SlashContext::default()
        };
        let names: Vec<&str> = matching("", notes)
            .iter()
            .map(|command| command.name())
            .collect();
        assert!(!names.contains(&"mute") && !names.contains(&"schedulemessage"));
        let channel: Vec<&str> = matching("", SlashContext::default())
            .iter()
            .map(|command| command.name())
            .collect();
        assert!(channel.contains(&"goto") && !channel.contains(&"leavechat"));
    }
}
