use std::cell::RefCell;
use std::rc::Rc;

use chatsvc::{INVALID_TOPIC_CHARACTER, MAX_TOPIC_LENGTH, TopicProblem, topic_problem};
use gpui_kit::assets::IconName;
use gpui_kit::base::PopoverState;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::icon;
use crate::app_state::AppState;
use crate::theme;

const BUTTON_TOOLTIP: &str = "Name group chat";
const FIELD_LABEL: &str = "Group name";
const PLACEHOLDER: &str = "Type a group name";
const FORM_WIDTH: f32 = 300.;
const FORM_PADDING: f32 = 12.;
const FORM_GAP: f32 = 8.;
const LABEL_SIZE: f32 = 12.;
const ICON_SIZE: f32 = 16.;

pub fn problem_text(problem: TopicProblem) -> String {
    match problem {
        TopicProblem::TooLong => {
            format!("Your group name can't be longer than {MAX_TOPIC_LENGTH} characters.")
        }
        TopicProblem::InvalidCharacter => {
            format!("'{INVALID_TOPIC_CHARACTER}' is not a valid character in group names.")
        }
    }
}

struct GroupNameForm {
    app: Entity<AppState>,
    chat_id: String,
    input: Entity<InputState>,
    popover: WeakEntity<PopoverState>,
    _subscription: Subscription,
}

impl GroupNameForm {
    fn new(
        app: Entity<AppState>,
        chat_id: String,
        title: String,
        popover: WeakEntity<PopoverState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(PLACEHOLDER);
            state.set_value(title, window, cx);
            state
        });
        input.update(cx, |state, cx| state.focus(window, cx));
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        GroupNameForm {
            app,
            chat_id,
            input,
            popover,
            _subscription: subscription,
        }
    }

    fn on_input_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } => self.save(window, cx),
            _ => {}
        }
    }

    fn problem(&self, cx: &App) -> Option<TopicProblem> {
        topic_problem(self.input.read(cx).value().trim())
    }

    fn close(&self, window: &mut Window, cx: &mut App) {
        self.popover
            .update(cx, |state, cx| state.dismiss(window, cx))
            .ok();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.problem(cx).is_some() {
            return;
        }
        let topic = self.input.read(cx).value().trim().to_owned();
        let chat_id = self.chat_id.clone();
        self.app
            .update(cx, |state, cx| state.rename_chat(&chat_id, &topic, cx));
        self.close(window, cx);
    }
}

impl Render for GroupNameForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let problem = self.problem(cx);
        v_flex()
            .w(px(FORM_WIDTH))
            .p(px(FORM_PADDING))
            .gap(px(FORM_GAP))
            .child(
                div()
                    .text_size(px(LABEL_SIZE))
                    .text_color(theme::text_soft())
                    .child(FIELD_LABEL),
            )
            .child(Input::new(&self.input))
            .when_some(problem, |form, problem| {
                form.child(
                    div()
                        .text_size(px(LABEL_SIZE))
                        .text_color(theme::red_soft())
                        .child(problem_text(problem)),
                )
            })
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(FORM_GAP))
                    .child(
                        Button::new("group-name-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|form, _, window, cx| form.close(window, cx))),
                    )
                    .child(
                        Button::new("group-name-save")
                            .primary()
                            .label("Save")
                            .disabled(problem.is_some())
                            .on_click(cx.listener(|form, _, window, cx| form.save(window, cx))),
                    ),
            )
    }
}

pub fn group_name_button(app: &Entity<AppState>, chat_id: &str, title: &str) -> AnyElement {
    let (app, chat_id, title) = (app.clone(), chat_id.to_owned(), title.to_owned());
    let form: Rc<RefCell<Option<Entity<GroupNameForm>>>> = Rc::default();
    let reset = form.clone();
    Popover::new("group-name")
        .anchor(Anchor::TopLeft)
        .p(px(0.))
        .trigger(
            Button::new("group-name-button")
                .ghost()
                .compact()
                .tooltip(BUTTON_TOOLTIP)
                .child(icon(IconName::Pencil, ICON_SIZE, theme::text_soft())),
        )
        .on_open_change(move |open, _, _| {
            if !*open {
                reset.borrow_mut().take();
            }
        })
        .content(move |_, window, cx| {
            if let Some(existing) = form.borrow().clone() {
                return existing;
            }
            let popover = cx.entity().downgrade();
            let (app, chat_id, title) = (app.clone(), chat_id.clone(), title.clone());
            let created = cx.new(|cx| GroupNameForm::new(app, chat_id, title, popover, window, cx));
            *form.borrow_mut() = Some(created.clone());
            created
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{TopicProblem, problem_text};

    #[test]
    fn problems_use_the_teams_wording() {
        assert_eq!(
            problem_text(TopicProblem::TooLong),
            "Your group name can't be longer than 250 characters."
        );
        assert_eq!(
            problem_text(TopicProblem::InvalidCharacter),
            "':' is not a valid character in group names."
        );
    }
}
