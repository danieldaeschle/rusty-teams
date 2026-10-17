use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use calling::{ShareKind, ShareSource};
use gpui_kit::*;

use super::model::format_elapsed;
use super::overlay_layout::{
    BorderVisibility, MINI_POSITION_KEY, bar_collapsed, bar_rect, border_rects, border_thickness,
    border_visibility, format_position, mini_rect, mini_work_area, parse_position,
};
use super::share_overlay::{
    GALLERY_POSITION_KEY, GalleryLayout, LocalShare, Monitor, Surface, excluded_from_capture,
    gallery_available_height, gallery_candidates, gallery_layout, gallery_rect,
    minimize_on_share_start, overlay_plan,
};
use crate::app_state::AppState;
use crate::notify::platform::{self, NativeHandle, ScreenRect, ShareGeometry, WorkArea};
use crate::views::call_mini_window::MiniWindowView;
use crate::views::call_pop_out::{POP_OUT_SIZE, POP_OUT_TITLE, SharePopOutView};
use crate::views::call_share_gallery::ShareGalleryView;
use crate::views::share_bar::{ShareBarView, ShareBorderView};

const TICK: Duration = Duration::from_millis(100);
const IDLE_TICK: Duration = Duration::from_millis(250);
const SLOW_TICK_EVERY: u32 = 5;
const BORDER_SIDES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Role {
    Bar,
    Mini,
    Gallery,
    Border(usize),
}

struct Placed {
    handle: AnyWindowHandle,
    native: Option<NativeHandle>,
    rect: Option<ScreenRect>,
    anchor: Option<NativeHandle>,
    visible: bool,
}

impl Placed {
    fn place(&mut self, rect: ScreenRect, cx: &mut App) {
        if self.rect == Some(rect) {
            return;
        }
        self.rect = Some(rect);
        let Some(native) = self.native else {
            return;
        };
        match self.anchor {
            Some(_) => {
                platform::place_rect(native, rect.left, rect.top, rect.width(), rect.height())
            }
            None => platform::place(native, rect.left, rect.top, rect.width(), rect.height()),
        }
        // WM_SIZE arrives while the app is borrowed, so GPUI drops its own resize report.
        self.handle
            .update(cx, |_, window, cx| window.bounds_changed(cx))
            .ok();
    }

    fn set_visible(&mut self, visible: bool) {
        let Some(native) = self.native else {
            return;
        };
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible {
            platform::show_window_inactive(native);
        } else {
            platform::hide_window(native);
        }
    }
}

struct ShareSession {
    started: Instant,
    source: Option<ShareSource>,
    geometry: Option<ShareGeometry>,
    minimize_checked: bool,
}

pub struct CallWindows {
    app: Entity<AppState>,
    main_window: AnyWindowHandle,
    main_native: Option<NativeHandle>,
    work_area: Option<WorkArea>,
    minimized: bool,
    windows: HashMap<Role, Placed>,
    opening: HashSet<Role>,
    share: Option<ShareSession>,
    pop_out: Option<AnyWindowHandle>,
    pop_out_opening: bool,
    pinned: bool,
    bar_idle_since: Instant,
    bar_collapsed: bool,
    shown_second: u64,
    mini_saved: Option<(i32, i32)>,
    mini_seen: Option<(i32, i32)>,
    gallery_saved: Option<(i32, i32)>,
    gallery_seen: Option<(i32, i32)>,
    gallery_layout: GalleryLayout,
    speaker_only: bool,
    main_excluded: bool,
    minimized_for_share: bool,
    ticks: u32,
}

fn display_work_area(cx: &App) -> Option<WorkArea> {
    let bounds = cx.primary_display()?.bounds();
    let left = f32::from(bounds.origin.x) as i32;
    let top = f32::from(bounds.origin.y) as i32;
    Some(WorkArea {
        left,
        top,
        right: left + f32::from(bounds.size.width) as i32,
        bottom: top + f32::from(bounds.size.height) as i32,
        scale: 1.,
    })
}

