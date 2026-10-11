use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use image::{Frame, ImageReader};

use super::avatar::person_avatar;
use super::widgets::symbol;
use crate::app_state::AppState;
use crate::downloads;
use crate::image_view_gallery::{GalleryItem, ViewerRequest};
use crate::theme;
use crate::image_view_math::{
    MAX_ZOOM, Pair, View, clamp_pan, clamp_scale, counter_label, display_size,
    fit_scale, image_origin, next_rotation, percent_label, rotated_size, step_index, step_scale,
    thumbnail_window, toggle_target, wheel_scale, zoom_at,
};

const HEADER_HEIGHT: f32 = 52.;
const FOOTER_HEIGHT: f32 = 120.;
const BUTTON_SIZE: f32 = 36.;
const BUTTON_RADIUS: f32 = 8.;
const BUTTON_ICON_SIZE: f32 = 20.;
const NAV_ICON_SIZE: f32 = 24.;
const ZOOM_LABEL_WIDTH: f32 = 52.;
const NAV_SIZE: f32 = 44.;
const NAV_MARGIN: f32 = 16.;
const AVATAR_SIZE: f32 = 32.;
const SIDE_PADDING: f32 = 16.;
const THUMBNAIL_SIZE: f32 = 48.;
const THUMBNAIL_GAP: f32 = 8.;
const THUMBNAIL_RADIUS: f32 = 6.;
const THUMBNAIL_BORDER: f32 = 2.;
const CAPTION_MAX_WIDTH: f32 = 720.;
const CAPTION_HEIGHT: f32 = 22.;
const STATUS_HEIGHT: f32 = 18.;
const STRIP_HEIGHT: f32 = THUMBNAIL_SIZE + 2. * THUMBNAIL_BORDER;
const WHEEL_LINE_HEIGHT: f32 = 20.;
const BACKDROP: u32 = 0x080809fc;
const DISABLED_OPACITY: f32 = 0.35;
const NOTICE_DURATION: Duration = Duration::from_secs(2);

pub enum ImageViewerEvent {
    Close,
}

struct RotatedImage {
    url: String,
    quarter_turns: u8,
    render: Arc<RenderImage>,
}

#[derive(Clone, Copy)]
struct Layout {
    stage: Pair,
    source: Pair,
    fit: f32,
    scale: f32,
    pan: Pair,
}

pub struct ImageViewer {
    app: Entity<AppState>,
    conversation_id: String,
    conversation_title: String,
    items: Vec<GalleryItem>,
    index: usize,
    scale: Option<f32>,
    pan: Pair,
    quarter_turns: u8,
    rotated: Option<RotatedImage>,
    entered_fullscreen: bool,
    drag_from: Option<Point<Pixels>>,
    notice: Option<String>,
    notice_serial: u64,
    focus_handle: FocusHandle,
    _observation: Subscription,
}

impl EventEmitter<ImageViewerEvent> for ImageViewer {}

fn decode(path: &Path) -> Option<image::DynamicImage> {
    ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()
}

fn rotate_decoded(decoded: image::DynamicImage, quarter_turns: u8) -> image::DynamicImage {
    match quarter_turns {
        1 => decoded.rotate90(),
        2 => decoded.rotate180(),
        3 => decoded.rotate270(),
        _ => decoded,
    }
}

fn rotated_render(path: &Path, quarter_turns: u8) -> Option<RenderImage> {
    let mut pixels = rotate_decoded(decode(path)?, quarter_turns).to_rgba8();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(RenderImage::new(vec![Frame::new(pixels)]))
}

fn rotated_png(path: &Path, quarter_turns: u8) -> Option<Vec<u8>> {
    let rotated = rotate_decoded(decode(path)?, quarter_turns);
    let mut bytes = Vec::new();
    rotated
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(bytes)
}

fn clipboard_format(bytes: &[u8]) -> Option<ImageFormat> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some(ImageFormat::Png),
        image::ImageFormat::Jpeg => Some(ImageFormat::Jpeg),
        image::ImageFormat::Gif => Some(ImageFormat::Gif),
        image::ImageFormat::WebP => Some(ImageFormat::Webp),
        image::ImageFormat::Bmp => Some(ImageFormat::Bmp),
        _ => None,
    }
}

fn clipboard_item(path: &Path, quarter_turns: u8) -> Option<ClipboardItem> {
    if quarter_turns != 0 {
        let bytes = rotated_png(path, quarter_turns)?;
        return Some(ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Png, bytes)));
    }
    let bytes = std::fs::read(path).ok()?;
    let format = clipboard_format(&bytes)?;
    Some(ClipboardItem::new_image(&Image::from_bytes(format, bytes)))
}

