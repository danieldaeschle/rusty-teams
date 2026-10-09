use std::rc::Rc;

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::*;
use teams_core::{ImageRef, LinkPreview};

use super::{ConversationView, HoverSlot, PENDING_KEY_PREFIX};
use crate::app_state::AppState;
use crate::rows::{Delivery, MessageRow, PostRow};
use crate::scheduled_rows::SCHEDULED_KEY_PREFIX;
use crate::views::adaptive_card::ensure_cards_inputs;
use crate::views::composer::Composer;
use crate::views::message_row::{DeliveryActions, RowActions};
use crate::views::post_card::{PostAction, PostActions};

pub(super) struct RenderEnv {
    pub view: WeakEntity<ConversationView>,
    pub app: Entity<AppState>,
    pub highlighted: Option<String>,
    pub drafting: bool,
    pub reply_editor: Option<(String, Entity<Composer>)>,
}

fn boxed(action: impl Fn(&mut App) + 'static) -> Box<dyn Fn(&mut App)> {
    Box::new(action)
}

impl RenderEnv {
    fn request_missing_media(&self, message: &MessageRow, cx: &mut App) {
        let senders: Vec<String> = (!message.own && !message.series.has_prev)
            .then(|| message.sender_id.clone())
            .flatten()
            .into_iter()
            .chain(
                message
                    .reactions
                    .iter()
                    .flat_map(|chip| chip.reactors.iter())
                    .filter_map(|reactor| reactor.user_id.clone()),
            )
            .filter(|sender| self.app.read(cx).directory.avatar(sender).is_none())
            .collect();
        if !senders.is_empty() {
            let app = self.app.clone();
            cx.defer(move |cx| {
                app.update(cx, |state, cx| state.request_avatars(senders, cx));
            });
        }
        let missing_images: Vec<ImageRef> = message
            .images
            .iter()
            .cloned()
            .chain(message.link_preview.as_ref().and_then(LinkPreview::image))
            .filter(|image| self.app.read(cx).directory.image(&image.url).is_none())
            .collect();
        if !missing_images.is_empty() {
            let app = self.app.clone();
            cx.defer(move |cx| {
                app.update(cx, |state, cx| state.request_images(missing_images, cx));
            });
        }
    }

    fn bot_identity(
        &self,
        message: &MessageRow,
        cx: &mut App,
    ) -> Option<crate::card_state::BotIdentity> {
        let application_id = message.application_id.as_deref()?;
        let known = self
            .app
            .read(cx)
            .bot_identity(&message.conversation_id, application_id);
        if known.is_none() {
            let (app, conversation_id) = (self.app.clone(), message.conversation_id.clone());
            cx.defer(move |cx| {
                app.update(cx, |state, cx| {
                    state.request_chat_apps(&conversation_id, cx)
                });
            });
        }
        known
    }

