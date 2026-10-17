use gpui_kit::Context;
use store::MessageImportance;
use teams_core::NOTES_CHAT_ID;

use crate::app_state::AppState;
use crate::data::is_one_on_one;
use crate::runtime;

pub const DELIVERY_BUTTON_TITLE: &str = "Set delivery options";
pub const URGENT_MAX_MEMBERS: usize = 20;
pub const URGENT_TOO_MANY_MEMBERS: &str = "Unavailable in chats with more than 20 people";
const MEETING_KIND: &str = "meeting";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryOption {
    pub importance: MessageImportance,
    pub header: &'static str,
    pub content: &'static str,
    pub symbol: &'static str,
    pub tag: &'static str,
}

pub const STANDARD: DeliveryOption = DeliveryOption {
    importance: MessageImportance::Normal,
    header: "Standard",
    content: "Message will be sent as usual",
    symbol: "fluent_chat",
    tag: "standard-delivery-option",
};

pub const IMPORTANT: DeliveryOption = DeliveryOption {
    importance: MessageImportance::High,
    header: "Important",
    content: "Message will be marked as important",
    symbol: "fluent_important",
    tag: "important-delivery-option",
};

pub const URGENT: DeliveryOption = DeliveryOption {
    importance: MessageImportance::Urgent,
    header: "Urgent",
    content: "Recipient(s) notified every 2 min for 20 min",
    symbol: "fluent_alert_urgent",
    tag: "urgent-delivery-option",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgentAvailability {
    Hidden,
    Disabled,
    Available,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeliveryContext<'a> {
    pub conversation_id: &'a str,
    pub chat_kind: &'a str,
    pub member_count: usize,
    pub channel: bool,
    pub one_on_one_bot: bool,
    pub editing: bool,
    pub policy_allows_urgent: bool,
}

/// Teams hides Urgent while editing, in notes, bot 1:1 chats, meeting chats, channels and when the messaging policy denies priority messages; more than 20 people disable it.
pub fn urgent_availability(context: &DeliveryContext) -> UrgentAvailability {
    let hidden = context.editing
        || !context.policy_allows_urgent
        || context.conversation_id == NOTES_CHAT_ID
        || context.one_on_one_bot
        || context.channel
        || context.chat_kind.eq_ignore_ascii_case(MEETING_KIND);
    if hidden {
        UrgentAvailability::Hidden
    } else if context.member_count > URGENT_MAX_MEMBERS {
        UrgentAvailability::Disabled
    } else {
        UrgentAvailability::Available
    }
}

impl AppState {
    pub fn urgent_availability(&self, conversation_id: &str, editing: bool) -> UrgentAvailability {
        let chat = self
            .sidebar
            .chats
            .iter()
            .find(|chat| chat.id == conversation_id);
        let one_on_one_bot = chat.is_some_and(|chat| {
            is_one_on_one(chat)
                && self
                    .bot_apps
                    .get(&chat.id)
                    .is_some_and(|apps| apps.iter().any(|app| !app.bot_ids.is_empty()))
        });
        urgent_availability(&DeliveryContext {
            conversation_id,
            chat_kind: chat.map_or("", |chat| chat.kind.as_str()),
            member_count: chat.map_or(0, |chat| chat.members.len()),
            channel: chat.is_none() && !conversation_id.is_empty(),
            one_on_one_bot,
            editing,
            policy_allows_urgent: self.allow_priority_messages,
        })
    }

    pub fn refresh_messaging_policy(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let read = runtime::spawn(async move { engine.allow_priority_messages().await.ok() });
        cx.spawn(async move |this, cx| {
            let Some(allowed) = read.await.ok().flatten() else {
                return;
            };
            this.update(cx, |state, cx| {
                state.allow_priority_messages = allowed;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

pub fn delivery_options(urgent: UrgentAvailability) -> Vec<(DeliveryOption, bool)> {
    let mut options = vec![(STANDARD, true), (IMPORTANT, true)];
    match urgent {
        UrgentAvailability::Hidden => {}
        UrgentAvailability::Disabled => options.push((URGENT, false)),
        UrgentAvailability::Available => options.push((URGENT, true)),
    }
    options
}

/// The importance that can still be sent once the chat or the policy changed.
pub fn allowed_importance(
    importance: MessageImportance,
    urgent: UrgentAvailability,
) -> MessageImportance {
    if importance == MessageImportance::Urgent && urgent != UrgentAvailability::Available {
        MessageImportance::Normal
    } else {
        importance
    }
}

pub fn urgent_banner(member_count: usize) -> &'static str {
    if member_count > 2 {
        "Notify recipients every 2 minutes for 20 minutes"
    } else {
        "Notify recipient every 2 minutes for 20 minutes"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(member_count: usize) -> DeliveryContext<'static> {
        DeliveryContext {
            conversation_id: "19:group@thread.v2",
            chat_kind: "group",
            member_count,
            policy_allows_urgent: true,
            ..Default::default()
        }
    }

    #[test]
    fn urgent_is_offered_in_ordinary_chats() {
        assert_eq!(
            urgent_availability(&group(5)),
            UrgentAvailability::Available
        );
        let options: Vec<_> = delivery_options(UrgentAvailability::Available)
            .into_iter()
            .map(|(option, enabled)| (option.header, enabled))
            .collect();
        assert_eq!(
            options,
            [("Standard", true), ("Important", true), ("Urgent", true)]
        );
    }

    #[test]
    fn urgent_is_hidden_where_teams_hides_it() {
        let hidden = [
            DeliveryContext {
                conversation_id: NOTES_CHAT_ID,
                ..group(1)
            },
            DeliveryContext {
                chat_kind: "meeting",
                ..group(4)
            },
            DeliveryContext {
                one_on_one_bot: true,
                ..group(1)
            },
            DeliveryContext {
                channel: true,
                ..group(4)
            },
            DeliveryContext {
                editing: true,
                ..group(4)
            },
            DeliveryContext {
                policy_allows_urgent: false,
                ..group(4)
            },
        ];
        for context in hidden {
            assert_eq!(
                urgent_availability(&context),
                UrgentAvailability::Hidden,
                "{context:?}"
            );
        }
        assert_eq!(delivery_options(UrgentAvailability::Hidden).len(), 2);
    }

    #[test]
    fn more_than_twenty_people_disable_urgent() {
        assert_eq!(
            urgent_availability(&group(20)),
            UrgentAvailability::Available
        );
        assert_eq!(
            urgent_availability(&group(21)),
            UrgentAvailability::Disabled
        );
        assert_eq!(
            delivery_options(UrgentAvailability::Disabled)[2],
            (URGENT, false)
        );
    }

    #[test]
    fn a_chosen_urgent_falls_back_to_standard_where_it_is_not_allowed() {
        assert_eq!(
            allowed_importance(MessageImportance::Urgent, UrgentAvailability::Disabled),
            MessageImportance::Normal
        );
        assert_eq!(
            allowed_importance(MessageImportance::High, UrgentAvailability::Hidden),
            MessageImportance::High
        );
    }

    #[test]
    fn the_banner_uses_the_plural_for_more_than_two_members() {
        assert_eq!(
            urgent_banner(2),
            "Notify recipient every 2 minutes for 20 minutes"
        );
        assert_eq!(
            urgent_banner(3),
            "Notify recipients every 2 minutes for 20 minutes"
        );
    }
}
