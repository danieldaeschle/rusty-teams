use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::*;
use mcp::ReactionProposal;
use teams_core::{AnnouncementBanner, FileCard, ImageRef, LinkPreview};

use super::forward_select::SelectionSnapshot;
use super::{ConversationView, HoverSlot, PENDING_KEY_PREFIX};
use crate::agent_api::agent_label;
use crate::agents::AgentRun;
use crate::app_state::AppState;
use crate::forward_selection::block_reason;
use crate::rows::{Delivery, MessageRow, PostRow, ReactionChip, RecordingCard};
use crate::scheduled_rows::SCHEDULED_KEY_PREFIX;
use crate::translation::TranslationLine;
use crate::views::adaptive_card::{ensure_cards_inputs, request_card_refreshes};
use crate::views::attachments::ImageActions;
use crate::views::call_card::CallActions;
use crate::views::call_notice::RecapActions;
use crate::views::composer::Composer;
use crate::views::message_context_menu::message_context_menu;
use crate::views::message_appear::AppearKind;
use crate::views::message_row::{DeliveryActions, RowActions, RowSelect, SharedRowAction};
use crate::views::post_card::{PostAction, PostActions};
use crate::views::reaction_proposal_pill::ReactionPill;
use crate::views::toolbar_placement::TimelineBounds;
use crate::views::translation_line::{LineAction, TranslationActions};

