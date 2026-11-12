use std::collections::HashMap;

use app_ui_kit::layout::ISLAND_GAP;
use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::conversation::ConversationView;
use super::shell::AppShell;
use crate::app_state::{AppState, Selection, selection_title};
use crate::theme;

pub const POP_OUT_CHAT_LABEL: &str = "Pop out chat";
pub const POP_IN_TOOLTIP: &str = "Pop in back to the main window";
const CHAT_WINDOW_SIZE: (f32, f32) = (760., 820.);
const CHAT_WINDOW_MIN_SIZE: (f32, f32) = (420., 480.);

#[derive(Default)]
struct ChatPopOuts {
    windows: HashMap<String, AnyWindowHandle>,
    opening: Vec<String>,
}

impl Global for ChatPopOuts {}

pub struct ChatPopOutView {
    conversation: Entity<ConversationView>,
}

impl ChatPopOutView {
    fn new(
        app: Entity<AppState>,
        chat_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let closing_chat_id = chat_id.clone();
        window.on_window_should_close(cx, move |_, cx| {
            forget(&closing_chat_id, cx);
            true
        });
        let selection = Selection::Chat(chat_id);
        let conversation = cx.new(|cx| ConversationView::popped_out(app, selection, window, cx));
        ChatPopOutView { conversation }
    }
}

impl Render for ChatPopOutView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("chat-pop-out")
            .size_full()
            .p(px(ISLAND_GAP))
            .bg(theme::background())
            .child(self.conversation.clone())
    }
}

fn forget(chat_id: &str, cx: &mut App) {
    if cx.has_global::<ChatPopOuts>() {
        cx.global_mut::<ChatPopOuts>().windows.remove(chat_id);
    }
}

pub fn pop_out_chat(app: &Entity<AppState>, chat_id: &str, cx: &mut App) {
    let pop_outs = cx.default_global::<ChatPopOuts>();
    if pop_outs.opening.iter().any(|opening| opening == chat_id) {
        return;
    }
    if let Some(handle) = pop_outs.windows.get(chat_id).copied() {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        cx.global_mut::<ChatPopOuts>().windows.remove(chat_id);
    }
    cx.global_mut::<ChatPopOuts>()
        .opening
        .push(chat_id.to_owned());
    let title = selection_title(&app.read(cx).sidebar, &Selection::Chat(chat_id.to_owned()));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(CHAT_WINDOW_SIZE.0), px(CHAT_WINDOW_SIZE.1)),
            cx,
        ))),
        window_min_size: Some(size(px(CHAT_WINDOW_MIN_SIZE.0), px(CHAT_WINDOW_MIN_SIZE.1))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (app, chat_id) = (app.clone(), chat_id.to_owned());
    // open_window draws right away, and the conversation view reads the app entity while rendering.
    cx.defer(move |cx| {
        let window_chat_id = chat_id.clone();
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| ChatPopOutView::new(app, window_chat_id, window, cx))
        });
        let pop_outs = cx.global_mut::<ChatPopOuts>();
        pop_outs.opening.retain(|opening| opening != &chat_id);
        if let Ok((handle, _)) = opened {
            pop_outs.windows.insert(chat_id, handle);
        }
    });
}

pub fn return_to_main_window(
    app: &Entity<AppState>,
    chat_id: &str,
    window: &mut Window,
    cx: &mut App,
) {
    forget(chat_id, cx);
    let selection = Selection::Chat(chat_id.to_owned());
    app.update(cx, |state, cx| state.select(selection, cx));
    let main_window = cx
        .windows()
        .into_iter()
        .find(|handle| handle.downcast::<AppShell>().is_some());
    if let Some(main_window) = main_window {
        main_window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    }
    window.remove_window();
}