fn suggested_name(path: &Path, rotated: bool) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_owned());
    if rotated {
        return Path::new(&name).with_extension("png").to_string_lossy().into_owned();
    }
    if path.extension().is_some() {
        return name;
    }
    let extension = std::fs::read(path)
        .ok()
        .and_then(|bytes| image::guess_format(&bytes).ok())
        .and_then(|format| format.extensions_str().first().copied())
        .unwrap_or("png");
    format!("{name}.{extension}")
}

fn write_image(source: &Path, target: &Path, quarter_turns: u8) -> std::io::Result<()> {
    if quarter_turns == 0 {
        return std::fs::copy(source, target).map(|_| ());
    }
    let bytes = rotated_png(source, quarter_turns)
        .ok_or_else(|| std::io::Error::other("rotation failed"))?;
    std::fs::write(target, bytes)
}

fn control_button(
    id: &'static str,
    glyph: &'static str,
    tooltip: &'static str,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(BUTTON_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(BUTTON_RADIUS))
        .text_color(white())
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(white().opacity(0.12)))
        })
        .when(!enabled, |button| button.opacity(DISABLED_OPACITY))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .child(symbol(glyph, BUTTON_ICON_SIZE, white()))
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            if enabled {
                on_click(event, window, cx);
            }
        })
}

