use std::sync::Arc;
use std::time::Instant;

use calling::VideoKey;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::icon;
use crate::app_state::AppState;
use crate::theme;

const STAGE_RADIUS: f32 = 12.;
const LABEL_OFFSET: f32 = 14.;
const STAGE_BACKGROUND: f32 = 0.04;
const LABEL_BACKGROUND: f32 = 0.6;
pub const WAITING_TEXT: &str = "Loading the shared screen...";
pub const FULL_WINDOW_HINT: &str = "Double-click or press Esc to leave full window";

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

fn picture(image: Option<Arc<RenderImage>>) -> AnyElement {
    match image {
        Some(image) => img(ImageSource::Render(image))
            .size_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .text_color(theme::text_muted())
            .child(WAITING_TEXT)
            .into_any_element(),
    }
}

pub fn render_stage(app: &Entity<AppState>, state: &AppState, fullscreen: bool) -> Option<AnyElement> {
    let call = state.call.as_ref()?;
    let label = call.model.sharing_label()?;
    let image = call.pictures.live(&VideoKey::Screen, Instant::now());
    let toggle_app = app.clone();
    Some(
        div()
            .id(if fullscreen { "call-stage-full" } else { "call-stage" })
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .bg(stage_black())
            .when(!fullscreen, |stage| stage.rounded(px(STAGE_RADIUS)))
            .cursor_pointer()
            .on_click(move |event, _, cx| {
                if event.click_count() >= 2 {
                    toggle_app.update(cx, |state, cx| state.toggle_stage_fullscreen(cx));
                }
            })
            .child(picture(image))
            .child(label_chip(label))
            .when(fullscreen, |stage| {
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
    let stage = render_stage(app, state, true)?;
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
