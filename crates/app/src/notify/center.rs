use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use chrono::Utc;
use gpui_kit::*;
use store::Sidebar;

use super::badge::{Badge, badge_for};
use super::incoming::IncomingTracker;
use super::layout::{self, Slot};
use super::platform::{self, NativeHandle, Tray, TrayCommand, WorkArea};
use super::rules::{
    ChatKind, Decision, Environment, Incoming, Preview, Settings, SoundThrottle, decide,
    should_flash,
};
use super::settings;
use super::settings_view::SettingsView;
use super::stack::{
    FADE_IN, FAST_FADE, ReplyState, SENT_DURATION, SLOW_FADE, ToastModel, ToastStack,
};
use super::text::describe;
use super::toast::{PillView, ToastView};
use crate::app_state::{AppEvent, AppState, Selection};
use crate::data::{self, Directory, PresenceKind};
use crate::runtime;

const TICK: Duration = Duration::from_millis(33);
const SLIDE_DURATION: Duration = Duration::from_millis(180);
const SLIDE_DISTANCE: f32 = 48.;
const DEMO_SPEC_DELAY: Duration = Duration::from_secs(3);

struct OpenWindow {
    handle: AnyWindowHandle,
    native: Option<NativeHandle>,
    opened: Instant,
}

impl OpenWindow {
    fn place(&self, native: NativeHandle, slot: Slot, x: i32, cx: &mut App) {
        platform::place(native, x, slot.y, slot.width, slot.height);
        // WM_SIZE arrives while the app is borrowed, so GPUI drops its own resize report.
        self.handle
            .update(cx, |_, window, cx| window.bounds_changed(cx))
            .ok();
    }
}

pub struct NotificationCenter {
    app: Entity<AppState>,
    main_window: AnyWindowHandle,
    main_native: Option<NativeHandle>,
    settings: Settings,
    stack: ToastStack,
    tracker: IncomingTracker,
    throttle: SoundThrottle,
    windows: HashMap<u64, OpenWindow>,
    opening: HashSet<u64>,
    pill: Option<OpenWindow>,
    pill_opening: bool,
    hover_hold: bool,
    settings_window: Option<AnyWindowHandle>,
    settings_opening: bool,
    unread_mentions: HashSet<String>,
    tray: Option<Tray>,
    shown_badge: Option<Badge>,
    animations: bool,
    _subscription: Subscription,
}