impl ImageViewer {
    pub fn new(
        app: Entity<AppState>,
        request: ViewerRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let observation = cx.observe(&app, |_, _, cx| cx.notify());
        let mut viewer = ImageViewer {
            app,
            conversation_id: request.conversation_id,
            conversation_title: request.conversation_title,
            index: request.index.min(request.items.len().saturating_sub(1)),
            items: request.items,
            scale: None,
            pan: (0., 0.),
            quarter_turns: 0,
            rotated: None,
            entered_fullscreen: false,
            drag_from: None,
            notice: None,
            notice_serial: 0,
            focus_handle,
            _observation: observation,
        };
        cx.on_release_in(window, |this, window, _| {
            this.release_rotated(window);
            if this.entered_fullscreen && window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        })
        .detach();
        viewer.preload(window, cx);
        viewer
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    fn preload(&mut self, window: &Window, cx: &mut Context<Self>) {
        let last_index = self.items.len().saturating_sub(1);
        let visible = thumbnail_window(
            self.index,
            self.items.len(),
            Self::thumbnail_capacity(window),
        );
        let first = visible.start.min(self.index.saturating_sub(1));
        let last = visible.end.saturating_sub(1).max((self.index + 1).min(last_index));
        let directory = &self.app.read(cx).directory;
        let missing: Vec<_> = self
            .items
            .get(first..=last)
            .unwrap_or_default()
            .iter()
            .map(|item| item.image.clone())
            .filter(|image| directory.image(&image.url).is_none())
            .collect();
        if !missing.is_empty() {
            self.app
                .update(cx, |state, cx| state.request_images(missing, cx));
        }
    }

    fn thumbnail_capacity(window: &Window) -> usize {
        let width = f32::from(window.viewport_size().width);
        ((width - 2. * SIDE_PADDING) / (THUMBNAIL_SIZE + 2. * THUMBNAIL_BORDER + THUMBNAIL_GAP))
            .floor()
            .max(1.) as usize
    }

    fn release_rotated(&mut self, window: &mut Window) {
        if let Some(rotated) = self.rotated.take() {
            window.drop_image(rotated.render).ok();
        }
    }

    fn toggle_fullscreen(&mut self, window: &mut Window) {
        self.entered_fullscreen = !window.is_fullscreen();
        window.toggle_fullscreen();
    }

    fn current(&self) -> Option<&GalleryItem> {
        self.items.get(self.index)
    }

    fn current_failed(&self, cx: &App) -> bool {
        self.current()
            .is_some_and(|item| self.app.read(cx).directory.image_failed(&item.image.url))
    }

    fn current_path(&self, cx: &App) -> Option<PathBuf> {
        let item = self.current()?;
        let entry = self.app.read(cx).directory.image(&item.image.url)?;
        Some(entry.path.clone())
    }

    fn stage_size(window: &Window) -> Pair {
        let viewport = window.viewport_size();
        (
            f32::from(viewport.width).max(1.),
            (f32::from(viewport.height) - HEADER_HEIGHT - FOOTER_HEIGHT).max(1.),
        )
    }

    fn layout(&self, window: &Window, cx: &App) -> Option<Layout> {
        let item = self.current()?;
        let entry = self.app.read(cx).directory.image(&item.image.url)?;
        let stage = Self::stage_size(window);
        let declared = item.image.width.zip(item.image.height);
        let source = entry
            .size
            .or(declared)
            .map_or(stage, |(width, height)| (width as f32, height as f32));
        let source = rotated_size(source, self.quarter_turns);
        let fit = fit_scale(source, stage);
        let scale = self.scale.map_or(fit, |scale| clamp_scale(scale, fit));
        let pan = clamp_pan(self.pan, display_size(source, scale), stage);
        Some(Layout {
            stage,
            source,
            fit,
            scale,
            pan,
        })
    }

    fn anchor_of(position: Point<Pixels>, window: &Window) -> Pair {
        let stage = Self::stage_size(window);
        (
            f32::from(position.x) - stage.0 / 2.,
            f32::from(position.y) - HEADER_HEIGHT - stage.1 / 2.,
        )
    }

    fn zoom_to(&mut self, wanted: f32, anchor: Pair, window: &Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout(window, cx) else {
            return;
        };
        let view = zoom_at(
            View {
                scale: layout.scale,
                pan: layout.pan,
            },
            wanted,
            anchor,
            layout.source,
            layout.stage,
        );
        self.scale = (view.scale > layout.fit).then_some(view.scale);
        self.pan = view.pan;
        cx.notify();
    }

    fn zoom_step(&mut self, direction: i32, window: &Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout(window, cx) else {
            return;
        };
        self.zoom_to(step_scale(layout.scale, direction), (0., 0.), window, cx);
    }

    fn zoom_fit(&mut self, cx: &mut Context<Self>) {
        self.scale = None;
        self.pan = (0., 0.);
        cx.notify();
    }

    fn toggle_zoom(&mut self, position: Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout(window, cx) else {
            return;
        };
        let target = toggle_target(layout.scale, layout.fit);
        self.zoom_to(target, Self::anchor_of(position, window), window, cx);
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout(window, cx) else {
            return;
        };
        let top = f32::from(event.position.y);
        if top < HEADER_HEIGHT || top > HEADER_HEIGHT + layout.stage.1 {
            return;
        }
        let delta = event.delta.pixel_delta(px(WHEEL_LINE_HEIGHT)).y.into();
        let anchor = Self::anchor_of(event.position, window);
        self.zoom_to(wheel_scale(layout.scale, delta), anchor, window, cx);
        cx.stop_propagation();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(last) = self.drag_from else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag_from = None;
            return;
        }
        let Some(layout) = self.layout(window, cx) else {
            return;
        };
        let moved = (
            layout.pan.0 + f32::from(event.position.x - last.x),
            layout.pan.1 + f32::from(event.position.y - last.y),
        );
        self.pan = clamp_pan(moved, display_size(layout.source, layout.scale), layout.stage);
        self.drag_from = Some(event.position);
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag_from.take().is_some() {
            cx.notify();
        }
    }

    fn jump(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.index || index >= self.items.len() {
            return;
        }
        self.index = index;
        self.scale = None;
        self.pan = (0., 0.);
        self.quarter_turns = 0;
        self.release_rotated(window);
        self.drag_from = None;
        self.preload(window, cx);
        cx.notify();
    }

    fn go(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.jump(step_index(self.index, self.items.len(), delta), window, cx);
    }

