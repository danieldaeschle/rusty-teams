use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use futures_channel::oneshot;
use gpui_kit::*;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use crate::appearance::DEFAULT_ZOOM;

struct Shared {
    factor: Cell<f32>,
    executor: ForegroundExecutor,
    windows: RefCell<Vec<Weak<WindowState>>>,
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

pub fn factor() -> f32 {
    SHARED.with(|shared| {
        shared
            .borrow()
            .as_ref()
            .map_or(DEFAULT_ZOOM, |shared| shared.factor.get())
    })
}

pub fn set_factor(factor: f32) {
    let Some(shared) = SHARED.with(|shared| shared.borrow().clone()) else {
        return;
    };
    if (shared.factor.get() - factor).abs() < f32::EPSILON {
        return;
    }
    shared.factor.set(factor);
    let windows: Vec<Rc<WindowState>> = shared
        .windows
        .borrow_mut()
        .drain(..)
        .filter_map(|window| window.upgrade())
        .collect();
    shared
        .windows
        .borrow_mut()
        .extend(windows.iter().map(Rc::downgrade));
    shared
        .executor
        .spawn(async move {
            for window in windows {
                window.relayout();
            }
        })
        .detach();
}

pub fn wrap(inner: Rc<dyn Platform>, initial_factor: f32) -> Rc<dyn Platform> {
    let shared = Rc::new(Shared {
        factor: Cell::new(initial_factor),
        executor: inner.foreground_executor(),
        windows: RefCell::new(Vec::new()),
    });
    SHARED.with(|slot| *slot.borrow_mut() = Some(shared.clone()));
    Rc::new(ZoomPlatform { inner, shared })
}

pub fn zooms(params: &WindowParams) -> bool {
    matches!(params.kind, WindowKind::Normal) && params.is_resizable
}

struct WindowState {
    shared: Rc<Shared>,
    zoomed: bool,
    platform_size: Cell<Size<Pixels>>,
    platform_scale: Cell<f32>,
    on_resize: RefCell<Option<Box<dyn FnMut(Size<Pixels>, f32)>>>,
}

impl WindowState {
    fn zoom(&self) -> f32 {
        if self.zoomed {
            self.shared.factor.get()
        } else {
            DEFAULT_ZOOM
        }
    }