impl NotificationCenter {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = settings::load(&app.read(cx).store);
        let tray = if app.read(cx).mode.demo {
            None
        } else {
            Tray::new(settings.sound, settings.do_not_disturb)
        };
        let subscription = cx.subscribe(&app, |this, _, event: &AppEvent, cx| {
            this.on_app_event(event, cx);
        });
        let center = NotificationCenter {
            main_window: window.window_handle(),
            main_native: platform::native_handle(window),
            settings,
            stack: ToastStack::default(),
            tracker: IncomingTracker::new(Utc::now()),
            throttle: SoundThrottle::default(),
            windows: HashMap::new(),
            opening: HashSet::new(),
            pill: None,
            pill_opening: false,
            hover_hold: false,
            settings_window: None,
            settings_opening: false,
            unread_mentions: HashSet::new(),
            tray,
            shown_badge: None,
            animations: platform::animations_enabled(),
            app,
            _subscription: subscription,
        };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |center, cx| center.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        if center.app.read(cx).mode.demo {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(DEMO_SPEC_DELAY).await;
                this.update(cx, |center, cx| center.show_demo_toasts(cx))
                    .ok();
            })
            .detach();
        }
        center
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn model(&self, id: u64) -> Option<&ToastModel> {
        self.stack.get(id)
    }

    pub fn queued_count(&self) -> usize {
        self.stack.queued_count()
    }

    pub fn stack_opacity(&self) -> f32 {
        let now = Instant::now();
        self.stack
            .visible()
            .iter()
            .map(|toast| toast.opacity(now))
            .fold(0., f32::max)
    }

    fn animated(&self, duration: Duration) -> Duration {
        if self.animations { duration } else { Duration::ZERO }
    }

    pub fn directory<'a>(&self, cx: &'a App) -> &'a Directory {
        &self.app.read(cx).directory
    }

    pub fn reply_text(&self, id: u64) -> Option<String> {
        self.stack.get(id).map(|toast| toast.reply_text.clone())
    }

    pub fn intercept_close(&self, _cx: &App) -> bool {
        match (self.settings.close_to_tray, &self.tray, self.main_native) {
            (true, Some(_), Some(native)) => {
                platform::hide_window(native);
                true
            }
            _ => false,
        }
    }

    pub fn update_settings(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);
        settings::save(&self.app.read(cx).store, &self.settings);
        self.sync_tray();
        cx.notify();
    }

    fn on_app_event(&mut self, event: &AppEvent, cx: &mut Context<Self>) {
        match event {
            AppEvent::Messages(conversation_id) => self.on_messages(conversation_id, cx),
            AppEvent::Selection => self.on_selection(cx),
            AppEvent::Sidebar => self.refresh_badge(cx),
            _ => {}
        }
    }

    fn on_selection(&mut self, cx: &mut Context<Self>) {
        let selected = self
            .app
            .read(cx)
            .selection
            .as_ref()
            .map(|selection| selection.conversation_id().to_owned());
        if let Some(conversation_id) = selected {
            self.unread_mentions.remove(&conversation_id);
            self.stack.remove_conversation(&conversation_id);
        }
        self.refresh_badge(cx);
    }

    fn on_messages(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let my_user_id = state.directory.me.as_ref().map(|me| me.user_id.clone());
        let found = self.tracker.collect(
            &state.store,
            &state.sidebar,
            &state.followed_channels,
            my_user_id.as_deref(),
            conversation_id,
        );
        for incoming in found {
            self.handle_incoming(incoming, cx);
        }
        self.refresh_badge(cx);
    }

    fn environment(&self, conversation_id: &str, cx: &mut Context<Self>) -> Environment {
        let state = self.app.read(cx);
        let selected = state
            .selection
            .as_ref()
            .is_some_and(|selection| selection.conversation_id() == conversation_id);
        let own_do_not_disturb = state.directory.me.as_ref().is_some_and(|me| {
            state.directory.presence_of(&me.user_id).kind() == PresenceKind::DoNotDisturb
        });
        let in_call = match (state.engine.as_ref(), state.directory.me.as_ref()) {
            (Some(engine), Some(me)) => data::in_call(engine, &me.user_id),
            _ => false,
        };
        Environment {
            chat_in_foreground: selected && self.main_window_active(cx),
            system_quiet: platform::system_quiet(),
            own_do_not_disturb,
            in_call,
        }
    }

    fn main_window_active(&self, cx: &mut Context<Self>) -> bool {
        self.main_window
            .update(cx, |_, window, _| window.is_window_active())
            .unwrap_or(false)
    }

    pub fn handle_incoming(&mut self, incoming: Incoming, cx: &mut Context<Self>) {
        let environment = self.environment(&incoming.conversation_id, cx);
        if !environment.chat_in_foreground && incoming.mentions_me {
            self.unread_mentions
                .insert(incoming.conversation_id.clone());
        }
        let decision = decide(&self.settings, &incoming, environment);
        self.present(&incoming, decision, cx);
        if should_flash(
            &self.settings,
            decision.toast,
            self.main_window_active(cx),
        ) && let Some(native) = self.main_native
        {
            platform::flash(native);
        }
    }

    fn present(&mut self, incoming: &Incoming, decision: Decision, cx: &mut Context<Self>) {
        if !decision.toast {
            return;
        }
        let now = Instant::now();
        let outcome = self.stack.push(incoming, now);
        if let Some(model) = self.stack.get_mut(outcome.id) {
            model.time = chrono::Local::now().format("%H:%M").to_string();
        }
        let throttle_allows = self.throttle.allow(&incoming.conversation_id, now);
        if decision.sound && outcome.plays_sound(throttle_allows) && !self.app.read(cx).mode.demo {
            platform::play_sound();
        }
        if let Some(user_id) = incoming.sender_id.clone() {
            self.app
                .update(cx, |state, cx| state.request_avatars(vec![user_id], cx));
        }
        self.sync_windows(cx);
    }

    fn visible_ids(&self) -> Vec<u64> {
        self.stack.visible().iter().map(|toast| toast.id).collect()
    }

    fn work_area(&self, cx: &App) -> WorkArea {
        if let Some(area) = self.main_native.and_then(platform::work_area) {
            return area;
        }
        let bounds = cx
            .primary_display()
            .map(|display| display.bounds())
            .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(720.))));
        WorkArea {
            left: f32::from(bounds.origin.x) as i32,
            top: f32::from(bounds.origin.y) as i32,
            right: f32::from(bounds.origin.x + bounds.size.width) as i32,
            bottom: f32::from(bounds.origin.y + bounds.size.height) as i32,
            scale: 1.,
        }
    }

    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        let visible = self.visible_ids();
        let stale: Vec<u64> = self
            .windows
            .keys()
            .filter(|id| !visible.contains(id))
            .copied()
            .collect();
        for id in stale {
            self.close_window(id, cx);
        }
        let area = self.work_area(cx);
        let heights: Vec<f32> = self
            .stack
            .visible()
            .iter()
            .map(|toast| {
                let lines = preview_lines(&describe(toast, self.settings.preview).preview, cx);
                layout::toast_height(toast, lines)
            })
            .collect();
        let with_hide_all = visible.len() > 1 || self.stack.queued_count() > 0;
        let slots = layout::stack_slots(area, self.settings.corner, &heights, with_hide_all);
        for (id, slot) in visible.iter().zip(slots.toasts.iter()) {
            self.place_toast(*id, *slot, area, cx);
        }
        self.sync_pill(slots.pill, area, cx);
    }

    fn slide_offset(&self, opened: Instant, area: WorkArea) -> i32 {
        if !self.animations {
            return 0;
        }
        let progress = (opened.elapsed().as_secs_f32() / SLIDE_DURATION.as_secs_f32()).min(1.);
        let eased = 1. - (1. - progress).powi(3);
        let distance = ((1. - eased) * SLIDE_DISTANCE * area.scale) as i32;
        if self.settings.corner.is_right() {
            distance
        } else {
            -distance
        }
    }

    fn place_toast(&mut self, id: u64, slot: Slot, area: WorkArea, cx: &mut Context<Self>) {
        if !self.windows.contains_key(&id) {
            self.open_toast_window(id, slot, area, cx);
            return;
        }
        let offset = self
            .windows
            .get(&id)
            .map(|window| self.slide_offset(window.opened, area))
            .unwrap_or(0);
        if let Some(window) = self.windows.get(&id)
            && let Some(native) = window.native
        {
            window.place(native, slot, slot.x + offset, cx);
        }
    }

    fn open_toast_window(&mut self, id: u64, slot: Slot, area: WorkArea, cx: &mut Context<Self>) {
        if !self.opening.insert(id) {
            return;
        }
        open_popup(
            popup_options(slot, area),
            cx,
            move |center, window, cx| cx.new(|cx| ToastView::new(center, id, window, cx)),
            move |this, window| {
                this.opening.remove(&id);
                if let Some(window) = window {
                    this.windows.insert(id, window);
                }
            },
        );
    }

    fn sync_pill(
        &mut self,
        slot: Option<Slot>,
        area: WorkArea,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = slot else {
            if let Some(pill) = self.pill.take() {
                pill.handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
            return;
        };
        if self.pill.is_none() && !self.pill_opening {
            self.pill_opening = true;
            open_popup(
                popup_options(slot, area),
                cx,
                |center, _, cx| cx.new(|cx| PillView::new(center, cx)),
                |this, window| {
                    this.pill_opening = false;
                    this.pill = window;
                },
            );
        }
        if let Some(pill) = self.pill.as_ref()
            && let Some(native) = pill.native
        {
            pill.place(native, slot, slot.x, cx);
        }
    }

    fn close_window(&mut self, id: u64, cx: &mut Context<Self>) {
        let closed = self.windows.get(&id).is_some_and(|window| {
            window
                .handle
                .update(cx, |_, window, _| window.remove_window())
                .is_ok()
        });
        if closed {
            self.windows.remove(&id);
        }
    }

    fn drop_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        let fast = self.animated(FAST_FADE);
        self.stack.dismiss(id, Instant::now(), fast);
        cx.notify();
    }

    pub fn hide_all(&mut self, cx: &mut Context<Self>) {
        for id in self.stack.ids() {
            self.drop_toast(id, cx);
        }
    }

    fn sync_hover_hold(&mut self, now: Instant) {
        let any_hovered = self.stack.any_hovered();
        if !any_hovered && !self.hover_hold {
            return;
        }
        if any_hovered {
            let fast = self.animated(FAST_FADE);
            self.stack.restore(now, fast);
        }
        for id in self.stack.ids() {
            if let Some(toast) = self.stack.get_mut(id)
                && !toast.queued
            {
                if any_hovered {
                    toast.timer.pause(now);
                } else if !toast.holds_open() {
                    toast.timer.resume(now);
                }
            }
        }
        self.hover_hold = any_hovered;
    }

    fn is_dismissed(&self, id: u64) -> bool {
        self.stack.get(id).is_none_or(|toast| toast.dismissed)
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let (fade_in, slow) = (self.animated(FADE_IN), self.animated(SLOW_FADE));
        for id in self.stack.expired(now) {
            self.stack.fade_out(id, now, slow);
        }
        for id in self.stack.gone(now) {
            self.stack.remove(id);
        }
        self.stack.promote(now, fade_in);
        self.sync_hover_hold(now);
        if !self.stack.is_empty() || !self.windows.is_empty() || self.pill.is_some() {
            self.sync_windows(cx);
            cx.notify();
        }
        self.poll_tray(cx);
    }

    fn poll_tray(&mut self, cx: &mut Context<Self>) {
        let commands = self.tray.as_ref().map(Tray::poll).unwrap_or_default();
        for command in commands {
            match command {
                TrayCommand::Toggle => self.toggle_main_window(cx),
                TrayCommand::Open => self.raise_main_window(cx),
                TrayCommand::ToggleSound => {
                    self.update_settings(cx, |settings| settings.sound = !settings.sound)
                }
                TrayCommand::ToggleDoNotDisturb => self.update_settings(cx, |settings| {
                    settings.do_not_disturb = !settings.do_not_disturb
                }),
                TrayCommand::Settings => self.open_settings(cx),
                TrayCommand::Quit => cx.quit(),
            }
        }
    }

    fn toggle_main_window(&mut self, cx: &mut Context<Self>) {
        if self.main_window_active(cx) {
            if let Some(native) = self.main_native {
                platform::hide_window(native);
            }
        } else {
            self.raise_main_window(cx);
        }
    }

    fn raise_main_window(&mut self, cx: &mut Context<Self>) {
        if let Some(native) = self.main_native {
            platform::restore_and_raise(native);
        }
        self.main_window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    }

    fn sync_tray(&self) {
        if let Some(tray) = &self.tray {
            tray.sync(
                self.settings.sound,
                self.settings.do_not_disturb,
                !self.unread_mentions.is_empty(),
            );
        }
    }

    fn refresh_badge(&mut self, cx: &mut Context<Self>) {
        let (count, still_unread) = unread_summary(&self.app.read(cx).sidebar);
        self.unread_mentions
            .retain(|conversation_id| still_unread.contains(conversation_id));
        let badge = badge_for(count);
        if badge == self.shown_badge {
            return;
        }
        if let Some(native) = self.main_native {
            platform::set_badge(native, badge.as_ref());
            if badge.is_none() && self.shown_badge.is_some() {
                platform::stop_flash(native);
            }
        }
        self.shown_badge = badge;
        self.sync_tray();
    }

    pub fn set_hover(&mut self, id: u64, hovered: bool, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(toast) = self.stack.get_mut(id) else {
            return;
        };
        if toast.hovered == hovered {
            return;
        }
        toast.hovered = hovered;
        self.sync_hover_hold(now);
        cx.notify();
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.drop_toast(id, cx);
    }

    pub fn open_reply(&mut self, id: u64, cx: &mut Context<Self>) -> Option<String> {
        let now = Instant::now();
        let toast = self.stack.get_mut(id)?;
        toast.reply = ReplyState::Open;
        toast.timer.pause(now);
        let draft = toast.reply_text.clone();
        cx.notify();
        Some(draft)
    }

    pub fn close_reply(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(toast) = self.stack.get_mut(id) {
            toast.reply = ReplyState::Closed;
            toast.reply_text.clear();
            toast.hovered = false;
        }
        cx.notify();
    }

    pub fn submit_reply(&mut self, id: u64, text: String, cx: &mut Context<Self>) {
        let Some(toast) = self.stack.get_mut(id) else {
            return;
        };
        toast.reply_text = text.clone();
        let conversation_id = toast.conversation_id.clone();
        let message_id = toast.message_id.clone();
        let is_channel = matches!(toast.kind, ChatKind::Channel { .. });
        let state = self.app.read(cx);
        let (mode, engine) = (state.mode, state.engine.clone());
        let outcome: Result<(), ()> = if mode.demo {
            Ok(())
        } else if mode.read_only {
            Err(())
        } else if let Some(engine) = engine {
            let receiver = runtime::spawn(async move {
                if is_channel {
                    engine.reply_to(&conversation_id, &message_id, &text).await
                } else {
                    engine.send_message(&conversation_id, &text, None).await
                }
                .map(|_| ())
            });
            cx.spawn(async move |this, cx| {
                let sent = matches!(receiver.await, Ok(Ok(())));
                this.update(cx, |center, cx| center.finish_reply(id, sent, cx))
                    .ok();
            })
            .detach();
            return;
        } else {
            Err(())
        };
        self.finish_reply(id, outcome.is_ok(), cx);
    }

    fn finish_reply(&mut self, id: u64, sent: bool, cx: &mut Context<Self>) {
        let Some(toast) = self.stack.get_mut(id) else {
            return;
        };
        toast.reply = if sent {
            ReplyState::Sent
        } else {
            ReplyState::Failed
        };
        let conversation_id = toast.conversation_id.clone();
        cx.notify();
        if sent {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SENT_DURATION).await;
                this.update(cx, |center, cx| {
                    center.mark_conversation_read(&conversation_id, cx);
                    center.drop_toast(id, cx);
                })
                .ok();
            })
            .detach();
        }
    }

    fn mark_conversation_read(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        self.unread_mentions.remove(conversation_id);
        let conversation_id = conversation_id.to_owned();
        self.app
            .update(cx, |state, cx| state.mark_chat_read(&conversation_id, cx));
        self.refresh_badge(cx);
    }

    pub fn mark_read(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.is_dismissed(id) {
            return;
        }
        let Some(conversation_id) = self
            .stack
            .get(id)
            .map(|toast| toast.conversation_id.clone())
        else {
            return;
        };
        self.mark_conversation_read(&conversation_id, cx);
        self.drop_toast(id, cx);
    }

    pub fn activate(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.is_dismissed(id) {
            return;
        }
        let Some((conversation_id, message_id)) = self
            .stack
            .get(id)
            .map(|toast| (toast.conversation_id.clone(), toast.message_id.clone()))
        else {
            return;
        };
        self.drop_toast(id, cx);
        self.raise_main_window(cx);
        let selection = selection_for(&self.app.read(cx).sidebar, &conversation_id);
        if let Some(selection) = selection {
            self.app.update(cx, |state, cx| {
                state.jump_to_message(selection, message_id, cx)
            });
        }
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.settings_window = None;
        }
        if self.settings_opening {
            return;
        }
        self.settings_opening = true;
        let center = cx.entity();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(SettingsView::WIDTH), px(SettingsView::HEIGHT)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Notifications".into()),
                ..Default::default()
            }),
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        };
        // open_window draws once right away, and SettingsView reads this entity while rendering.
        cx.defer(move |cx| {
            let view_center = center.clone();
            let opened = gpui_kit::open_window(options, cx, move |_, cx| {
                cx.new(|cx| SettingsView::new(view_center, cx))
            });
            center.update(cx, |this, _| {
                this.settings_opening = false;
                this.settings_window = opened.ok().map(|(handle, _)| handle);
            });
        });
    }

    pub fn send_test_notification(&mut self, cx: &mut Context<Self>) {
        let incoming = Incoming {
            conversation_id: "test-notification".to_owned(),
            message_id: "test".to_owned(),
            kind: ChatKind::Direct,
            chat_title: "Mara Lindqvist".to_owned(),
            sender_id: None,
            sender_name: "Mara Lindqvist".to_owned(),
            preview: Preview::Text("This is a test notification.".to_owned()),
            mentions_me: false,
            muted: false,
            signals: Default::default(),
            created_at: Utc::now(),
        };
        let decision = Decision {
            toast: true,
            sound: self.settings.sound,
        };
        self.throttle = SoundThrottle::default();
        self.present(&incoming, decision, cx);
    }

    fn show_demo_toasts(&mut self, cx: &mut Context<Self>) {
        for incoming in demo_incoming() {
            let decision = Decision {
                toast: true,
                sound: false,
            };
            self.present(&incoming, decision, cx);
        }
    }
}