fn overlay_options(role: Role, rect: ScreenRect, scale: f32) -> WindowOptions {
    let bounds = Bounds::new(
        point(px(rect.left as f32 / scale), px(rect.top as f32 / scale)),
        size(
            px(rect.width() as f32 / scale),
            px(rect.height() as f32 / scale),
        ),
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: matches!(role, Role::Mini | Role::Gallery),
        is_resizable: false,
        is_minimizable: false,
        window_background: match role {
            Role::Border(_) => WindowBackgroundAppearance::Opaque,
            Role::Bar | Role::Mini | Role::Gallery => WindowBackgroundAppearance::Transparent,
        },
        ..Default::default()
    }
}

impl CallWindows {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved_position = |key: &str, cx: &App| {
            app.read(cx)
                .store
                .meta(key)
                .ok()
                .flatten()
                .and_then(|text| parse_position(&text))
        };
        let mini_saved = saved_position(MINI_POSITION_KEY, cx);
        let gallery_saved = saved_position(GALLERY_POSITION_KEY, cx);
        let windows = CallWindows {
            app,
            main_window: window.window_handle(),
            main_native: platform::native_handle(window),
            work_area: None,
            minimized: false,
            windows: HashMap::new(),
            opening: HashSet::new(),
            share: None,
            pop_out: None,
            pop_out_opening: false,
            pinned: false,
            bar_idle_since: Instant::now(),
            bar_collapsed: false,
            shown_second: 0,
            mini_saved,
            mini_seen: None,
            gallery_saved,
            gallery_seen: None,
            gallery_layout: gallery_layout(0, true, f32::MAX),
            speaker_only: false,
            main_excluded: false,
            minimized_for_share: false,
            ticks: 0,
        };
        cx.spawn(async move |this, cx| {
            let mut pause = TICK;
            loop {
                cx.background_executor().timer(pause).await;
                match this.update(cx, |windows, cx| windows.tick(cx)) {
                    Ok(next_pause) => pause = next_pause,
                    Err(_) => break,
                }
            }
        })
        .detach();
        windows
    }

    pub fn app(&self) -> &Entity<AppState> {
        &self.app
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn bar_collapsed(&self) -> bool {
        self.bar_collapsed
    }

    pub fn gallery_layout(&self) -> GalleryLayout {
        self.gallery_layout
    }

    pub fn sharing(&self) -> bool {
        self.share.is_some()
    }

    pub fn toggle_speaker_only(&mut self, cx: &mut Context<Self>) {
        self.speaker_only = !self.speaker_only;
        cx.notify();
    }

    pub fn share_text(&self) -> String {
        let Some(session) = &self.share else {
            return String::new();
        };
        let what = match session.source.as_ref().map(|source| source.kind) {
            Some(ShareKind::Window) => "window",
            _ => "screen",
        };
        format!(
            "Sharing {what} {}",
            format_elapsed(session.started.elapsed())
        )
    }

    pub fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        self.pinned = !self.pinned;
        self.bar_idle_since = Instant::now();
        cx.notify();
    }

    pub fn wake_bar(&mut self, cx: &mut Context<Self>) {
        self.bar_idle_since = Instant::now();
        if self.bar_collapsed {
            self.bar_collapsed = false;
            cx.notify();
        }
    }

    pub fn return_to_call(&mut self, cx: &mut Context<Self>) {
        if let Some(native) = self.main_native {
            platform::restore_and_raise(native);
        }
        self.main_window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        self.app.update(cx, |state, cx| state.show_call(true, cx));
    }

    fn idle(&self, cx: &App) -> bool {
        self.app.read(cx).call.is_none()
            && self.share.is_none()
            && self.windows.is_empty()
            && self.opening.is_empty()
            && !self.main_excluded
    }

    fn tick(&mut self, cx: &mut Context<Self>) -> Duration {
        if self.idle(cx) {
            return IDLE_TICK;
        }
        self.ticks = self.ticks.wrapping_add(1);
        let slow = self.ticks % SLOW_TICK_EVERY == 0;
        self.refresh_main_window();
        self.sync_share(slow, cx);
        self.sync_capture_exclusion();
        self.sync_pop_out(cx);
        self.sync_monitor(cx);
        if slow {
            self.track_position(Role::Mini, cx);
            self.track_position(Role::Gallery, cx);
        }
        TICK
    }

    fn refresh_main_window(&mut self) {
        let Some(native) = self.main_native else {
            return;
        };
        let minimized = platform::window_minimized(native);
        if self.minimized && !minimized {
            self.minimized_for_share = false;
        }
        self.minimized = minimized;
        if !self.minimized {
            self.work_area = platform::work_area(native).or(self.work_area);
        }
    }

    fn local_share(&self, cx: &App) -> Option<Option<ShareSource>> {
        let model = &self.app.read(cx).call.as_ref()?.model;
        let label = model.local_share.as_ref()?;
        Some(
            model
                .share_sources
                .iter()
                .find(|source| &source.label() == label)
                .cloned(),
        )
    }

    fn sync_share(&mut self, refresh_geometry: bool, cx: &mut Context<Self>) {
        let Some(source) = self.local_share(cx) else {
            self.end_share(cx);
            return;
        };
        let mut refresh_geometry = refresh_geometry;
        if self.share.is_none() {
            self.bar_idle_since = Instant::now();
            self.share = Some(ShareSession {
                started: Instant::now(),
                source: None,
                geometry: None,
                minimize_checked: false,
            });
            refresh_geometry = true;
        }
        let Some(session) = self.share.as_mut() else {
            return;
        };
        session.source = source;
        let shared_window = session
            .source
            .as_ref()
            .filter(|source| source.kind == ShareKind::Window)
            .map(|source| source.id as NativeHandle);
        if refresh_geometry || shared_window.is_some() {
            session.geometry = session
                .source
                .as_ref()
                .and_then(|source| platform::share_geometry(source.kind, source.id));
        }
        let geometry = session.geometry;
        let elapsed_seconds = session.started.elapsed().as_secs();
        let check_minimize = !std::mem::replace(&mut session.minimize_checked, true);
        if check_minimize {
            self.minimize_for_share(geometry.map(|geometry| geometry.target), cx);
        }
        let Some(work) = geometry
            .map(|geometry| geometry.work)
            .or(self.work_area)
            .or_else(|| display_work_area(cx))
        else {
            return;
        };
        let can_collapse = self
            .windows
            .get(&Role::Bar)
            .is_some_and(|placed| placed.native.is_some());
        let collapsed = can_collapse && bar_collapsed(self.pinned, self.bar_idle_since.elapsed());
        if collapsed != self.bar_collapsed || elapsed_seconds != self.shown_second {
            self.bar_collapsed = collapsed;
            self.shown_second = elapsed_seconds;
            cx.notify();
        }
        self.ensure_window(Role::Bar, bar_rect(work, collapsed), work.scale, true, cx);
        match border_visibility(shared_window.is_some(), geometry.is_some()) {
            BorderVisibility::Show => {
                let thickness = border_thickness(work.scale);
                let rects = geometry.map(|geometry| border_rects(geometry.target, thickness));
                for (side, rect) in rects.into_iter().flatten().enumerate() {
                    if let Some(placed) = self.windows.get_mut(&Role::Border(side)) {
                        placed.anchor = shared_window;
                        placed.set_visible(true);
                    }
                    self.ensure_window(Role::Border(side), rect, work.scale, true, cx);
                }
                self.restack_borders(shared_window);
            }
            BorderVisibility::Hide => {
                for side in 0..BORDER_SIDES {
                    if let Some(placed) = self.windows.get_mut(&Role::Border(side)) {
                        placed.set_visible(false);
                    }
                }
            }
            BorderVisibility::Close => self.close_borders(cx),
        }
    }

    fn local_share_mode(&self) -> LocalShare {
        let kind = self
            .share
            .as_ref()
            .and_then(|session| session.source.as_ref())
            .map(|source| source.kind);
        LocalShare::of(kind, self.share.is_some())
    }

    fn minimize_for_share(&mut self, shared: Option<ScreenRect>, cx: &mut Context<Self>) {
        let Some(native) = self.main_native.filter(|_| !self.minimized) else {
            return;
        };
        if !minimize_on_share_start(
            self.local_share_mode(),
            platform::window_rect(native),
            shared,
        ) {
            return;
        }
        self.minimized_for_share = true;
        self.main_window
            .update(cx, |_, window, _| window.minimize_window())
            .ok();
    }

    fn sync_capture_exclusion(&mut self) {
        let Some(native) = self.main_native else {
            return;
        };
        let wanted = excluded_from_capture(Surface::Main, self.local_share_mode());
        if wanted == self.main_excluded {
            return;
        }
        self.main_excluded = wanted;
        if wanted {
            platform::exclude_from_capture(native);
        } else {
            platform::include_in_capture(native);
        }
    }

    fn end_share(&mut self, cx: &mut Context<Self>) {
        if self.share.take().is_none() {
            return;
        }
        if std::mem::take(&mut self.minimized_for_share) && self.minimized {
            self.return_to_call(cx);
        }
        self.bar_collapsed = false;
        self.close(Role::Bar, cx);
        self.close_borders(cx);
    }

    fn close_borders(&mut self, cx: &mut Context<Self>) {
        for side in 0..BORDER_SIDES {
            self.close(Role::Border(side), cx);
        }
    }

    fn sync_pop_out(&mut self, cx: &mut Context<Self>) {
        let wanted = self
            .app
            .read(cx)
            .call
            .as_ref()
            .is_some_and(|call| call.share_stage.popped && call.model.screen_sharer.is_some());
        match (wanted, self.pop_out) {
            (true, None) => self.open_pop_out(cx),
            (false, Some(handle)) => {
                self.pop_out = None;
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
            (true, Some(handle)) => {
                if handle.update(cx, |_, _, _| ()).is_err() {
                    self.pop_out = None;
                    self.app
                        .update(cx, |state, cx| state.return_stage_from_pop_out(cx));
                }
            }
            (false, None) => {}
        }
    }

    fn open_pop_out(&mut self, cx: &mut Context<Self>) {
        if self.pop_out_opening {
            return;
        }
        self.pop_out_opening = true;
        let controller = cx.entity();
        let app = self.app.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(POP_OUT_SIZE.0), px(POP_OUT_SIZE.1)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(POP_OUT_TITLE.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        // open_window draws right away, and the view reads the app entity while rendering.
        cx.defer(move |cx| {
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| SharePopOutView::new(app, window, cx))
            });
            controller.update(cx, |this, _| {
                this.pop_out_opening = false;
                this.pop_out = opened.ok().map(|(handle, _)| handle);
            });
        });
    }

    fn sync_monitor(&mut self, cx: &mut Context<Self>) {
        let in_call = self.app.read(cx).call.is_some();
        let plan = overlay_plan(
            in_call,
            self.local_share_mode(),
            self.minimized,
            self.speaker_only,
        );
        let work = self.work_area.or_else(|| display_work_area(cx));
        match (plan.monitor, work) {
            (Monitor::Mini, Some(main_work)) => {
                self.close(Role::Gallery, cx);
                let work = mini_work_area(main_work, self.mini_saved, platform::work_area_at);
                let rect = mini_rect(work, self.mini_saved);
                self.ensure_window(Role::Mini, rect, work.scale, false, cx);
            }
            (Monitor::Gallery, Some(main_work)) => {
                self.close(Role::Mini, cx);
                self.place_gallery(main_work, cx);
            }
            _ => {
                self.close(Role::Mini, cx);
                self.close(Role::Gallery, cx);
            }
        }
    }

    fn place_gallery(&mut self, main_work: WorkArea, cx: &mut Context<Self>) {
        let work = mini_work_area(main_work, self.gallery_saved, platform::work_area_at);
        let (others, self_tile) = self.app.read(cx).call.as_ref().map_or((0, true), |call| {
            (
                gallery_candidates(&call.model).len(),
                !call.model.view.hide_me,
            )
        });
        let layout = gallery_layout(others, self_tile, gallery_available_height(work));
        if layout != self.gallery_layout {
            self.gallery_layout = layout;
            cx.notify();
        }
        let origin = self
            .windows
            .get(&Role::Gallery)
            .and_then(|placed| placed.native)
            .and_then(platform::window_rect)
            .map(|rect| (rect.left, rect.top))
            .or(self.gallery_saved);
        let rect = gallery_rect(work, layout, origin);
        match self.windows.get_mut(&Role::Gallery) {
            Some(placed) => {
                let size = |rect: ScreenRect| (rect.width(), rect.height());
                if placed.rect.map(size) != Some(size(rect)) {
                    placed.place(rect, cx);
                }
            }
            None => self.open(Role::Gallery, rect, work.scale, cx),
        }
    }

    fn track_position(&mut self, role: Role, cx: &mut Context<Self>) {
        let Some(native) = self.windows.get(&role).and_then(|placed| placed.native) else {
            return;
        };
        let Some(rect) = platform::window_rect(native) else {
            return;
        };
        let (key, saved, seen) = match role {
            Role::Mini => (MINI_POSITION_KEY, &mut self.mini_saved, &mut self.mini_seen),
            Role::Gallery => (
                GALLERY_POSITION_KEY,
                &mut self.gallery_saved,
                &mut self.gallery_seen,
            ),
            Role::Bar | Role::Border(_) => return,
        };
        let position = (rect.left, rect.top);
        let settled = *seen == Some(position);
        *seen = Some(position);
        if settled && *saved != Some(position) {
            *saved = Some(position);
            let _ = self
                .app
                .read(cx)
                .store
                .set_meta(key, &format_position(position.0, position.1));
        }
    }

    fn restack_borders(&self, shared_window: Option<NativeHandle>) {
        let Some(shared) = shared_window else {
            return;
        };
        let group: Vec<NativeHandle> = (0..BORDER_SIDES)
            .filter_map(|side| self.windows.get(&Role::Border(side))?.native)
            .collect();
        platform::stack_group_above(shared, &group);
    }

    fn ensure_window(
        &mut self,
        role: Role,
        rect: ScreenRect,
        scale: f32,
        follow: bool,
        cx: &mut Context<Self>,
    ) {
        match self.windows.get_mut(&role) {
            Some(placed) => {
                if placed.rect.is_none() || follow && placed.rect != Some(rect) {
                    placed.place(rect, cx);
                }
            }
            None => self.open(role, rect, scale, cx),
        }
    }

    fn close(&mut self, role: Role, cx: &mut Context<Self>) {
        if let Some(placed) = self.windows.remove(&role) {
            placed
                .handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }

    fn open(&mut self, role: Role, rect: ScreenRect, scale: f32, cx: &mut Context<Self>) {
        if !self.opening.insert(role) {
            return;
        }
        let controller = cx.entity();
        let options = overlay_options(role, rect, scale);
        // open_window draws right away, and the views read this entity while rendering.
        cx.defer(move |cx| {
            let view_controller = controller.clone();
            let opened = cx.open_window(options, move |window, cx| {
                let view: AnyView = match role {
                    Role::Bar => cx.new(|cx| ShareBarView::new(view_controller, cx)).into(),
                    Role::Mini => cx.new(|cx| MiniWindowView::new(view_controller, cx)).into(),
                    Role::Gallery => cx
                        .new(|cx| ShareGalleryView::new(view_controller, cx))
                        .into(),
                    Role::Border(_) => cx.new(|_| ShareBorderView).into(),
                };
                cx.new(|cx| base::Root::new(view, window, cx).bg(transparent_black()))
            });
            let placed = opened.ok().map(|handle| {
                let handle = AnyWindowHandle::from(handle);
                let native = handle
                    .update(cx, |_, window, _| platform::native_handle(window))
                    .ok()
                    .flatten();
                if let Some(native) = native {
                    platform::prepare_toast_window(native);
                    platform::exclude_from_capture(native);
                }
                if matches!(role, Role::Border(_)) {
                    handle
                        .update(cx, |_, window, _| {
                            platform::make_window_click_through(window)
                        })
                        .ok();
                }
                Placed {
                    handle,
                    native,
                    rect: None,
                    anchor: None,
                    visible: true,
                }
            });
            controller.update(cx, |this, _| {
                this.opening.remove(&role);
                if let Some(placed) = placed {
                    this.windows.insert(role, placed);
                }
            });
        });
    }
}