pub(super) struct RenderEnv {
    pub view: WeakEntity<ConversationView>,
    pub app: Entity<AppState>,
    pub highlighted: Option<String>,
    pub drafting: bool,
    pub reply_editor: Option<(String, Entity<Composer>)>,
    pub timeline: TimelineBounds,
    pub agent_runs: Rc<RefCell<HashMap<String, AgentRun>>>,
    pub reaction_proposals: Rc<RefCell<HashMap<String, ReactionProposal>>>,
    pub forward_selection: Option<SelectionSnapshot>,
    pub appearing: HashMap<String, AppearKind>,
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
            .chain(
                message
                    .call
                    .as_ref()
                    .and_then(|call| call.recap.as_ref()?.recording.thumbnail.clone()),
            )
            .chain(message.files.iter().filter_map(FileCard::preview))
            .chain(message.banner.as_ref().and_then(AnnouncementBanner::image))
            .chain(
                message
                    .reactions
                    .iter()
                    .filter_map(ReactionChip::custom_emoji_image),
            )
            .chain(
                app_ui_kit::render::blocks::emoji_urls(&message.blocks)
                    .into_iter()
                    .map(inline_emoji_image),
            )
            .filter(|image| self.app.read(cx).directory.image(&image.url).is_none())
            .collect();
        if !missing_images.is_empty() {
            let app = self.app.clone();
            cx.defer(move |cx| {
                app.update(cx, |state, cx| state.request_images(missing_images, cx));
            });
        }
    }

    fn reaction_pill(&self, message: &MessageRow) -> Option<ReactionPill> {
        let proposal = self
            .reaction_proposals
            .borrow()
            .get(&message.key)
            .cloned()?;
        let (accept_view, discard_view) = (self.view.clone(), self.view.clone());
        let (accept_key, discard_key) = (message.key.clone(), message.key.clone());
        let (emoji, discard_id, accept_id) = (
            proposal.emoji.clone(),
            proposal.id.clone(),
            proposal.id.clone(),
        );
        Some(ReactionPill {
            agent: agent_label(proposal.agent.as_deref()),
            emoji: proposal.emoji,
            accept: Rc::new(move |cx| {
                accept_view
                    .update(cx, |this, cx| {
                        this.accept_reaction(&accept_key, &accept_id, &emoji, cx)
                    })
                    .ok();
            }),
            discard: Rc::new(move |cx| {
                discard_view
                    .update(cx, |this, cx| {
                        this.discard_reaction(&discard_key, &discard_id, cx)
                    })
                    .ok();
            }),
        })
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
        let is_real = !self.drafting
            && !message.key.starts_with(PENDING_KEY_PREFIX)
            && scheduled.is_none()
            && message.call.is_none();
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
        let (menu, react, reaction_controls, add_reaction) = view
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
                let refresh =
                    message
                        .adaptive_cards
                        .iter()
                        .enumerate()
                        .find_map(|(index, card)| {
                            card.refresh
                                .as_ref()
                                .map(|refresh| (index, refresh.action.clone()))
                        });
                let menu = this.message_menu(message, mine, refresh, view.clone(), cx);
                let react = menu.react.clone();
                let visible = this.toolbar_visible(&message.key, cx);
                let controls = (!message.reactions.is_empty())
                    .then(|| this.reaction_controls(&message.key, view.clone()));
                let adder = (!message.reactions.is_empty()).then(|| menu.reaction_adder());
                (visible.then_some(menu), Some(react), controls, adder)
            })
            .unwrap_or((None, None, None, None));
        let right_click_opens_actions = self.app.read(cx).actions_on_right_click;
        let open_context = (right_click_opens_actions && actionable).then(|| {
            let (view, key) = (view.clone(), message.key.clone());
            Rc::new(move |position: Point<Pixels>, cx: &mut App| {
                view.update(cx, |this, cx| this.open_context_menu(&key, position, cx))
                    .ok();
            }) as Rc<dyn Fn(Point<Pixels>, &mut App)>
        });
        let context_menu = view
            .upgrade()
            .filter(|_| actionable)
            .and_then(|entity| {
                let this = entity.read(cx);
                let open = this.context_menu_for(&message.key)?;
                let mine = message
                    .reactions
                    .iter()
                    .filter(|chip| chip.mine)
                    .map(|chip| chip.glyph())
                    .collect();
                let menu = this.message_menu(message, mine, None, view.clone(), cx);
                let (closing, outside) = (view.clone(), view.clone());
                Some((menu, open, closing, outside))
            })
            .map(|(menu, open, closing, outside)| {
                message_context_menu(
                    menu,
                    &open,
                    Rc::new(move |cx| {
                        closing.update(cx, |this, cx| this.close_context_menu(false, cx)).ok();
                    }),
                    Rc::new(move |cx| {
                        outside.update(cx, |this, cx| this.close_context_menu(true, cx)).ok();
                    }),
                    window,
                    cx,
                )
            });
        let reply = (is_real && !message.deleted && !right_click_opens_actions).then(|| {
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
        let open_image = (is_real && !message.deleted && !message.images.is_empty()).then(|| {
            let (open_view, open_key) = (view.clone(), message.key.clone());
            let (share_view, share_key) = (view.clone(), message.key.clone());
            ImageActions {
                open: Rc::new(move |url: &str, cx: &mut App| {
                    open_view
                        .update(cx, |this, cx| this.open_image_viewer(&open_key, url, cx))
                        .ok();
                }),
                share: Some(Rc::new(move |cx: &mut App| {
                    share_view
                        .update(cx, |this, cx| this.forward_message(&share_key, cx))
                        .ok();
                })),
            }
        });
        let bot = self.bot_identity(message, cx);
        let actionable_run = (is_real && !message.deleted)
            .then(|| self.agent_runs.borrow().get(&message.key).cloned())
            .flatten();
        let reaction_proposal = (is_real && !message.deleted)
            .then(|| self.reaction_pill(message))
            .flatten();
        let translation = (is_real && !message.deleted)
            .then(|| self.translation_actions(message))
            .flatten();
        ensure_cards_inputs(
            &self.app,
            &message.adaptive_cards,
            &message.conversation_id,
            &message.key,
            window,
            cx,
        );
        request_card_refreshes(
            &self.app,
            &message.adaptive_cards,
            &message.conversation_id,
            &message.key,
            cx,
        );
        let select = self
            .forward_selection
            .as_ref()
            .map(|snapshot| self.row_select(snapshot, message));
        let selecting = select.is_some();
        RowActions {
            select,
            bot,
            delivery,
            reply: reply.filter(|_| !selecting),
            open_context: open_context.filter(|_| !selecting),
            context_menu: context_menu.filter(|_| !selecting),
            hovered: hovered.filter(|_| !selecting),
            menu: menu.filter(|_| !selecting),
            scheduled_menu,
            react: react.filter(|_| !selecting),
            reaction_controls,
            add_reaction: add_reaction.filter(|_| !selecting),
            files: files.filter(|_| !selecting),
            open_image: open_image.filter(|_| !selecting),
            highlighted: self.highlighted.as_deref() == Some(message.key.as_str()),
            saved: self
                .app
                .read(cx)
                .is_saved(&message.conversation_id, &message.key),
            pinned: self
                .app
                .read(cx)
                .is_pinned(&message.conversation_id, &message.key),
            translation,
            timeline: self.timeline.clone(),
            agent_run: actionable_run,
            call: self.call_actions(message, cx),
            reaction_proposal,
            undo_delete: self.undo_delete_action(message, is_real, view, cx),
            compact: self.app.read(cx).account_chat_settings().message_density
                == teams_core::MessageDensity::Compact,
            appear: self.appearing.get(&message.key).copied(),
        }
    }

    fn row_select(&self, snapshot: &SelectionSnapshot, message: &MessageRow) -> RowSelect {
        let (app, key) = (self.app.clone(), message.key.clone());
        RowSelect {
            checked: snapshot.selection.contains(&message.key),
            blocked: block_reason(message, &snapshot.selection, snapshot.image_count),
            toggle: Rc::new(move |cx| {
                app.update(cx, |state, cx| state.toggle_forward_selected(&key, cx))
            }),
        }
    }

    fn undo_delete_action(
        &self,
        message: &MessageRow,
        is_real: bool,
        view: &WeakEntity<ConversationView>,
        cx: &App,
    ) -> SharedRowAction {
        let restorable = is_real
            && self.app.read(cx).restorable_deletes.can_undo(
                &message.conversation_id,
                &message.key,
                message.own,
                message.deleted,
            );
        restorable.then(|| {
            let (view, key) = (view.clone(), message.key.clone());
            Rc::new(move |cx: &mut App| {
                view.update(cx, |this, cx| this.undo_delete(&key, cx)).ok();
            }) as Rc<dyn Fn(&mut App)>
        })
    }

    fn call_actions(&self, message: &MessageRow, cx: &App) -> CallActions {
        let Some(call) = &message.call else {
            return CallActions::none();
        };
        let app = self.app.clone();
        let conversation_id = message.conversation_id.clone();
        let joinable = call.started && app.read(cx).running_meeting(&conversation_id).is_some();
        let join = joinable.then(|| {
            boxed(move |cx| {
                app.update(cx, |state, cx| {
                    state.join_meeting_call(&conversation_id, cx)
                });
            })
        });
        let open = |recording: &RecordingCard| {
            let (view, message_id, url) = (
                self.view.clone(),
                recording.message_id.clone(),
                recording.url.clone(),
            );
            boxed(move |cx| {
                view.update(cx, |this, cx| {
                    this.open_recording(&message_id, url.as_deref(), cx)
                })
                .ok();
            })
        };
        let recording = call.recap.as_ref().map(|recap| &recap.recording);
        let share = recording
            .and_then(|recording| recording.url.clone())
            .map(|url| {
                boxed(move |cx| cx.write_to_clipboard(ClipboardItem::new_string(url.clone())))
            });
        CallActions {
            join,
            recap: RecapActions {
                view_recap: recording.map(open),
                open_recording: recording.map(open),
                transcript: recording.map(open),
                share,
            },
        }
    }

    fn translation_actions(&self, message: &MessageRow) -> Option<TranslationActions> {
        let line = message.translation.as_ref()?;
        let (app, conversation_id, key) = (
            self.app.clone(),
            message.conversation_id.clone(),
            message.key.clone(),
        );
        let translate: LineAction = {
            let (app, conversation_id, key) = (app.clone(), conversation_id.clone(), key.clone());
            Rc::new(move |cx: &mut App| {
                app.update(cx, |state, cx| {
                    state.translate_message(&conversation_id, &key, cx)
                });
            })
        };
        let never = match line {
            TranslationLine::Offer { language_code, .. } => {
                let language_code = language_code.clone();
                Some(Rc::new(move |cx: &mut App| {
                    app.update(cx, |state, cx| {
                        state.never_translate_language(&language_code, cx)
                    });
                }) as LineAction)
            }
            _ => None,
        };
        Some(TranslationActions { translate, never })
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

    fn request_author_presence(&self, message: &MessageRow, cx: &mut App) {
        let Some(sender_id) = message.sender_id.clone().filter(|_| !message.own) else {
            return;
        };
        let app = self.app.clone();
        cx.defer(move |cx| {
            app.update(cx, |state, cx| state.request_presence(vec![sender_id], cx));
        });
    }

    pub fn post_actions(&self, post: &PostRow, window: &mut Window, cx: &mut App) -> PostActions {
        self.request_author_presence(&post.root, cx);
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
        composer.clone().into_any_element()
    }
}

fn inline_emoji_image(url: String) -> ImageRef {
    let hash = url.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    ImageRef {
        id: format!("emoji-{hash:08x}"),
        url,
        width: None,
        height: None,
        alt: None,
    }
}