/// Opens a window outside the current update: `open_window` draws right away, and every
/// notification view reads the center while rendering.
fn open_popup<V: Render>(
    options: WindowOptions,
    cx: &mut Context<NotificationCenter>,
    build: impl FnOnce(Entity<NotificationCenter>, &mut Window, &mut App) -> Entity<V> + 'static,
    opened: impl FnOnce(&mut NotificationCenter, Option<OpenWindow>) + 'static,
) {
    let center = cx.entity();
    cx.defer(move |cx| {
        let view_center = center.clone();
        let Ok(handle) = cx.open_window(options, move |window, cx| {
            let view = build(view_center, window, cx);
            cx.new(|cx| base::Root::new(view, window, cx).bg(transparent_black()))
        }) else {
            center.update(cx, |this, _| opened(this, None));
            return;
        };
        let handle = AnyWindowHandle::from(handle);
        let native = handle
            .update(cx, |_, window, _| platform::native_handle(window))
            .ok()
            .flatten();
        if let Some(native) = native {
            platform::prepare_toast_window(native);
        }
        let window = OpenWindow {
            handle,
            native,
            opened: Instant::now(),
        };
        center.update(cx, |this, _| opened(this, Some(window)));
    });
}

fn preview_lines(text: &str, cx: &App) -> usize {
    let mut wrapper = cx
        .text_system()
        .line_wrapper(font(crate::theme::font_family()), px(layout::PREVIEW_SIZE));
    text.split('\n')
        .map(|line| {
            let fragments = [LineFragment::text(line)];
            wrapper
                .wrap_line(&fragments, px(layout::PREVIEW_WIDTH), IndentAdjustment::NoIndent)
                .count()
                + 1
        })
        .sum()
}

