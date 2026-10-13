use std::sync::Arc;
use std::time::Instant;

use calling::VideoKey;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use gpui_kit::component::tooltip::Tooltip;

use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{ShareStage, ZoomStep};
use crate::theme;

const STAGE_RADIUS: f32 = 12.;
const LABEL_OFFSET: f32 = 14.;
const STAGE_BACKGROUND: f32 = 0.04;
const LABEL_BACKGROUND: f32 = 0.6;
pub const WAITING_TEXT: &str = "Loading the shared screen...";
pub const FULL_WINDOW_HINT: &str = "Double-click or press Esc to leave full window";
pub const POP_OUT_TOOLTIP: &str = "Pop out";
pub const POPPED_TEXT: &str = "Shared content is open in its own window";
const LINE_HEIGHT: f32 = 20.;
const CHIP_HEIGHT: f32 = 30.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageMode {
    Docked,
    Fullscreen,
    PopOut,
}

fn stage_black() -> Hsla {
    hsla(0., 0., STAGE_BACKGROUND, 1.)
}

fn label_chip(text: String) -> Div {
    h_flex()
        .absolute()
        .top(px(LABEL_OFFSET))
        .left(px(LABEL_OFFSET))
        .h(px(30.))
        .px(px(12.))
        .gap(px(8.))
        .items_center()
        .rounded_full()
        .bg(hsla(0., 0., 0., LABEL_BACKGROUND))
        .text_color(theme::white())
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(icon(IconName::ScreenShare, 15., theme::white()))
        .child(text)
}

fn picture(image: Option<Arc<RenderImage>>, stage: &ShareStage) -> AnyElement {
    let Some(image) = image else {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .text_color(theme::text_muted())
            .child(WAITING_TEXT)
            .into_any_element();
    };
    let size = stage.size();
    let (offset_x, offset_y) = stage.view.offset(size);
    let zoom = stage.view.zoom();
    div()
        .absolute()
        .left(px(offset_x))
        .top(px(offset_y))
        .w(px(size.0 * zoom))
        .h(px(size.1 * zoom))
        .child(
            img(ImageSource::Render(image))
                .size_full()
                .object_fit(ObjectFit::Contain),
        )
        .into_any_element()
}

fn measure(stage: &ShareStage) -> impl IntoElement {
    let bounds = stage.bounds.clone();
    canvas(
        |_, _, _| {},
        move |area, _, window, _| {
            let measured = (
                f32::from(area.origin.x),
                f32::from(area.origin.y),
                f32::from(area.size.width),
                f32::from(area.size.height),
            );
            if bounds.get() != measured {
                bounds.set(measured);
                window.refresh();
            }
        },
    )
    .absolute()
    .size_full()
}

fn zoom_chip(app: &Entity<AppState>, stage: &ShareStage) -> Option<AnyElement> {
    if !stage.view.is_zoomed() {
        return None;
    }
    let app = app.clone();
    Some(
        div()
            .absolute()
            .top(px(LABEL_OFFSET))
            .right(px(LABEL_OFFSET))
            .debug_selector(|| "shell/islands/main/call-stage/share-content/zoom-chip".into())
            .child(
                h_flex()
                    .id("call-stage-zoom")
                    .h(px(CHIP_HEIGHT))
                    .px(px(12.))
                    .gap(px(8.))
                    .items_center()
                    .rounded_full()
                    .bg(hsla(0., 0., 0., LABEL_BACKGROUND))
                    .text_color(theme::white())
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|chip| chip.opacity(0.85))
                    .tooltip(|window, cx| Tooltip::new("Reset zoom").build(window, cx))
                    .child(icon(IconName::Search, 15., theme::white()))
                    .child(format!("{}%", stage.view.percent()))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        app.update(cx, |state, cx| state.zoom_share(ZoomStep::Reset, cx));
                    }),
            )
            .into_any_element(),
    )
}

fn pop_out_button(app: &Entity<AppState>) -> AnyElement {
    let app = app.clone();
    div()
        .absolute()
        .bottom(px(LABEL_OFFSET))
        .right(px(LABEL_OFFSET))
        .child(
            h_flex()
                .id("call-stage-pop-out")
                .h(px(CHIP_HEIGHT))
                .px(px(12.))
                .gap(px(8.))
                .items_center()
                .rounded_full()
                .bg(hsla(0., 0., 0., LABEL_BACKGROUND))
                .text_color(theme::white())
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|chip| chip.opacity(0.85))
                .child(icon(IconName::ExternalLink, 15., theme::white()))
                .child(POP_OUT_TOOLTIP)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    app.update(cx, |state, cx| state.pop_out_stage(cx));
                }),
        )
        .into_any_element()
}

