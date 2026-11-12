use app_ui_kit::layout::ISLAND_GAP;
use gpui_kit::component::{TitleBar, elevation, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::call_end_view::render_call_end;
use super::call_pop_out::zoom_listener;
use super::call_stage::render_stage_overlay;
use super::call_view::render_call_view;
use super::conversation::ConversationView;
use super::prejoin_view::PrejoinView;
use super::actions::ToggleCallMute;
use super::shortcuts::{
    HangUpCall, OpenLobbyList, PushToTalk, ToggleCallCamera, ToggleCallFullscreen, ToggleCallHand,
    ToggleCallShare, ZoomShareIn, ZoomShareOut, ZoomShareReset,
};
use super::title_bar::{TITLE_BAR_HEIGHT, app_icon_image};
use crate::app_state::{AppState, Selection};
use crate::call::ZoomStep;
use crate::theme;

pub const CALL_WINDOW_TAG: &str = "call-window";
pub const CALL_WINDOW_SIZE: (f32, f32) = (1180., 760.);
pub const CALL_WINDOW_MIN_SIZE: (f32, f32) = (560., 420.);
const FALLBACK_TITLE: &str = "Call";
const KEY_CONTEXT: &str = "CallWindow";
const APP_ICON_SIZE: f32 = 18.;

pub fn call_window_title(state: &AppState) -> String {
    state
        .call
        .as_ref()
        .map(|call| call.model.title.clone())
        .or_else(|| state.prejoin.as_ref().map(|pending| pending.join.title.clone()))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| FALLBACK_TITLE.to_owned())
}

/// The meeting window: like Teams, a call or meeting runs in its own window and the main window stays free for chats.
pub struct CallWindowView {
    app: Entity<AppState>,
    focus: FocusHandle,
    call_chat: Option<(String, Entity<ConversationView>)>,
    prejoin: Option<Entity<PrejoinView>>,
    fullscreen: bool,
    was_fullscreen: bool,
    _app_subscription: Subscription,
    _media_subscription: Option<Subscription>,
}

impl CallWindowView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app_subscription = cx.observe(&app, |_, _, cx| cx.notify());
        let media_subscription = app
            .read(cx)
            .call_media
            .clone()
            .map(|media| cx.observe(&media, |_, _, cx| cx.notify()));
        let closing_app = app.clone();
        window.on_window_should_close(cx, move |_, cx| {
            leave_or_dismiss(&closing_app, cx);
            true
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        CallWindowView {
            app,
            focus,
            call_chat: None,
            prejoin: None,
            fullscreen: false,
            was_fullscreen: false,
            _app_subscription: app_subscription,
            _media_subscription: media_subscription,
        }
    }

    fn sync_call_chat(&mut self, thread: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        match (thread, &self.call_chat) {
            (Some(wanted), Some((open, _))) if &wanted == open => {}
            (Some(wanted), _) => {
                let selection = Selection::Chat(wanted.clone());
                let app = self.app.clone();
                let view = cx.new(|cx| ConversationView::pinned_to(app, selection, window, cx));
                self.call_chat = Some((wanted, view));
            }
            (None, _) => self.call_chat = None,
        }
    }

    fn sync_prejoin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = self.app.read(cx).prejoin.clone();
        let open = self.prejoin.as_ref().map(|view| view.read(cx).pending_id());
        match (wanted, open) {
            (Some(pending), Some(id)) if pending.id == id => {}
            (Some(pending), _) => {
                let app = self.app.clone();
                self.prejoin = Some(cx.new(|cx| PrejoinView::new(app, &pending, window, cx)));
            }
            (None, _) => self.prejoin = None,
        }
    }

    fn sync_fullscreen(&mut self, wanted: bool, window: &mut Window) {
        if wanted == self.fullscreen {
            return;
        }
        self.fullscreen = wanted;
        if wanted {
            self.was_fullscreen = window.is_fullscreen();
        }
        if wanted != window.is_fullscreen() && (wanted || !self.was_fullscreen) {
            window.toggle_fullscreen();
        }
    }

    fn title_bar(&self, title: String, cx: &App) -> impl IntoElement {
        let app_icon = crate::active_appearance::current(cx).app_icon;
        let app = self.app.clone();
        TitleBar::new()
            .h(px(TITLE_BAR_HEIGHT))
            .border_b_0()
            .bg(theme::background())
            .on_close_window(move |_, window, cx| {
                leave_or_dismiss(&app, cx);
                window.remove_window();
            })
            .child(
                h_flex()
                    .debug_selector(|| "call-window/title-bar".into())
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap(px(12.))
                    .items_center()
                    .child(img(app_icon_image(app_icon)).size(px(APP_ICON_SIZE)).flex_none())
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(title),
                    ),
            )
    }
}

