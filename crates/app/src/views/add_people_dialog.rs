use chatsvc::{HistoryShare, MAX_HISTORY_DAYS};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::MemberRecord;

use super::new_chat::{NewChatDraft, Pick};
use crate::app_state::AppState;
use crate::theme;

const DIALOG_WIDTH: f32 = 480.;
const TITLE: &str = "Add people";
const FIELD_RADIUS: f32 = 6.;
const DAYS_FIELD_WIDTH: f32 = 64.;
const DEFAULT_DAYS: &str = "1";
const SECTION_GAP: f32 = 12.;
const OPTION_GAP: f32 = 8.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryChoice {
    None,
    Days,
    All,
}

/// Teams accepts 1 to 999 days and disables Add for anything else.
pub fn history_share(choice: HistoryChoice, days_text: &str) -> Option<HistoryShare> {
    match choice {
        HistoryChoice::None => Some(HistoryShare::None),
        HistoryChoice::All => Some(HistoryShare::All),
        HistoryChoice::Days => {
            let digits: String = days_text.chars().filter(char::is_ascii_digit).collect();
            let days: u32 = digits.parse().ok()?;
            (1..=MAX_HISTORY_DAYS)
                .contains(&days)
                .then_some(HistoryShare::Days(days))
        }
    }
}

/// Everyone of the new group chat after adding to a 1:1: the other member first, then the picks.
pub fn group_chat_people(
    members: &[MemberRecord],
    my_user_id: Option<&str>,
    picks: &[Pick],
) -> Vec<MemberRecord> {
    members
        .iter()
        .filter(|member| member.user_id.is_some() && member.user_id.as_deref() != my_user_id)
        .cloned()
        .chain(picks.iter().map(member_of))
        .collect()
}

fn member_of(pick: &Pick) -> MemberRecord {
    MemberRecord {
        user_id: Some(pick.user_id.clone()),
        display_name: pick.name.clone(),
    }
}

struct AddPeopleForm {
    app: Entity<AppState>,
    chat_id: String,
    one_on_one: bool,
    people: Entity<NewChatDraft>,
    days: Entity<InputState>,
    choice: HistoryChoice,
    _subscriptions: Vec<Subscription>,
}

impl AddPeopleForm {
    fn new(
        app: Entity<AppState>,
        chat_id: String,
        one_on_one: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let members: Vec<String> = app
            .read(cx)
            .sidebar
            .chats
            .iter()
            .find(|chat| chat.id == chat_id)
            .map(|chat| {
                chat.members
                    .iter()
                    .filter_map(|member| member.user_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let people_app = app.clone();
        let people = cx.new(|cx| {
            NewChatDraft::new(people_app, window, cx)
                .compact()
                .excluding(members)
        });
        let days = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(DEFAULT_DAYS, window, cx);
            input
        });
        let subscriptions = vec![
            cx.observe(&people, |_, _, cx| cx.notify()),
            cx.subscribe_in(&days, window, |form, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Focus) {
                    form.choice = HistoryChoice::Days;
                }
                cx.notify();
            }),
        ];
        AddPeopleForm {
            app,
            chat_id,
            one_on_one,
            people,
            days,
            choice: HistoryChoice::None,
            _subscriptions: subscriptions,
        }
    }

    fn share(&self, cx: &App) -> Option<HistoryShare> {
        history_share(self.choice, &self.days.read(cx).value())
    }

    fn can_add(&self, cx: &App) -> bool {
        !self.people.read(cx).chips().is_empty() && (self.one_on_one || self.share(cx).is_some())
    }

    fn choose(&mut self, choice: HistoryChoice, cx: &mut Context<Self>) {
        self.choice = choice;
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_add(cx) {
            return;
        }
        let picks = self.people.read(cx).chips().to_vec();
        let chat_id = self.chat_id.clone();
        if self.one_on_one {
            self.app.update(cx, |state, cx| {
                let my_user_id = state.directory.me.as_ref().map(|me| me.user_id.clone());
                let members = state
                    .sidebar
                    .chats
                    .iter()
                    .find(|chat| chat.id == chat_id)
                    .map(|chat| chat.members.clone())
                    .unwrap_or_default();
                let people = group_chat_people(&members, my_user_id.as_deref(), &picks);
                state.start_group_chat_with(people, cx);
            });
        } else {
            let share = self.share(cx).unwrap_or_default();
            let people = picks.iter().map(member_of).collect();
            self.app.update(cx, |state, cx| {
                state.add_chat_members(&chat_id, people, share, cx)
            });
        }
        window.close_dialog(cx);
    }

    fn history_options(&self, cx: &mut Context<Self>) -> Div {
        let form = cx.entity();
        let option = |id: &'static str, label: &'static str, choice: HistoryChoice| {
            let form = form.clone();
            Radio::new(id)
                .label(label)
                .checked(self.choice == choice)
                .on_click(move |_, _, cx| form.update(cx, |form, cx| form.choose(choice, cx)))
        };
        v_flex()
            .debug_selector(|| "add-people/history".into())
            .gap(px(OPTION_GAP))
            .child(option(
                "add-people-no-history",
                "Don't include chat history",
                HistoryChoice::None,
            ))
            .child(
                h_flex()
                    .gap(px(OPTION_GAP))
                    .items_center()
                    .child(option(
                        "add-people-some-history",
                        "Include history from the past number of days:",
                        HistoryChoice::Days,
                    ))
                    .child(
                        div()
                            .w(px(DAYS_FIELD_WIDTH))
                            .flex_none()
                            .child(Input::new(&self.days)),
                    ),
            )
            .child(option(
                "add-people-all-history",
                "Include all chat history",
                HistoryChoice::All,
            ))
    }
}