fn popup_options(slot: Slot, area: WorkArea) -> WindowOptions {
    let scale = area.scale;
    let bounds = Bounds::new(
        point(px(slot.x as f32 / scale), px(slot.y as f32 / scale)),
        size(
            px(slot.width as f32 / scale),
            px(slot.height as f32 / scale),
        ),
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

pub fn selection_for(sidebar: &Sidebar, conversation_id: &str) -> Option<Selection> {
    if sidebar.chats.iter().any(|chat| chat.id == conversation_id) {
        return Some(Selection::Chat(conversation_id.to_owned()));
    }
    sidebar
        .teams
        .iter()
        .flat_map(|team| team.channels.iter())
        .any(|channel| channel.id == conversation_id)
        .then(|| Selection::Channel(conversation_id.to_owned()))
}

pub fn unread_summary(sidebar: &Sidebar) -> (usize, HashSet<String>) {
    let chats = sidebar.chats.iter().filter(|chat| chat.unread);
    let channels = sidebar
        .teams
        .iter()
        .flat_map(|team| team.channels.iter())
        .filter(|channel| channel.unread);
    let ids: HashSet<String> = chats
        .map(|chat| chat.id.clone())
        .chain(channels.map(|channel| channel.id.clone()))
        .collect();
    (ids.len(), ids)
}

fn demo_incoming() -> Vec<Incoming> {
    let item = |conversation: &str,
                kind: ChatKind,
                title: &str,
                sender: &str,
                text: &str,
                mention: bool| Incoming {
        conversation_id: conversation.to_owned(),
        message_id: format!("{conversation}-message"),
        kind,
        chat_title: title.to_owned(),
        sender_id: None,
        sender_name: sender.to_owned(),
        preview: Preview::Text(text.to_owned()),
        mentions_me: mention,
        muted: false,
        signals: Default::default(),
        created_at: Utc::now(),
    };
    vec![
        item(
            "demo-toast-direct",
            ChatKind::Direct,
            "Tobias Klein",
            "Tobias Klein",
            "Sounds good, I will look at it tomorrow.",
            false,
        ),
        item(
            "demo-toast-group",
            ChatKind::Group { member_count: 5 },
            "Retro-Team",
            "Priya Nair",
            "Please add topics by 5 pm.",
            false,
        ),
        item(
            "demo-toast-mention",
            ChatKind::Channel {
                team: "Platform".to_owned(),
                channel: "Releases".to_owned(),
            },
            "Releases",
            "Mara Lindqvist",
            "@Jonas can you approve the merge? The pipeline is waiting.",
            true,
        ),
        item(
            "demo-toast-lea",
            ChatKind::Direct,
            "Lea Schneider",
            "Lea Schneider",
            "Lunch at 12?",
            false,
        ),
        item(
            "demo-toast-jonas",
            ChatKind::Direct,
            "Jonas Ortega",
            "Jonas Ortega",
            "Deploy window moved to Thursday 6 pm.",
            false,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use store::{ChannelRecord, ChatRecord, SidebarTeam, TeamRecord};

    use super::{preview_lines, selection_for, unread_summary};
    use crate::app_state::Selection;
    use store::Sidebar;

    fn sidebar() -> Sidebar {
        Sidebar {
            chats: vec![
                ChatRecord {
                    id: "a".into(),
                    unread: true,
                    ..Default::default()
                },
                ChatRecord {
                    id: "b".into(),
                    unread: false,
                    ..Default::default()
                },
            ],
            teams: vec![SidebarTeam {
                team: TeamRecord {
                    id: "t".into(),
                    name: "Team".into(),
                },
                channels: vec![ChannelRecord {
                    id: "ch".into(),
                    team_id: "t".into(),
                    name: "General".into(),
                    membership_type: None,
                    last_message_at: None,
                    unread: true,
                }],
                hidden: false,
                hidden_channel_ids: Vec::new(),
            }],
        }
    }

    #[test]
    fn unread_summary_counts_chats_and_channels() {
        let (count, ids) = unread_summary(&sidebar());
        assert_eq!(count, 2);
        assert!(ids.contains("a") && ids.contains("ch"));
    }

    #[test]
    fn selection_kind_follows_sidebar() {
        assert_eq!(
            selection_for(&sidebar(), "a"),
            Some(Selection::Chat("a".into()))
        );
        assert_eq!(
            selection_for(&sidebar(), "ch"),
            Some(Selection::Channel("ch".into()))
        );
        assert_eq!(selection_for(&sidebar(), "x"), None);
    }

    #[gpui_kit::test]
    fn preview_lines_follow_the_wrapped_text(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            assert_eq!(preview_lines("ok", cx), 1);
            assert_eq!(preview_lines("a\nb", cx), 2);
            let long = "Cristina: ok, thanks for the info! I think not now ".repeat(3);
            assert!(preview_lines(&long, cx) >= 2);
        });
    }
}