fn call_action<A: Action>(
    app: &Entity<AppState>,
    action: fn(&mut AppState, &mut Context<AppState>),
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let app = app.clone();
    move |_, _, cx| app.update(cx, |state, cx| action(state, cx))
}

fn leave_or_dismiss(app: &Entity<AppState>, cx: &mut App) {
    app.update(cx, |state, cx| {
        if state.call.is_some() {
            state.leave_call(cx);
        } else if state.prejoin.is_some() {
            state.cancel_prejoin(cx);
        } else {
            state.dismiss_call_end(cx);
        }
    });
}

impl Focusable for CallWindowView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CallWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chat_thread = self.app.read(cx).call_chat_thread();
        self.sync_call_chat(chat_thread, window, cx);
        self.sync_prejoin(window, cx);
        let state = self.app.read(cx);
        let fullscreen = state.call_fullscreen();
        let open_dialogs = [
            state.transfer_picker.is_some(),
            state.call.as_ref().is_some_and(|call| call.options_dialog.is_some()),
            state.call.as_ref().is_some_and(|call| call.model.consent_required),
        ]
        .into_iter()
        .filter(|open| *open)
        .count();
        let title = call_window_title(state);
        window.set_window_title(&title);
        let window_width = f32::from(window.viewport_size().width);
        let stage_width = window_width - 2. * ISLAND_GAP;
        let chat_panel = self.call_chat.as_ref().map(|(_, view)| view.clone().into());
        let stage = render_call_view(&self.app, state, chat_panel, fullscreen, stage_width)
            .or_else(|| render_call_end(&self.app, state))
            .or_else(|| self.prejoin.clone().map(|prejoin| prejoin.into_any_element()));
        let stage_overlay = render_stage_overlay(&self.app, state);
        elevation::set_app_modal_layers(window, open_dialogs, cx);
        self.sync_fullscreen(fullscreen, window);
        let title_bar = (!fullscreen).then(|| self.title_bar(title, cx));
        let release_app = self.app.clone();
        let modifier_app = self.app.clone();
        let escape_app = self.app.clone();
        v_flex()
            .id(CALL_WINDOW_TAG)
            .debug_selector(|| CALL_WINDOW_TAG.into())
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(theme::background())
            .text_color(theme::text())
            .text_size(px(14.))
            .on_action(call_action::<ToggleCallMute>(&self.app, AppState::toggle_call_mute))
            .on_action(call_action::<PushToTalk>(&self.app, AppState::press_push_to_talk))
            .on_action(call_action::<HangUpCall>(&self.app, AppState::leave_call))
            .on_action(call_action::<ToggleCallCamera>(&self.app, AppState::toggle_call_camera))
            .on_action(call_action::<ToggleCallShare>(&self.app, AppState::toggle_call_share))
            .on_action(call_action::<ToggleCallHand>(&self.app, AppState::toggle_call_hand))
            .on_action(call_action::<OpenLobbyList>(&self.app, AppState::open_call_lobby_list))
            .on_action(call_action::<ToggleCallFullscreen>(&self.app, AppState::toggle_call_fullscreen))
            .on_action(zoom_listener::<ZoomShareIn>(&self.app, ZoomStep::In))
            .on_action(zoom_listener::<ZoomShareOut>(&self.app, ZoomStep::Out))
            .on_action(zoom_listener::<ZoomShareReset>(&self.app, ZoomStep::Reset))
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    escape_app.update(cx, |state, cx| state.escape_call_fullscreen(cx));
                }
            })
            .on_key_up(move |event: &KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" {
                    release_app.update(cx, |state, cx| state.release_push_to_talk(cx));
                }
            })
            .on_modifiers_changed(move |event: &ModifiersChangedEvent, _, cx| {
                if !event.modifiers.control {
                    modifier_app.update(cx, |state, cx| state.release_push_to_talk(cx));
                }
            })
            .children(title_bar)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .when(!fullscreen, |row| row.px(px(ISLAND_GAP)).pb(px(ISLAND_GAP)))
                    .children(stage),
            )
            .children(stage_overlay)
    }
}
