use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    message_scroller::MessageScrollerState,
    v_flex,
};
use gpui_kit::*;

use super::{ConversationView, ViewMode};
use crate::notice::short_error;
use crate::rows::Row;
use crate::runtime;
use crate::theme;
use crate::views::composer::{Composer, ComposerEvent, ReplyPreview};
use crate::views::widgets::icon;

const REPLY_PLACEHOLDER: &str = "Reply";
const SUBJECT_PLACEHOLDER: &str = "Add a subject";

pub(super) struct FeedStash {
    scroller: Entity<MessageScrollerState>,
    rows: Rc<RefCell<Vec<Row>>>,
    near_top: bool,
}

pub(super) fn subject_input(
    window: &mut Window,
    cx: &mut Context<ConversationView>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(SUBJECT_PLACEHOLDER))
}

impl ConversationView {
    pub(super) fn in_feed(&self) -> bool {
        !self.draft_active
            && self
                .current
                .as_ref()
                .is_some_and(|current| current.mode == ViewMode::ThreadList)
    }

    pub(super) fn open_new_post(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_post_open = true;
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
        cx.notify();
    }

    pub(super) fn close_new_post(&mut self, cx: &mut Context<Self>) {
        if self.new_post_open {
            self.new_post_open = false;
            cx.notify();
        }
    }

    pub(super) fn take_subject(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let subject = self.subject.read(cx).value().trim().to_owned();
        self.subject
            .update(cx, |input, cx| input.set_value("", window, cx));
        (!subject.is_empty()).then_some(subject)
    }

    pub(super) fn restore_subject(
        &mut self,
        subject: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subject = subject.unwrap_or_default().to_owned();
        self.subject
            .update(cx, |input, cx| input.set_value(subject, window, cx));
    }

    pub(super) fn has_subject(&self, cx: &App) -> bool {
        !self.subject.read(cx).value().trim().is_empty()
    }