    fn relayout(&self) {
        let zoom = self.zoom();
        let size = self.platform_size.get();
        let scale = self.platform_scale.get();
        if let Ok(mut callback) = self.on_resize.try_borrow_mut()
            && let Some(callback) = callback.as_mut()
        {
            callback(scale_size(size, 1. / zoom), scale * zoom);
        }
    }
}

fn scale_point(point: Point<Pixels>, factor: f32) -> Point<Pixels> {
    gpui_kit::point(point.x * factor, point.y * factor)
}

fn scale_size(size: Size<Pixels>, factor: f32) -> Size<Pixels> {
    gpui_kit::size(size.width * factor, size.height * factor)
}

fn scale_bounds(bounds: Bounds<Pixels>, factor: f32) -> Bounds<Pixels> {
    Bounds {
        origin: scale_point(bounds.origin, factor),
        size: scale_size(bounds.size, factor),
    }
}

fn unzoom_input(input: PlatformInput, zoom: f32) -> PlatformInput {
    if (zoom - DEFAULT_ZOOM).abs() < f32::EPSILON {
        return input;
    }
    let factor = 1. / zoom;
    let scale = |point: Point<Pixels>| scale_point(point, factor);
    match input {
        PlatformInput::MouseDown(mut event) => {
            event.position = scale(event.position);
            PlatformInput::MouseDown(event)
        }
        PlatformInput::MouseUp(mut event) => {
            event.position = scale(event.position);
            PlatformInput::MouseUp(event)
        }
        PlatformInput::MousePressure(mut event) => {
            event.position = scale(event.position);
            PlatformInput::MousePressure(event)
        }
        PlatformInput::MouseMove(mut event) => {
            event.position = scale(event.position);
            PlatformInput::MouseMove(event)
        }
        PlatformInput::MouseExited(mut event) => {
            event.position = scale(event.position);
            PlatformInput::MouseExited(event)
        }
        PlatformInput::ScrollWheel(mut event) => {
            event.position = scale(event.position);
            if let ScrollDelta::Pixels(delta) = event.delta {
                event.delta = ScrollDelta::Pixels(scale(delta));
            }
            PlatformInput::ScrollWheel(event)
        }
        PlatformInput::Pinch(mut event) => {
            event.position = scale(event.position);
            PlatformInput::Pinch(event)
        }
        PlatformInput::LongPress(mut event) => {
            event.start_position = scale(event.start_position);
            event.position = scale(event.position);
            PlatformInput::LongPress(event)
        }
        PlatformInput::TouchDrag(mut event) => {
            event.start_position = scale(event.start_position);
            event.position = scale(event.position);
            PlatformInput::TouchDrag(event)
        }
        PlatformInput::Touch(mut event) => {
            event.position = scale(event.position);
            event.predicted_position = event.predicted_position.map(scale);
            PlatformInput::Touch(event)
        }
        PlatformInput::FileDrop(event) => PlatformInput::FileDrop(match event {
            FileDropEvent::Entered { position, paths } => FileDropEvent::Entered {
                position: scale(position),
                paths,
            },
            FileDropEvent::Pending { position } => FileDropEvent::Pending {
                position: scale(position),
            },
            FileDropEvent::Submit { position } => FileDropEvent::Submit {
                position: scale(position),
            },
            other => other,
        }),
        other @ (PlatformInput::KeyDown(_)
        | PlatformInput::KeyUp(_)
        | PlatformInput::ModifiersChanged(_)) => other,
    }
}

struct ZoomPlatform {
    inner: Rc<dyn Platform>,
    shared: Rc<Shared>,
}

impl Platform for ZoomPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.inner.background_executor()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.inner.foreground_executor()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.inner.text_system()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        self.inner.run(on_finish_launching)
    }

    fn quit(&self) {
        self.inner.quit()
    }

    fn set_initial_windowing(&self, request: WindowingRequest) {
        self.inner.set_initial_windowing(request)
    }

    fn request_windowing(&self, request: WindowingRequest) -> Task<Result<()>> {
        self.inner.request_windowing(request)
    }

    fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<OsString>) {
        self.inner.restart(binary_path, arguments)
    }

    fn activate(&self, ignoring_other_apps: bool) {
        self.inner.activate(ignoring_other_apps)
    }

    fn set_activation_policy(&self, policy: ActivationPolicy) {
        self.inner.set_activation_policy(policy)
    }

    fn hide(&self) {
        self.inner.hide()
    }

    fn hide_other_apps(&self) {
        self.inner.hide_other_apps()
    }

    fn unhide_other_apps(&self) {
        self.inner.unhide_other_apps()
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.inner.displays()
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.inner.primary_display()
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.inner.active_window()
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        self.inner.window_stack()
    }

    fn is_screen_capture_supported(&self) -> bool {
        self.inner.is_screen_capture_supported()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let zoomed = zooms(&options);
        let inner = self.inner.open_window(handle, options)?;
        let state = Rc::new(WindowState {
            shared: self.shared.clone(),
            zoomed,
            platform_size: Cell::new(inner.content_size()),
            platform_scale: Cell::new(inner.scale_factor()),
            on_resize: RefCell::new(None),
        });
        self.shared.windows.borrow_mut().push(Rc::downgrade(&state));
        Ok(Box::new(ZoomWindow { inner, state }))
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.inner.window_appearance()
    }

    fn set_window_appearance(&self, appearance: Option<WindowAppearance>) {
        self.inner.set_window_appearance(appearance)
    }

    fn button_layout(&self) -> Option<WindowButtonLayout> {
        self.inner.button_layout()
    }

    fn open_url(&self, url: &str) {
        self.inner.open_url(url)
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.inner.on_open_urls(callback)
    }

    fn register_url_scheme(&self, url: &str) -> Task<Result<()>> {
        self.inner.register_url_scheme(url)
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        self.inner.prompt_for_paths(options)
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        self.inner.prompt_for_new_path(directory, suggested_name)
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        self.inner.can_select_mixed_files_and_dirs()
    }

    fn reveal_path(&self, path: &Path) {
        self.inner.reveal_path(path)
    }

    fn open_with_system(&self, path: &Path) {
        self.inner.open_with_system(path)
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.inner.on_quit(callback)
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_reopen(callback)
    }

    fn on_system_sleep(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_sleep(callback)
    }

    fn on_system_wake(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_wake(callback)
    }

    fn on_app_lifecycle(&self, callback: Box<dyn FnMut(AppLifecyclePhase)>) {
        self.inner.on_app_lifecycle(callback)
    }

    fn on_memory_warning(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_memory_warning(callback)
    }

    fn gestures(&self) -> Option<Rc<dyn PlatformGestures>> {
        self.inner.gestures()
    }

    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) {
        self.inner.set_menus(menus, keymap)
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        self.inner.get_menus()
    }

    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        self.inner.set_dock_menu(menu, keymap)
    }

    fn perform_dock_menu_action(&self, action: usize) {
        self.inner.perform_dock_menu_action(action)
    }

    fn add_recent_document(&self, path: &Path) {
        self.inner.add_recent_document(path)
    }

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.inner.on_app_menu_action(callback)
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_will_open_app_menu(callback)
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.inner.on_validate_app_menu_command(callback)
    }

    fn thermal_state(&self) -> ThermalState {
        self.inner.thermal_state()
    }

    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_thermal_state_change(callback)
    }

    fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>> {
        self.inner.prevent_idle_sleep(reason)
    }

    fn set_app_identity(&self, identifier: &str, name: &str) {
        self.inner.set_app_identity(identifier, name)
    }

    fn show_system_notification(&self, notification: SystemNotification) {
        self.inner.show_system_notification(notification)
    }

    fn dismiss_system_notification(&self, tag: &str) {
        self.inner.dismiss_system_notification(tag)
    }

    fn on_system_notification_response(
        &self,
        callback: Box<dyn FnMut(SystemNotificationResponse)>,
    ) {
        self.inner.on_system_notification_response(callback)
    }

    fn compositor_name(&self) -> &'static str {
        self.inner.compositor_name()
    }

    fn graphical_environment(&self) -> Option<GraphicalEnvironment> {
        self.inner.graphical_environment()
    }

    fn app_path(&self) -> Result<PathBuf> {
        self.inner.app_path()
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        self.inner.path_for_auxiliary_executable(name)
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        self.inner.set_cursor_style(style)
    }

    fn hide_cursor_until_mouse_moves(&self) {
        self.inner.hide_cursor_until_mouse_moves()
    }

    fn is_cursor_visible(&self) -> bool {
        self.inner.is_cursor_visible()
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        self.inner.should_auto_hide_scrollbars()
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.inner.read_from_clipboard()
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        self.inner.write_to_clipboard(item)
    }

    fn read_from_clipboard_async(&self) -> Task<Result<Option<ClipboardItem>, ClipboardReadError>> {
        self.inner.read_from_clipboard_async()
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.inner.read_from_primary()
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn write_to_primary(&self, item: ClipboardItem) {
        self.inner.write_to_primary(item)
    }

    #[cfg(target_os = "macos")]
    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
        self.inner.read_from_find_pasteboard()
    }

    #[cfg(target_os = "macos")]
    fn write_to_find_pasteboard(&self, item: ClipboardItem) {
        self.inner.write_to_find_pasteboard(item)
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        self.inner.write_credentials(url, username, password)
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        self.inner.read_credentials(url)
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        self.inner.delete_credentials(url)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        self.inner.keyboard_layout()
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        self.inner.keyboard_mapper()
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_keyboard_layout_change(callback)
    }
}

