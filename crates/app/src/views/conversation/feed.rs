use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, InputState},
    message_scroller::MessageScrollerState,
    v_flex,
};
use gpui_kit::*;

use super::{ConversationView, ViewMode};
use crate::i18n::tr;
use crate::notice::short_error;
use crate::rows::Row;
use crate::runtime;
use crate::views::channel_tabs::ChannelPane;
use crate::views::composer::{Composer, ComposerEvent, ReplyPreview};
use crate::views::post_card::{PostAction, post_in_channel_card};

const REPLY_PLACEHOLDER: &str = "Reply in thread";

pub(super) struct FeedStash {
    scroller: Entity<MessageScrollerState>,
    rows: Rc<RefCell<Vec<Row>>>,
    near_top: bool,
}

pub(super) fn subject_input(
    window: &mut Window,
    cx: &mut Context<ConversationView>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx).placeholder(crate::views::composer::SUBJECT_PLACEHOLDER)
    })
}

impl ConversationView {
    pub(super) fn in_feed(&self) -> bool {
        !self.draft_active
            && self
                .current
                .as_ref()
                .is_some_and(|current| current.mode == ViewMode::ThreadList)
    }

    pub fn start_channel_post(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.in_feed() {
            self.open_new_post(window, cx);
        }
    }

    pub(super) fn open_new_post(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mark_new_post_open(cx);
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
    }

    pub(super) fn open_new_announcement(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mark_new_post_open(cx);
        self.composer
            .update(cx, |composer, cx| composer.begin_announcement(window, cx));
    }

    fn mark_new_post_open(&mut self, cx: &mut Context<Self>) {
        self.new_post_open = true;
        let subject = self.subject.clone();
        self.composer.update(cx, |composer, cx| {
            composer.set_post_layout(Some(subject), cx)
        });
        cx.notify();
    }

    pub(super) fn restore_new_post(
        &mut self,
        subject: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.in_feed() {
            return;
        }
        self.restore_subject(subject.as_deref(), window, cx);
        self.mark_new_post_open(cx);
        self.composer.update(cx, |composer, cx| {
            composer.sync_subject_placeholder(window, cx)
        });
    }

    pub(super) fn close_new_post(&mut self, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.set_post_layout(None, cx));
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
        match event {
            InputEvent::PressEnter { .. } => self
                .composer
                .update(cx, |composer, cx| composer.focus(window, cx)),
            InputEvent::Change => self
                .composer
                .update(cx, |composer, cx| composer.schedule_draft_save(cx)),
            _ => {}
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

    fn keep_reply_editor_in_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root_id) = self.reply_root.clone() else {
            return;
        };
        if !self.in_feed() {
            return;
        }
        let view = cx.entity().downgrade();
        window.on_next_frame(move |_, cx| {
            view.update(cx, |this, cx| {
                let index = this
                    .rows
                    .borrow()
                    .iter()
                    .position(|row| row.key() == root_id);
                if let Some(index) = index {
                    this.scroller
                        .update(cx, |scroller, cx| scroller.reveal_item_bottom(index, cx));
                }
            })
            .ok();
        });
    }

    pub(super) fn sync_reply_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reply_root = None;
        let (Some(composer), Some(conversation_id)) =
            (self.reply_composer.clone(), self.conversation_id())
        else {
            return;
        };
        composer.update(cx, |composer, cx| {
            composer.set_conversation(&conversation_id, window, cx);
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
        self.report_composer_activity(event, cx);
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
            ComposerEvent::Edited => self.keep_reply_editor_in_view(window, cx),
            ComposerEvent::Schedule { .. }
            | ComposerEvent::EditLast
            | ComposerEvent::FocusMessages
            | ComposerEvent::ClosePost
            | ComposerEvent::RestorePost(_) => {}
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
            self.mark_new_post_open(cx);
        }
    }

    pub(super) fn render_new_post(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.in_feed() || self.pane != ChannelPane::Posts {
            return None;
        }
        let proposal_card = self.render_proposal_card(cx);
        if !self.new_post_open {
            let view = cx.entity().downgrade();
            let announce_view = view.clone();
            let open: PostAction = Box::new(move |window, cx| {
                view.update(cx, |this, cx| this.open_new_post(window, cx))
                    .ok();
            });
            let announce: PostAction = Box::new(move |window, cx| {
                announce_view
                    .update(cx, |this, cx| this.open_new_announcement(window, cx))
                    .ok();
            });
            return Some(
                v_flex()
                    .w_full()
                    .flex_none()
                    .children(proposal_card)
                    .child(div().w_full().flex_none().px(px(20.)).py(px(12.)).child(
                        post_in_channel_card(&self.app.read(cx).directory, open, announce),
                    ))
                    .into_any_element(),
            );
        }
        let editing = self.composer.read(cx).is_editing();
        let cancel = editing.then(|| {
            h_flex()
                .w_full()
                .px(px(20.))
                .pb(px(12.))
                .justify_end()
                .child(
                    Button::new("cancel-new-post")
                        .ghost()
                        .compact()
                        .label(tr("common.cancel"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.cancel_new_post(window, cx)),
                        ),
                )
        });
        Some(
            v_flex()
                .w_full()
                .flex_none()
                .children(proposal_card)
                .child(div().w_full().pt(px(12.)).child(self.composer.clone()))
                .children(cancel)
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
        if self.close_new_folder(cx) {
            return true;
        }
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