    pub fn message_actions(
        &self,
        message: &MessageRow,
        window: &mut Window,
        cx: &mut App,
    ) -> RowActions {
        let view = &self.view;
        let failed = matches!(message.delivery, Delivery::Failed(_));
        let retry = failed.then(|| {
            let (view, key, handle) = (view.clone(), message.key.clone(), window.window_handle());
            boxed(move |cx| {
                let (view, key) = (view.clone(), key.clone());
                handle
                    .update(cx, move |_, window, cx| {
                        view.update(cx, |this, cx| this.retry(&key, window, cx))
                            .ok();
                    })
                    .ok();
            })
        });
        let scheduled = view.upgrade().and_then(|entity| {
            entity
                .read(cx)
                .scheduled_row_actions(message, view, window, cx)
        });
        let delete = failed.then(|| {
            let (view, key) = (view.clone(), message.key.clone());
            boxed(move |cx| {
                view.update(cx, |this, cx| this.delete_pending(&key, cx))
                    .ok();
            })
        });
        self.request_missing_media(message, cx);
        let is_real =
            !self.drafting && !message.key.starts_with(PENDING_KEY_PREFIX) && scheduled.is_none();
        let hovered = is_real.then(|| {
            let (view, key) = (view.clone(), message.key.clone());
            Rc::new(move |hovered: bool, cx: &mut App| {
                view.update(cx, |this, cx| {
                    this.set_hover(HoverSlot::Row, &key, hovered, cx)
                })
                .ok();
            }) as Rc<dyn Fn(bool, &mut App)>
        });
        let (delivery, scheduled_menu, hovered) = match scheduled {
            Some(actions) => (actions.delivery, actions.menu, Some(actions.hovered)),
            None => (
                DeliveryActions {
                    retry,
                    delete,
                    send_now: None,
                },
                None,
                hovered,
            ),
        };
        let actionable = is_real && !message.deleted;
        let (menu, react, reaction_controls) = view
            .upgrade()
            .filter(|_| actionable)
            .map(|entity| {
                let this = entity.read(cx);
                let mine = message
                    .reactions
                    .iter()
                    .filter(|chip| chip.mine)
                    .map(|chip| chip.glyph())
                    .collect();
                let menu = this.message_menu(&message.key, message.own, mine, view.clone(), cx);
                let react = menu.react.clone();
                let visible = this.toolbar_visible(&message.key);
                let controls = (!message.reactions.is_empty())
                    .then(|| this.reaction_controls(&message.key, view.clone()));
                (visible.then_some(menu), Some(react), controls)
            })
            .unwrap_or((None, None, None));
        let reply = (is_real && !message.deleted).then(|| {
            let (view, key, handle) = (view.clone(), message.key.clone(), window.window_handle());
            boxed(move |cx| {
                let (view, key) = (view.clone(), key.clone());
                handle
                    .update(cx, move |_, window, cx| {
                        view.update(cx, |this, cx| this.begin_reply(&key, window, cx))
                            .ok();
                    })
                    .ok();
            })
        });
        let files = (is_real && !message.deleted && !message.files.is_empty())
            .then(|| view.upgrade())
            .flatten()
            .map(|entity| {
                entity
                    .read(cx)
                    .file_actions(&message.key, &message.files, view.clone())
            });
        let bot = self.bot_identity(message, cx);
        ensure_cards_inputs(
            &self.app,
            &message.adaptive_cards,
            &message.conversation_id,
            &message.key,
            window,
            cx,
        );
        RowActions {
            bot,
            delivery,
            reply,
            hovered,
            menu,
            scheduled_menu,
            react,
            reaction_controls,
            files,
            highlighted: self.highlighted.as_deref() == Some(message.key.as_str()),
            saved: self
                .app
                .read(cx)
                .is_saved(&message.conversation_id, &message.key),
        }
    }

    fn open_by_root(
        &self,
        root_key: String,
        run: fn(&mut ConversationView, String, &mut Window, &mut Context<ConversationView>),
    ) -> PostAction {
        let view = self.view.clone();
        Box::new(move |window, cx| {
            view.update(cx, |this, cx| run(this, root_key.clone(), window, cx))
                .ok();
        })
    }

    pub fn post_actions(&self, post: &PostRow, window: &mut Window, cx: &mut App) -> PostActions {
        let root_key = post.root.key.clone();
        let is_local =
            root_key.starts_with(PENDING_KEY_PREFIX) || root_key.starts_with(SCHEDULED_KEY_PREFIX);
        let open_thread = Some(
            self.open_by_root(root_key.clone(), |this, root_key, window, cx| {
                this.open_thread(root_key, window, cx)
            }),
        );
        let open_reply = (!is_local).then(|| {
            self.open_by_root(root_key.clone(), |this, root_key, window, cx| {
                this.open_reply_editor(root_key, None, window, cx)
            })
        });
        let reply_editor = self
            .reply_editor
            .as_ref()
            .filter(|(open_root, _)| *open_root == root_key)
            .map(|(_, composer)| self.inline_reply_editor(composer));
        PostActions {
            root: self.message_actions(&post.root, window, cx),
            replies: post
                .replies
                .iter()
                .map(|reply| self.message_actions(reply, window, cx))
                .collect(),
            open_thread,
            open_reply,
            reply_editor,
        }
    }

    fn inline_reply_editor(&self, composer: &Entity<Composer>) -> AnyElement {
        let view = self.view.clone();
        v_flex()
            .w_full()
            .gap(px(6.))
            .child(composer.clone())
            .child(
                h_flex().justify_end().child(
                    Button::new("cancel-inline-reply")
                        .ghost()
                        .compact()
                        .label("Cancel")
                        .on_click(move |_, _, cx| {
                            view.update(cx, |this, cx| this.close_reply_editor(cx)).ok();
                        }),
                ),
            )
            .into_any_element()
    }
}