    fn rotate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(item)) = (self.current_path(cx), self.current()) else {
            return;
        };
        let url = item.image.url.clone();
        self.quarter_turns = next_rotation(self.quarter_turns);
        self.scale = None;
        self.pan = (0., 0.);
        self.release_rotated(window);
        let quarter_turns = self.quarter_turns;
        if quarter_turns != 0 {
            cx.spawn_in(window, async move |this, cx| {
                let rotated = cx
                    .background_spawn(async move { rotated_render(&path, quarter_turns) })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    let still_current = this.quarter_turns == quarter_turns
                        && this.current().is_some_and(|item| item.image.url == url);
                    if !still_current {
                        return;
                    }
                    match rotated {
                        Some(render) => {
                            this.release_rotated(window);
                            this.rotated = Some(RotatedImage {
                                url,
                                quarter_turns,
                                render: Arc::new(render),
                            });
                        }
                        None => {
                            this.quarter_turns = 0;
                            this.show_notice("Could not rotate", cx);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn show_notice(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.notice_serial += 1;
        let serial = self.notice_serial;
        self.notice = Some(text.into());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_DURATION).await;
            this.update(cx, |this, cx| {
                if this.notice_serial == serial {
                    this.notice = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn rotated_render(&self) -> Option<&Arc<RenderImage>> {
        let url = &self.current()?.image.url;
        self.rotated
            .as_ref()
            .filter(|rotated| rotated.url == *url && rotated.quarter_turns == self.quarter_turns)
            .map(|rotated| &rotated.render)
    }

    fn rotation_pending(&self) -> bool {
        self.quarter_turns != 0 && self.rotated_render().is_none()
    }

    fn copy_image(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current_path(cx) else {
            return;
        };
        if self.rotation_pending() {
            self.show_notice("Rotating", cx);
            return;
        }
        let quarter_turns = self.quarter_turns;
        cx.spawn(async move |this, cx| {
            let fallback = path.clone();
            let item = cx
                .background_spawn(async move { clipboard_item(&path, quarter_turns) })
                .await;
            this.update(cx, |this, cx| match item {
                Some(item) => {
                    cx.write_to_clipboard(item);
                    this.show_notice("Image copied", cx);
                }
                None => {
                    cx.write_to_clipboard(ClipboardItem::new_string(fallback.display().to_string()));
                    this.show_notice("Copied file path", cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn save_as(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.current_path(cx) else {
            return;
        };
        if self.rotation_pending() {
            self.show_notice("Rotating", cx);
            return;
        }
        let quarter_turns = self.quarter_turns;
        cx.spawn(async move |this, cx| {
            let name_source = source.clone();
            let name = cx
                .background_spawn(async move { suggested_name(&name_source, quarter_turns != 0) })
                .await;
            let Ok(receiver) = this.update(cx, |_, cx| {
                cx.prompt_for_new_path(&downloads::downloads_directory(), Some(&name))
            }) else {
                return;
            };
            let Ok(Ok(Some(target))) = receiver.await else {
                return;
            };
            let written = cx
                .background_spawn(async move {
                    write_image(&source, &target, quarter_turns).map(|_| target)
                })
                .await;
            this.update(cx, |this, cx| match written {
                Ok(target) => {
                    let name = target
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    this.show_notice(format!("Saved {name}"), cx)
                }
                Err(_) => this.show_notice("Could not save the image", cx),
            })
            .ok();
        })
        .detach();
    }

    fn forward(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.current() else {
            return;
        };
        let message_key = item.message_key.clone();
        if self.app.read(cx).mode.read_only {
            self.show_notice("Read-only mode: message not forwarded", cx);
            return;
        }
        let conversation_id = self.conversation_id.clone();
        self.app.update(cx, |state, cx| {
            state.request_forward(&conversation_id, &message_key, cx)
        });
    }

    fn open_externally(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.current_path(cx) {
            cx.open_with_system(&path);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let command = event.keystroke.modifiers.secondary();
        match (event.keystroke.key.as_str(), command) {
            ("escape", _) => cx.emit(ImageViewerEvent::Close),
            ("left", false) => self.go(-1, window, cx),
            ("right", false) => self.go(1, window, cx),
            ("r", false) => self.rotate(window, cx),
            ("f11", _) => self.toggle_fullscreen(window),
            ("=" | "+", true) => self.zoom_step(1, window, cx),
            ("-", true) => self.zoom_step(-1, window, cx),
            ("0", true) => self.zoom_fit(cx),
            ("c", true) => self.copy_image(cx),
            ("s", true) => self.save_as(cx),
            _ => {}
        }
        cx.stop_propagation();
    }

    fn header(&self, layout: Option<Layout>, cx: &mut Context<Self>) -> Stateful<Div> {
        let item = self.current();
        let author = item.map(|item| item.author.clone()).unwrap_or_default();
        let subtitle = item
            .map(|item| format!("{} \u{b7} {}", item.time, self.conversation_title))
            .unwrap_or_default();
        let sender_id = item.and_then(|item| item.sender_id.clone());
        let can_zoom_out = layout.is_some_and(|layout| layout.scale > layout.fit);
        let can_zoom_in = layout.is_some_and(|layout| layout.scale < MAX_ZOOM);
        let zoom_label = layout.map(|layout| percent_label(layout.scale)).unwrap_or_default();
        let loaded = layout.is_some();
        h_flex()
            .id("viewer-header")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(HEADER_HEIGHT))
            .px(px(SIDE_PADDING))
            .gap(px(12.))
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(10.))
                    .items_center()
                    .child(
                        div()
                            .size(px(AVATAR_SIZE))
                            .flex_none()
                            .child(person_avatar(
                                &self.app.read(cx).directory,
                                sender_id.as_deref(),
                                &author,
                                AVATAR_SIZE,
                            )),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(white())
                                    .child(author),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(white().opacity(0.6))
                                    .child(subtitle),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(2.))
                    .items_center()
                    .child(control_button(
                        "viewer-zoom-out",
                        "zoom_out",
                        "Zoom out (Ctrl -)",
                        can_zoom_out,
                        cx.listener(|this, _, window, cx| this.zoom_step(-1, window, cx)),
                    ))
                    .child(
                        div()
                            .w(px(ZOOM_LABEL_WIDTH))
                            .flex_none()
                            .text_center()
                            .text_size(px(12.))
                            .text_color(white().opacity(0.8))
                            .child(zoom_label),
                    )
                    .child(control_button(
                        "viewer-zoom-in",
                        "zoom_in",
                        "Zoom in (Ctrl +)",
                        can_zoom_in,
                        cx.listener(|this, _, window, cx| this.zoom_step(1, window, cx)),
                    ))
                    .child(control_button(
                        "viewer-rotate",
                        "rotate_right",
                        "Rotate (R)",
                        loaded,
                        cx.listener(|this, _, window, cx| this.rotate(window, cx)),
                    ))
                    .child(control_button(
                        "viewer-copy",
                        "content_copy",
                        "Copy image (Ctrl C)",
                        loaded,
                        cx.listener(|this, _, _, cx| this.copy_image(cx)),
                    ))
                    .child(control_button(
                        "viewer-save",
                        "download",
                        "Save as (Ctrl S)",
                        loaded,
                        cx.listener(|this, _, _, cx| this.save_as(cx)),
                    ))
                    .child(control_button(
                        "viewer-forward",
                        "forward",
                        "Forward",
                        true,
                        cx.listener(|this, _, _, cx| this.forward(cx)),
                    ))
                    .child(control_button(
                        "viewer-fullscreen",
                        "fullscreen",
                        "Full screen (F11)",
                        true,
                        cx.listener(|this, _, window, _| this.toggle_fullscreen(window)),
                    ))
                    .child(control_button(
                        "viewer-open",
                        "open_in_new",
                        "Open in Photos",
                        loaded,
                        cx.listener(|this, _, _, cx| this.open_externally(cx)),
                    ))
                    .child(control_button(
                        "viewer-close",
                        "close",
                        "Close (Esc)",
                        true,
                        cx.listener(|_, _, _, cx| cx.emit(ImageViewerEvent::Close)),
                    )),
            )
            .on_click(|_, _, cx| cx.stop_propagation())
    }

    fn nav_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        left: bool,
        stage: Pair,
        cx: &mut Context<Self>,
        delta: isize,
    ) -> Stateful<Div> {
        let top = ((stage.1 - NAV_SIZE) / 2.).max(0.);
        div()
            .id(id)
            .absolute()
            .top(px(top))
            .when(left, |button| button.left(px(NAV_MARGIN)))
            .when(!left, |button| button.right(px(NAV_MARGIN)))
            .size(px(NAV_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(white().opacity(0.1))
            .cursor_pointer()
            .hover(|button| button.bg(white().opacity(0.2)))
            .child(symbol(glyph, NAV_ICON_SIZE, white()))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.go(delta, window, cx);
            }))
    }

    fn picture(&self, layout: Layout, cx: &mut Context<Self>) -> AnyElement {
        let display = display_size(layout.source, layout.scale);
        let origin = image_origin(display, layout.stage, layout.pan);
        let Some(item) = self.current() else {
            return div().into_any_element();
        };
        let source: Option<ImageSource> = if self.quarter_turns == 0 {
            self.app
                .read(cx)
                .directory
                .image(&item.image.url)
                .map(|entry| entry.path.clone().into())
        } else {
            self.rotated_render().map(|render| ImageSource::Render(render.clone()))
        };
        let Some(source) = source else {
            return loading_placeholder();
        };
        div()
            .id("viewer-picture")
            .absolute()
            .left(px(origin.0))
            .top(px(origin.1))
            .w(px(display.0))
            .h(px(display.1))
            .child(img(source).size_full().object_fit(ObjectFit::Fill))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.drag_from = Some(event.position);
                }),
            )
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if event.click_count() == 2 {
                    this.toggle_zoom(event.position(), window, cx);
                }
            }))
            .into_any_element()
    }

    fn thumbnail(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let item = &self.items[index];
        let current = index == self.index;
        let path = self
            .app
            .read(cx)
            .directory
            .image(&item.image.url)
            .map(|entry| entry.path.clone());
        div()
            .id(ElementId::Name(format!("viewer-thumbnail-{index}").into()))
            .size(px(THUMBNAIL_SIZE + 2. * THUMBNAIL_BORDER))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(THUMBNAIL_RADIUS + THUMBNAIL_BORDER))
            .border_2()
            .border_color(if current {
                theme::accent()
            } else {
                transparent_black()
            })
            .cursor_pointer()
            .child(
                div()
                    .size(px(THUMBNAIL_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .rounded(px(THUMBNAIL_RADIUS))
                    .bg(white().opacity(0.08))
                    .when(!current, |frame| frame.opacity(0.7))
                    .map(|frame| match path {
                        Some(path) => frame.child(
                            img(path)
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        ),
                        None => frame.child(symbol("image", 18., white().opacity(0.5))),
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.jump(index, window, cx);
            }))
    }

    fn footer(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let width = f32::from(window.viewport_size().width);
        let range = thumbnail_window(
            self.index,
            self.items.len(),
            Self::thumbnail_capacity(window),
        );
        let caption = self
            .current()
            .map(|item| item.caption.clone())
            .unwrap_or_default();
        let status = self
            .notice
            .clone()
            .unwrap_or_else(|| counter_label(self.index, self.items.len()));
        v_flex()
            .id("viewer-footer")
            .absolute()
            .bottom_0()
            .left_0()
            .w_full()
            .h(px(FOOTER_HEIGHT))
            .pb(px(12.))
            .gap(px(4.))
            .items_center()
            .justify_end()
            .child(
                div()
                    .w(px((width - 2. * SIDE_PADDING).clamp(0., CAPTION_MAX_WIDTH)))
                    .h(px(CAPTION_HEIGHT))
                    .flex_none()
                    .truncate()
                    .text_center()
                    .text_size(px(13.))
                    .text_color(white().opacity(0.9))
                    .child(caption),
            )
            .child(
                div()
                    .h(px(STATUS_HEIGHT))
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(white().opacity(0.6))
                    .child(status),
            )
            .child(
                h_flex()
                    .h(px(STRIP_HEIGHT))
                    .flex_none()
                    .gap(px(THUMBNAIL_GAP))
                    .items_center()
                    .children(range.map(|index| self.thumbnail(index, cx))),
            )
            .on_click(|_, _, cx| cx.stop_propagation())
    }
}

fn status_placeholder(text: &'static str) -> AnyElement {
    v_flex()
        .size_full()
        .gap(px(6.))
        .items_center()
        .justify_center()
        .child(symbol("image", 28., white().opacity(0.5)))
        .child(
            div()
                .text_size(px(12.))
                .text_color(white().opacity(0.6))
                .child(text),
        )
        .into_any_element()
}

fn loading_placeholder() -> AnyElement {
    status_placeholder("Loading image")
}

impl Render for ImageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = self.layout(window, cx);
        let stage = Self::stage_size(window);
        let has_previous = self.index > 0;
        let has_next = self.index + 1 < self.items.len();
        let picture = match layout {
            Some(layout) => self.picture(layout, cx),
            None if self.current_failed(cx) => status_placeholder("Image unavailable"),
            None => loading_placeholder(),
        };
        let header = self.header(layout, cx);
        let footer = self.footer(window, cx);
        div()
            .id("image-viewer")
            .key_context("ImageViewer")
            .track_focus(&self.focus_handle)
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(rgba(BACKDROP))
            .occlude()
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
            )
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ImageViewerEvent::Close)))
            .child(
                div()
                    .absolute()
                    .top(px(HEADER_HEIGHT))
                    .left_0()
                    .w(px(stage.0))
                    .h(px(stage.1))
                    .overflow_hidden()
                    .child(picture)
                    .when(has_previous, |stage_element| {
                        stage_element.child(self.nav_button(
                            "viewer-previous",
                            "chevron_left",
                            true,
                            stage,
                            cx,
                            -1,
                        ))
                    })
                    .when(has_next, |stage_element| {
                        stage_element.child(self.nav_button(
                            "viewer-next",
                            "chevron_right",
                            false,
                            stage,
                            cx,
                            1,
                        ))
                    }),
            )
            .child(header)
            .child(footer)
    }
}