impl Render for AddPeopleForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "add-people".into())
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .debug_selector(|| "add-people/people".into())
                    .rounded(px(FIELD_RADIUS))
                    .bg(theme::surface_raised())
                    .child(self.people.clone()),
            )
            .when(!self.one_on_one, |column| {
                column.child(self.history_options(cx))
            })
    }
}

pub fn open_add_people(
    app: &Entity<AppState>,
    chat_id: &str,
    one_on_one: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let (app, chat_id) = (app.clone(), chat_id.to_owned());
    let form = cx.new(|cx| AddPeopleForm::new(app, chat_id, one_on_one, window, cx));
    let people = form.read(cx).people.clone();
    window.open_dialog(cx, move |dialog: Dialog, _, cx| {
        let submit_form = form.clone();
        let can_add = form.read(cx).can_add(cx);
        let footer = DialogFooter::new()
            .child(
                Button::new("add-people-cancel")
                    .label("Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("add-people-confirm")
                    .primary()
                    .label("Add")
                    .disabled(!can_add)
                    .on_click(move |_, window, cx| {
                        submit_form.update(cx, |form, cx| form.submit(window, cx));
                    }),
            );
        dialog
            .title(TITLE)
            .w(px(DIALOG_WIDTH))
            .child(form.clone())
            .footer(footer)
    });
    window.defer(cx, move |window, cx| {
        people.update(cx, |draft, cx| draft.focus_query(window, cx));
    });
}

#[cfg(test)]
mod tests {
    use chatsvc::HistoryShare;
    use store::MemberRecord;

    use super::{HistoryChoice, Pick, group_chat_people, history_share};

    fn member(user_id: &str, name: &str) -> MemberRecord {
        MemberRecord {
            user_id: Some(user_id.to_owned()),
            display_name: name.to_owned(),
        }
    }

    #[test]
    fn history_choice_maps_to_the_share_teams_sends() {
        assert_eq!(
            history_share(HistoryChoice::None, "abc"),
            Some(HistoryShare::None)
        );
        assert_eq!(
            history_share(HistoryChoice::All, ""),
            Some(HistoryShare::All)
        );
        assert_eq!(
            history_share(HistoryChoice::Days, "14"),
            Some(HistoryShare::Days(14))
        );
    }

    #[test]
    fn day_counts_outside_one_to_999_disable_adding() {
        assert_eq!(history_share(HistoryChoice::Days, ""), None);
        assert_eq!(history_share(HistoryChoice::Days, "0"), None);
        assert_eq!(history_share(HistoryChoice::Days, "1000"), None);
        assert_eq!(
            history_share(HistoryChoice::Days, "999"),
            Some(HistoryShare::Days(999))
        );
    }

    #[test]
    fn one_on_one_becomes_a_group_with_the_other_member_and_the_picks() {
        let members = [member("me", "Daniel"), member("ann", "Ann")];
        let picks = [Pick {
            user_id: "bob".to_owned(),
            name: "Bob".to_owned(),
            mail: None,
        }];
        let people = group_chat_people(&members, Some("me"), &picks);
        assert_eq!(people, [member("ann", "Ann"), member("bob", "Bob")]);
    }
}
