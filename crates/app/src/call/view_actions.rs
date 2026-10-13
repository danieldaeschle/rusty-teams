use std::time::{Duration, Instant};

use calling::CallCommand;
use gpui_kit::*;

use super::caption_style::{CaptionPosition, CaptionSize};
use super::bubbles::{BUBBLE_SHOWN, ChatBubble, push_bubble};
use super::fullscreen::{FullscreenState, HINT_SHOWN};
use super::view_settings::{CallLayout, ViewSettings};
use crate::app_state::{AppEvent, AppState};
use crate::views::call_layouts::{measured_stage, video_wants};

const CHROME_TOUCH_THROTTLE: Duration = Duration::from_millis(250);
const DEMO_BUBBLE_START: Duration = Duration::from_secs(8);
const DEMO_BUBBLE_EVERY: Duration = Duration::from_secs(7);
const DEMO_BUBBLES: [(&str, &str); 4] = [
    ("Jonas Ortega", "Slides are in the channel now"),
    (
        "Priya Nair",
        "Can you share the release notes with us before we start the demo?",
    ),
    ("Lea Schneider", "Looks good to me, thanks"),
    ("Tobias Klein", "I will join the retro in five minutes"),
];
const TIMER_SLACK: Duration = Duration::from_millis(30);

impl AppState {
    fn change_call_view(&mut self, change: impl FnOnce(&mut ViewSettings), cx: &mut Context<Self>) {
        change(&mut self.call_view);
        self.call_view.hide_me = false;
        self.call_view.save(&self.store);
        if let Some(call) = self.call.as_mut() {
            let hide_me = call.model.view.hide_me;
            call.model.view = self.call_view;
            call.model.view.hide_me = hide_me;
            call.model.refresh_layout(Instant::now());
        }
        cx.notify();
    }

    pub fn set_call_layout(&mut self, layout: CallLayout, cx: &mut Context<Self>) {
        self.change_call_view(|view| view.choose(layout), cx);
    }

    pub fn toggle_call_hide_no_video(&mut self, cx: &mut Context<Self>) {
        self.change_call_view(|view| view.hide_no_video = !view.hide_no_video, cx);
    }

    pub fn set_call_caption_size(&mut self, size: CaptionSize, cx: &mut Context<Self>) {
        self.change_call_view(|view| view.caption_size = size, cx);
    }

    pub fn set_call_caption_position(&mut self, position: CaptionPosition, cx: &mut Context<Self>) {
        self.change_call_view(|view| view.caption_position = position, cx);
    }

    pub fn toggle_call_chat_bubbles(&mut self, cx: &mut Context<Self>) {
        self.change_call_view(|view| view.chat_bubbles = !view.chat_bubbles, cx);
    }