struct ZoomWindow {
    inner: Box<dyn PlatformWindow>,
    state: Rc<WindowState>,
}

impl HasWindowHandle for ZoomWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.inner.window_handle()
    }
}

impl HasDisplayHandle for ZoomWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.inner.display_handle()
    }
}

impl PlatformWindow for ZoomWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.inner.bounds()
    }

    fn is_maximized(&self) -> bool {
        self.inner.is_maximized()
    }

    fn window_bounds(&self) -> WindowBounds {
        self.inner.window_bounds()
    }

    fn content_size(&self) -> Size<Pixels> {
        scale_size(self.inner.content_size(), 1. / self.state.zoom())
    }

    fn on_visual_viewport_changed(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_visual_viewport_changed(callback)
    }

    fn prepare_frame(&self) -> bool {
        self.inner.prepare_frame()
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let zoom = self.state.zoom();
        self.inner.resize(scale_size(size, zoom))
    }

    fn scale_factor(&self) -> f32 {
        self.inner.scale_factor() * self.state.zoom()
    }

    fn appearance(&self) -> WindowAppearance {
        self.inner.appearance()
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.inner.display()
    }

    fn mouse_position(&self) -> Point<Pixels> {
        scale_point(self.inner.mouse_position(), 1. / self.state.zoom())
    }

    fn modifiers(&self) -> Modifiers {
        self.inner.modifiers()
    }

    fn capslock(&self) -> Capslock {
        self.inner.capslock()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.inner.set_input_handler(input_handler)
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.inner.take_input_handler()
    }

    fn set_text_input_configuration(&mut self, configuration: TextInputConfiguration) {
        self.inner.set_text_input_configuration(configuration)
    }

    fn prompt(
        &self,
        level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        self.inner.prompt(level, msg, detail, answers)
    }

    fn activate(&self) {
        self.inner.activate()
    }

    fn request_attention(&self) {
        self.inner.request_attention()
    }

    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn visibility(&self) -> WindowVisibility {
        self.inner.visibility()
    }

    fn is_hovered(&self) -> bool {
        self.inner.is_hovered()
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.inner.background_appearance()
    }

    fn set_title(&mut self, title: &str) {
        self.inner.set_title(title)
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        self.inner.set_background_appearance(background_appearance)
    }

    fn minimize(&self) {
        self.inner.minimize()
    }

    fn zoom(&self) {
        self.inner.zoom()
    }

    fn toggle_fullscreen(&self) {
        self.inner.toggle_fullscreen()
    }

    fn is_fullscreen(&self) -> bool {
        self.inner.is_fullscreen()
    }

    fn frame_waker(&self) -> Option<Rc<dyn Fn()>> {
        self.inner.frame_waker()
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.inner.on_request_frame(callback)
    }

    fn on_input(&self, mut callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        let state = self.state.clone();
        self.inner.on_input(Box::new(move |input| {
            callback(unzoom_input(input, state.zoom()))
        }))
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.inner.on_active_status_change(callback)
    }

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        self.inner.on_visibility_change(callback)
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.inner.on_hover_status_change(callback)
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        *self.state.on_resize.borrow_mut() = Some(callback);
        let state = self.state.clone();
        self.inner.on_resize(Box::new(move |size, scale| {
            state.platform_size.set(size);
            state.platform_scale.set(scale);
            state.relayout();
        }))
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_moved(callback)
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.inner.on_should_close(callback)
    }

    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        self.inner.on_hit_test_window_control(callback)
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.inner.on_close(callback)
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_appearance_changed(callback)
    }

    fn on_button_layout_changed(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_button_layout_changed(callback)
    }

    fn draw(&self, scene: &Scene) {
        self.inner.draw(scene)
    }

    fn schedule_frame(&self) {
        self.inner.schedule_frame()
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.inner.sprite_atlas()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        self.inner.is_subpixel_rendering_supported()
    }

    fn get_title(&self) -> String {
        self.inner.get_title()
    }

    fn show_character_palette(&self) {
        self.inner.show_character_palette()
    }

    fn titlebar_double_click(&self, is_resizable: bool, is_minimizable: bool) {
        self.inner
            .titlebar_double_click(is_resizable, is_minimizable)
    }

    fn native_window_state(&self) -> Option<Vec<u8>> {
        self.inner.native_window_state()
    }

    fn restore_native_window_state(&self, state: &[u8]) {
        self.inner.restore_native_window_state(state)
    }

    #[cfg(target_os = "windows")]
    fn get_raw_handle(&self) -> windows::Win32::Foundation::HWND {
        self.inner.get_raw_handle()
    }

    fn inner_window_bounds(&self) -> WindowBounds {
        self.inner.inner_window_bounds()
    }

    fn request_decorations(&self, decorations: WindowDecorations) {
        self.inner.request_decorations(decorations)
    }

    fn show_window_menu(&self, position: Point<Pixels>) {
        self.inner
            .show_window_menu(scale_point(position, self.state.zoom()))
    }

    fn start_window_move(&self) {
        self.inner.start_window_move()
    }

    fn can_start_external_drag(&self) -> bool {
        self.inner.can_start_external_drag()
    }

    fn start_external_drag(&self, payload: &ExternalDragPayload) -> bool {
        self.inner.start_external_drag(payload)
    }

    fn start_window_resize(&self, edge: ResizeEdge) {
        self.inner.start_window_resize(edge)
    }

    fn set_exclusive_zone(&self, zone: Pixels) {
        self.inner.set_exclusive_zone(zone * self.state.zoom())
    }

    fn set_input_region(&self, region: Option<&[Bounds<Pixels>]>) {
        let zoom = self.state.zoom();
        let scaled: Option<Vec<Bounds<Pixels>>> = region.map(|region| {
            region
                .iter()
                .map(|bounds| scale_bounds(*bounds, zoom))
                .collect()
        });
        self.inner.set_input_region(scaled.as_deref())
    }

    fn window_decorations(&self) -> Decorations {
        self.inner.window_decorations()
    }

    fn set_app_id(&mut self, app_id: &str) {
        self.inner.set_app_id(app_id)
    }

    fn map_window(&mut self) -> Result<()> {
        self.inner.map_window()
    }

    fn window_controls(&self) -> WindowControls {
        self.inner.window_controls()
    }

    fn set_client_inset(&self, inset: Pixels) {
        self.inner.set_client_inset(inset * self.state.zoom())
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.inner.gpu_specs()
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        self.inner
            .update_ime_position(scale_bounds(bounds, self.state.zoom()))
    }

    fn insets(&self) -> WindowInsets {
        self.inner.insets()
    }

    fn on_insets_changed(&self, callback: Box<dyn FnMut(WindowInsets)>) {
        self.inner.on_insets_changed(callback)
    }

    fn text_input_state_changed(&self, change: TextInputStateChange) {
        self.inner.text_input_state_changed(change)
    }

    fn play_system_bell(&self) {
        self.inner.play_system_bell()
    }

    fn a11y_init(&self, callbacks: A11yCallbacks) {
        self.inner.a11y_init(callbacks)
    }

    fn a11y_tree_update(&self, tree_update: accesskit::TreeUpdate) {
        self.inner.a11y_tree_update(tree_update)
    }

    fn a11y_update_window_bounds(&self) {
        self.inner.a11y_update_window_bounds()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        FileDropEvent, Modifiers, MouseButton, MouseDownEvent, PlatformInput, ScrollDelta,
        ScrollWheelEvent, TouchPhase, point, px,
    };

    use super::unzoom_input;

    #[test]
    fn pointer_positions_move_into_the_zoomed_layout() {
        let down = PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(300.), px(150.)),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        });
        let PlatformInput::MouseDown(event) = unzoom_input(down, 1.5) else {
            panic!("mouse down stays a mouse down");
        };
        assert_eq!(event.position, point(px(200.), px(100.)));
    }

    #[test]
    fn precise_scrolling_follows_the_finger_line_scrolling_keeps_its_lines() {
        let wheel = |delta| {
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(100.), px(100.)),
                delta,
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            })
        };
        let PlatformInput::ScrollWheel(pixels) =
            unzoom_input(wheel(ScrollDelta::Pixels(point(px(0.), px(40.)))), 2.)
        else {
            panic!("scroll stays a scroll");
        };
        assert_eq!(pixels.position, point(px(50.), px(50.)));
        assert!(
            matches!(pixels.delta, ScrollDelta::Pixels(delta) if delta == point(px(0.), px(20.)))
        );
        let PlatformInput::ScrollWheel(lines) =
            unzoom_input(wheel(ScrollDelta::Lines(point(0., 3.))), 2.)
        else {
            panic!("scroll stays a scroll");
        };
        assert!(matches!(lines.delta, ScrollDelta::Lines(delta) if delta == point(0., 3.)));
    }

    #[test]
    fn file_drops_land_where_the_pointer_is() {
        let drop = PlatformInput::FileDrop(FileDropEvent::Submit {
            position: point(px(90.), px(30.)),
        });
        let PlatformInput::FileDrop(FileDropEvent::Submit { position }) = unzoom_input(drop, 1.25)
        else {
            panic!("drop stays a drop");
        };
        assert_eq!(position, point(px(72.), px(24.)));
    }
}