    pub(super) fn on_subject_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.composer
                .update(cx, |composer, cx| composer.focus(window, cx));
        }
    }

    fn reply_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Composer> {
        if let Some(composer) = &self.reply_composer {
            return composer.clone();
        }
        let composer = cx.new(|cx| Composer::new(self.app.clone(), window, cx).inline());
        let subscription = cx.subscribe_in(&composer, window, Self::on_reply_event);
        self._subscriptions.push(subscription);
        self.reply_composer = Some(composer.clone());
        self.sync_reply_composer(window, cx);
        composer
    }

    pub(super) fn sync_reply_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reply_root = None;
        let (Some(composer), Some(conversation_id)) =
            (self.reply_composer.clone(), self.conversation_id())
        else {
            return;
        };
        composer.update(cx, |composer, cx| {
            composer.set_conversation(&conversation_id, "", window, cx);
            composer.set_placeholder(REPLY_PLACEHOLDER, window, cx);
            composer.set_text("", window, cx);
            composer.set_reply(None, cx);
        });
    }

    pub(super) fn open_reply_editor(
        &mut self,
        root_id: String,
        quote: Option<ReplyPreview>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let composer = self.reply_composer(window, cx);
        let previous = self.reply_root.replace(root_id.clone());
        let changed = previous.as_deref() != Some(root_id.as_str());
        composer.update(cx, |composer, cx| {
            if changed {
                composer.set_text("", window, cx);
            }
            composer.set_reply(quote, cx);
            composer.focus(window, cx);
        });
        self.remeasure_posts(&[Some(root_id), previous], cx);
    }

    pub(super) fn close_reply_editor(&mut self, cx: &mut Context<Self>) {
        if let Some(root_id) = self.reply_root.take() {
            self.remeasure_posts(&[Some(root_id)], cx);
        }
    }

    fn remeasure_posts(&mut self, root_ids: &[Option<String>], cx: &mut Context<Self>) {
        let indices: Vec<usize> = self
            .rows
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                matches!(row, Row::Post(post) if root_ids.iter().flatten().any(|id| *id == post.root.key))
            })
            .map(|(index, _)| index)
            .collect();
        self.scroller.update(cx, |scroller, cx| {
            for index in indices {
                scroller.remeasure_items(index..index + 1, cx);
            }
        });
        cx.notify();
    }

    fn on_reply_event(
        &mut self,
        _: &Entity<Composer>,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Submit(outgoing) => {
                let Some(root_id) = self.reply_root.clone() else {
                    return;
                };
                self.close_reply_editor(cx);
                self.send_in_thread(root_id, (**outgoing).clone(), window, cx);
            }
            ComposerEvent::Typing(active) => {
                let root_id = self.reply_root.clone();
                self.send_typing_in(root_id, *active, cx);
            }
            ComposerEvent::Schedule { .. } | ComposerEvent::EditLast => {}
        }
    }

    pub(super) fn stop_reply_typing(&mut self, cx: &mut Context<Self>) {
        let Some(composer) = self.reply_composer.clone() else {
            return;
        };
        if composer.update(cx, |composer, _| composer.stop_typing()) {
            let root_id = self.reply_root.clone();
            self.send_typing_in(root_id, false, cx);
        }
    }

    pub(super) fn show_main_composer(&mut self, cx: &mut Context<Self>) {
        if self.in_feed() {
            self.new_post_open = true;
            cx.notify();
        }
    }

    pub(super) fn render_new_post(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.in_feed() {
            return None;
        }
        if !self.new_post_open {
            return Some(
                h_flex()
                    .w_full()
                    .flex_none()
                    .px(px(24.))
                    .py(px(10.))
                    .child(
                        h_flex()
                            .id("new-post")
                            .h(px(32.))
                            .px(px(12.))
                            .gap(px(6.))
                            .items_center()
                            .rounded(px(8.))
                            .bg(theme::accent())
                            .text_color(theme::on_accent())
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .hover(|button| button.bg(theme::accent_text()))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_new_post(window, cx)),
                            )
                            .child(icon(IconName::Plus, 14., theme::on_accent()))
                            .child("New post"),
                    )
                    .into_any_element(),
            );
        }
        let editing = self.composer.read(cx).is_editing();
        let focused = self
            .subject
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx);
        let subject = (!editing).then(|| {
            div().w_full().px(px(24.)).pt(px(10.)).child(
                div()
                    .w_full()
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(10.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(if focused {
                        theme::accent()
                    } else {
                        theme::border_strong()
                    })
                    .child(
                        div()
                            .w_full()
                            .child(Input::new(&self.subject).appearance(false).bordered(false)),
                    ),
            )
        });
        Some(
            v_flex()
                .w_full()
                .flex_none()
                .border_b_1()
                .border_color(theme::border())
                .children(subject)
                .child(self.composer.clone())
                .child(
                    h_flex()
                        .w_full()
                        .px(px(24.))
                        .pb(px(10.))
                        .justify_end()
                        .child(
                            Button::new("cancel-new-post")
                                .ghost()
                                .compact()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_new_post(window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn cancel_new_post(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editing = self.composer.read(cx).is_editing();
        if editing {
            self.composer
                .update(cx, |composer, cx| composer.cancel_edit(window, cx));
        }
        self.close_new_post(cx);
    }

    pub(super) fn on_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.reply_root.is_some() {
            self.close_reply_editor(cx);
            return true;
        }
        if self.new_post_open && self.in_feed() {
            self.cancel_new_post(window, cx);
            return true;
        }
        if matches!(
            self.current.as_ref().map(|current| &current.mode),
            Some(ViewMode::Thread(_))
        ) && !self.draft_active
        {
            self.back_to_threads(cx);
            return true;
        }
        false
    }

    pub(super) fn enter_thread_mode(&mut self, root_id: String, cx: &mut Context<Self>) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.mode == ViewMode::ThreadList {
            let stash = FeedStash {
                scroller: self.scroller.clone(),
                rows: self.rows.clone(),
                near_top: self.feed_at_top,
            };
            self.feed_stash = Some(stash);
            self.scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
            self.rows = Rc::new(RefCell::new(Vec::new()));
        }
        current.mode = ViewMode::Thread(root_id);
        self.reply_root = None;
    }

    pub(super) fn enter_feed_mode(&mut self) -> bool {
        let Some(current) = self.current.as_mut() else {
            return false;
        };
        current.mode = ViewMode::ThreadList;
        match self.feed_stash.take() {
            Some(stash) => {
                self.scroller = stash.scroller;
                self.rows = stash.rows;
                self.feed_at_top = stash.near_top;
                true
            }
            None => false,
        }
    }

    pub(super) fn open_thread(
        &mut self,
        root_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_typing(cx);
        self.stop_reply_typing(cx);
        self.enter_thread_mode(root_id.clone(), cx);
        self.reload_pending(cx);
        self.rebuild(true, cx);
        self.refresh_thread(root_id, cx);
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
    }

    pub(super) fn back_to_threads(&mut self, cx: &mut Context<Self>) {
        self.stop_typing(cx);
        let restored = self.enter_feed_mode();
        self.reload_pending(cx);
        self.rebuild(!restored, cx);
    }

    pub(super) fn refresh_thread(&mut self, root_id: String, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        if state.mode.demo || state.mode.read_only {
            return;
        }
        let (Some(engine), Some(conversation_id)) = (state.engine.clone(), self.conversation_id())
        else {
            return;
        };
        let receiver = {
            let (conversation_id, root_id) = (conversation_id.clone(), root_id.clone());
            runtime::spawn(async move { engine.refresh_thread(&conversation_id, &root_id).await })
        };
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            this.update(cx, |this, cx| {
                this.finish_thread_refresh(&conversation_id, &root_id, result, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_thread_refresh(
        &mut self,
        conversation_id: &str,
        root_id: &str,
        result: Result<
            teams_core::Result<teams_core::Delta>,
            tokio::sync::oneshot::error::RecvError,
        >,
        cx: &mut Context<Self>,
    ) {
        let still_open = self.current.as_ref().is_some_and(|current| {
            current.selection.conversation_id() == conversation_id
                && current.mode == ViewMode::Thread(root_id.to_owned())
        });
        if !still_open {
            return;
        }
        let failure = match result {
            Ok(Ok(_)) => None,
            Ok(Err(error)) => Some(short_error(&error)),
            Err(error) => Some(short_error(&error)),
        };
        if let Some(failure) = failure {
            self.notice = Some(format!("Could not load all replies: {failure}"));
        }
        self.rebuild(false, cx);
    }
}