    pub fn set_call_hide_me(&mut self, hidden: bool, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.model.view.hide_me = hidden;
            call.model.refresh_layout(Instant::now());
            cx.notify();
        }
    }

    pub fn tick_call_layout(&mut self, cx: &mut Context<Self>) {
        let changed = self
            .call
            .as_mut()
            .is_some_and(|call| call.model.refresh_layout(Instant::now()));
        self.sync_video_wants();
        if changed {
            cx.notify();
        }
    }

    fn sync_video_wants(&mut self) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let wants = video_wants(&call.model, measured_stage(call));
        if wants != call.sent_wants {
            let _ = call.commands.send(CallCommand::SetVideoWants(wants.clone()));
            call.sent_wants = wants;
        }
    }

    pub fn toggle_call_fullscreen(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.viewing) else {
            return;
        };
        call.fullscreen = match call.fullscreen {
            Some(_) => None,
            None => Some(FullscreenState::new(Instant::now())),
        };
        self.watch_fullscreen_chrome(cx);
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn escape_call_fullscreen(&mut self, cx: &mut Context<Self>) {
        if self.stage_fullscreen() {
            self.leave_stage_fullscreen(cx);
            return;
        }
        let Some(call) = self.call.as_mut().filter(|call| call.fullscreen.is_some()) else {
            return;
        };
        call.fullscreen = None;
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn call_fullscreen(&self) -> bool {
        self.call
            .as_ref()
            .is_some_and(|call| call.viewing && call.fullscreen.is_some())
    }

    pub fn touch_call_chrome(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(fullscreen) = self.call.as_mut().and_then(|call| call.fullscreen.as_mut()) else {
            return;
        };
        let was_visible = fullscreen.chrome_visible(now);
        if was_visible
            && now.saturating_duration_since(fullscreen.active_at) < CHROME_TOUCH_THROTTLE
        {
            return;
        }
        fullscreen.active_at = now;
        if !was_visible {
            self.watch_fullscreen_chrome(cx);
            cx.notify();
        }
    }

    pub fn set_call_chrome_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let Some(fullscreen) = self
            .call
            .as_mut()
            .and_then(|call| call.fullscreen.as_mut())
            .filter(|fullscreen| fullscreen.hovered != hovered)
        else {
            return;
        };
        fullscreen.hovered = hovered;
        fullscreen.active_at = Instant::now();
        self.watch_fullscreen_chrome(cx);
        cx.notify();
    }

    fn watch_fullscreen_chrome(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let call_id = call.id;
        let Some(fullscreen) = call
            .fullscreen
            .as_mut()
            .filter(|fullscreen| !fullscreen.timer_running)
        else {
            return;
        };
        fullscreen.timer_running = true;
        cx.spawn(async move |this, cx| {
            while let Ok(Some(wait)) =
                this.update(cx, |state, cx| state.fullscreen_tick(call_id, cx))
            {
                cx.background_executor().timer(wait).await;
            }
        })
        .detach();
    }

    fn fullscreen_tick(&mut self, call_id: u64, cx: &mut Context<Self>) -> Option<Duration> {
        cx.notify();
        let now = Instant::now();
        let fullscreen = self
            .call
            .as_mut()
            .filter(|call| call.id == call_id)
            .and_then(|call| call.fullscreen.as_mut())?;
        let hint_left =
            HINT_SHOWN.saturating_sub(now.saturating_duration_since(fullscreen.entered));
        let next = [
            fullscreen.next_change(now),
            (!hint_left.is_zero()).then_some(hint_left),
        ]
        .into_iter()
        .flatten()
        .min();
        if next.is_none() {
            fullscreen.timer_running = false;
        }
        next.map(|wait| wait + TIMER_SLACK)
    }

    pub fn note_call_chat_message(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        sender_id: Option<String>,
        sender_name: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.call.as_mut().filter(|call| {
            !call.chat_open
                && call.model.view.chat_bubbles
                && call.chat_thread() == Some(conversation_id)
        }) else {
            return;
        };
        push_bubble(
            &mut call.bubbles,
            ChatBubble {
                id: message_id.to_owned(),
                sender_id,
                name: sender_name.to_owned(),
                text: text.to_owned(),
                at: Instant::now(),
            },
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(BUBBLE_SHOWN + TIMER_SLACK)
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
        cx.notify();
    }

    pub fn set_call_share_menu(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.share_menu_open = open;
            cx.notify();
        }
    }

    pub fn set_call_lobby_list(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.lobby_list_open = open;
            cx.notify();
        }
    }

    pub fn toggle_call_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        if call.model.local_share.is_some() {
            self.stop_call_share(cx);
            return;
        }
        if call.model.can_share() {
            self.show_call(true, cx);
            self.set_call_share_menu(true, cx);
        }
    }

    pub fn open_call_lobby_list(&mut self, cx: &mut Context<Self>) {
        let waiting = self
            .call
            .as_ref()
            .is_some_and(|call| call.model.can_admit() && !call.model.lobby_guests().is_empty());
        if waiting {
            self.show_call(true, cx);
            self.set_call_lobby_list(true, cx);
        }
    }

    pub(super) fn queue_demo_bubbles(&mut self, thread: &str, cx: &mut Context<Self>) {
        let thread = thread.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEMO_BUBBLE_START).await;
            for (index, (name, text)) in DEMO_BUBBLES.iter().enumerate() {
                let sent = this.update(cx, |state, cx| {
                    state.note_call_chat_message(
                        &thread,
                        &format!("demo-bubble-{index}"),
                        Some(format!("demo-guest-{index}")),
                        name,
                        text,
                        cx,
                    )
                });
                if sent.is_err() {
                    return;
                }
                cx.background_executor().timer(DEMO_BUBBLE_EVERY).await;
            }
        })
        .detach();
    }
}