pub fn popped_placeholder(app: &Entity<AppState>) -> AnyElement {
    let app = app.clone();
    v_flex()
        .id("call-stage-popped")
        .flex_1()
        .min_w_0()
        .h_full()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .rounded(px(STAGE_RADIUS))
        .bg(stage_black())
        .text_size(px(14.))
        .text_color(theme::text_muted())
        .child(POPPED_TEXT)
        .child(
            h_flex()
                .id("call-stage-bring-back")
                .h(px(CHIP_HEIGHT))
                .px(px(14.))
                .items_center()
                .rounded_full()
                .bg(theme::accent())
                .text_color(theme::on_accent())
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|button| button.opacity(0.9))
                .child("Bring back")
                .on_click(move |_, _, cx| {
                    app.update(cx, |state, cx| state.return_stage_from_pop_out(cx))
                }),
        )
        .into_any_element()
}

pub fn render_stage(
    app: &Entity<AppState>,
    state: &AppState,
    mode: StageMode,
) -> Option<AnyElement> {
    let call = state.call.as_ref()?;
    let label = call.model.sharing_label()?;
    let image = call.pictures.live(&VideoKey::Screen, Instant::now());
    let share_stage = &call.share_stage;
    let toggle_app = app.clone();
    let wheel_app = app.clone();
    let down_app = app.clone();
    let move_app = app.clone();
    let up_app = app.clone();
    let up_out_app = app.clone();
    let position = |point: Point<Pixels>| (f32::from(point.x), f32::from(point.y));
    Some(
        div()
            .id(match mode {
                StageMode::Docked => "call-stage",
                StageMode::Fullscreen => "call-stage-full",
                StageMode::PopOut => "call-stage-pop-out-window",
            })
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .bg(stage_black())
            .when(mode == StageMode::Docked, |stage| {
                stage
                    .rounded(px(STAGE_RADIUS))
                    .debug_selector(|| "shell/islands/main/call-stage/share-content".into())
            })
            .map(|stage| {
                if share_stage.view.is_zoomed() {
                    stage.cursor_grab()
                } else {
                    stage.cursor_pointer()
                }
            })
            .on_scroll_wheel(move |event, _, cx| {
                if !event.modifiers.control {
                    return;
                }
                let scroll = f32::from(event.delta.pixel_delta(px(LINE_HEIGHT)).y);
                wheel_app.update(cx, |state, cx| {
                    state.zoom_share_wheel(scroll, position(event.position), cx)
                });
                cx.stop_propagation();
            })
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                down_app.update(cx, |state, cx| state.begin_share_pan(position(event.position), cx));
            })
            .on_mouse_move(move |event, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    move_app.update(cx, |state, cx| state.pan_share_to(position(event.position), cx));
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                up_app.update(cx, |state, cx| state.end_share_pan(cx));
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                up_out_app.update(cx, |state, cx| state.end_share_pan(cx));
            })
            .on_click(move |event, _, cx| {
                if mode != StageMode::PopOut && event.click_count() >= 2 {
                    toggle_app.update(cx, |state, cx| state.toggle_stage_fullscreen(cx));
                }
            })
            .child(measure(share_stage))
            .child(picture(image, share_stage))
            .child(label_chip(label))
            .children(zoom_chip(app, share_stage))
            .when(mode == StageMode::Docked, |stage| stage.child(pop_out_button(app)))
            .when(mode == StageMode::Fullscreen, |stage| {
                stage.child(
                    div()
                        .absolute()
                        .bottom(px(LABEL_OFFSET))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .px(px(12.))
                                .py(px(6.))
                                .rounded_full()
                                .bg(hsla(0., 0., 0., LABEL_BACKGROUND))
                                .text_size(px(12.))
                                .text_color(theme::white())
                                .child(FULL_WINDOW_HINT),
                        ),
                )
            })
            .into_any_element(),
    )
}

pub fn render_stage_overlay(app: &Entity<AppState>, state: &AppState) -> Option<AnyElement> {
    if !state.stage_fullscreen() {
        return None;
    }
    let stage = render_stage(app, state, StageMode::Fullscreen)?;
    Some(
        v_flex()
            .id("call-stage-overlay")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(stage_black())
            .child(stage)
            .into_any_element(),
    )
}

pub fn render_call_overlay(view: AnyElement) -> AnyElement {
    v_flex()
        .id("call-fullscreen-overlay")
        .occlude()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(theme::background())
        .child(view)
        .into_any_element()
}
