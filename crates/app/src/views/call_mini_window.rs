use std::time::Instant;

use calling::VideoKey;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_view::{camera_button, leave_button, mute_button, round_control, status_line};
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{CallModel, CallWindows, Tile, TileState};
use crate::theme;

const CONTROLS_HEIGHT: f32 = 44.;
const CONTROL_SIZE: f32 = 30.;
const WINDOW_RADIUS: f32 = 10.;
const SELF_VIEW_WIDTH: f32 = 80.;
const SELF_VIEW_HEIGHT: f32 = 45.;
const SPEAKER_AVATAR: f32 = 56.;
const CHIP_BACKGROUND: f32 = 0.6;
const SHADE_BACKGROUND: f32 = 0.55;

pub struct MiniWindowView {
    windows: Entity<CallWindows>,
    app: Entity<AppState>,
    _app_subscription: Subscription,
    _media_subscription: Option<Subscription>,
}

impl MiniWindowView {
    pub fn new(windows: Entity<CallWindows>, cx: &mut Context<Self>) -> Self {
        let app = windows.read(cx).app().clone();
        let app_subscription = cx.observe(&app, |_, _, cx| cx.notify());
        let media_subscription = app
            .read(cx)
            .call_media
            .clone()
            .map(|media| cx.observe(&media, |_, _, cx| cx.notify()));
        MiniWindowView {
            windows,
            app,
            _app_subscription: app_subscription,
            _media_subscription: media_subscription,
        }
    }
}

pub fn speaker_tile(model: &CallModel) -> Option<&Tile> {
    let present: Vec<&Tile> = model.visible_tiles().into_iter().filter(|tile| tile.state == TileState::Present).collect();
    let focused = model
        .focus()
        .filter(|focus| !focus.own)
        .and_then(|focus| present.iter().find(|tile| tile.mri == focus.mri).copied());
    focused
        .or_else(|| present.iter().find(|tile| model.tile_speaking(tile)).copied())
        .or_else(|| present.first().copied())
}

fn chip(text: String) -> Div {
    div()
        .max_w(px(180.))
        .truncate()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(6.))
        .bg(hsla(0., 0., 0., CHIP_BACKGROUND))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::white())
        .child(text)
}

fn cover(image: std::sync::Arc<RenderImage>) -> Img {
    img(ImageSource::Render(image)).size_full().object_fit(ObjectFit::Cover)
}

fn speaker_layer(state: &AppState, model: &CallModel, now: Instant) -> AnyElement {
    let Some(call) = state.call.as_ref() else {
        return div().into_any_element();
    };
    let Some(tile) = speaker_tile(model) else {
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(icon(IconName::Phone, 28., theme::accent_text()))
            .child(div().max_w_full().truncate().text_size(px(13.)).child(model.title.clone()))
            .into_any_element();
    };
    let video = (tile.has_video || model.is_one_to_one())
        .then(|| call.pictures.live(&VideoKey::Person(tile.mri.clone()), now))
        .flatten();
    match video {
        Some(image) => cover(image).into_any_element(),
        None => v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(person_avatar(&state.directory, tile.user_id.as_deref(), &tile.name, SPEAKER_AVATAR))
            .child(div().max_w_full().truncate().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(tile.name.clone()))
            .into_any_element(),
    }
}

fn self_view(state: &AppState, model: &CallModel, now: Instant) -> Option<AnyElement> {
    let image = state
        .call
        .as_ref()
        .filter(|_| model.camera_on)
        .and_then(|call| call.pictures.live(&VideoKey::LocalCamera, now))?;
    Some(
        div()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .w(px(SELF_VIEW_WIDTH))
            .h(px(SELF_VIEW_HEIGHT))
            .overflow_hidden()
            .rounded(px(6.))
            .border_1()
            .border_color(theme::border_strong())
            .child(cover(image))
            .into_any_element(),
    )
}

impl Render for MiniWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.app.read(cx);
        let Some(call) = state.call.as_ref() else {
            return div().into_any_element();
        };
        let model = &call.model;
        let now = Instant::now();
        let (status, _) = status_line(model, now);
        let windows = self.windows.clone();
        let speaking = speaker_tile(model).is_some_and(|tile| model.tile_speaking(tile));
        let speaker_name = speaker_tile(model).map(|tile| tile.name.clone());
        div()
            .id("call-mini-window")
            .group("call-mini-window")
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(WINDOW_RADIUS))
            .bg(theme::surface())
            .border_2()
            .border_color(if speaking { theme::green() } else { theme::border_strong() })
            .text_color(theme::text())
            .child(speaker_layer(state, model, now))
            .child(
                div()
                    .id("call-mini-window-drag")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom(px(CONTROLS_HEIGHT))
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                    .child(div().absolute().top(px(8.)).left(px(8.)).child(chip(status)))
                    .children(speaker_name.map(|name| div().absolute().bottom(px(6.)).left(px(8.)).child(chip(name))))
                    .children(self_view(state, model, now)),
            )
            .child(
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(CONTROLS_HEIGHT))
                    .gap(px(10.))
                    .items_center()
                    .justify_center()
                    .bg(hsla(0., 0., 0., SHADE_BACKGROUND))
                    .opacity(0.)
                    .group_hover("call-mini-window", |controls| controls.opacity(1.))
                    .child(mute_button(&self.app, model, "call-mini-window-mute", CONTROL_SIZE))
                    .child(camera_button(&self.app, model).size(px(CONTROL_SIZE)))
                    .child(
                        round_control("call-mini-window-back", IconName::Maximize2, theme::surface_raised(), theme::text())
                            .size(px(CONTROL_SIZE))
                            .tooltip(|window, cx| Tooltip::new("Back to call").build(window, cx))
                            .on_click(move |_, _, cx| windows.update(cx, |windows, cx| windows.return_to_call(cx))),
                    )
                    .child(leave_button(&self.app, model, "call-mini-window-leave", true)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::speaker_tile;
    use crate::call::{CallModel, Tile, TileState};

    fn present(mri: &str, name: &str) -> Tile {
        Tile {
            state: TileState::Present,
            ..Tile::invited(mri, name)
        }
    }

    #[test]
    fn speaker_is_the_speaking_tile_before_the_first_one() {
        let mut model = CallModel::test();
        model.tiles = vec![present("8:a", "Ada"), present("8:b", "Bo")];
        assert_eq!(speaker_tile(&model).map(|tile| tile.name.as_str()), Some("Ada"));
        model.speaking_mris = vec!["8:b".to_owned()];
        assert_eq!(speaker_tile(&model).map(|tile| tile.name.as_str()), Some("Bo"));
    }

    #[test]
    fn speaker_is_the_pinned_tile_over_the_speaking_one() {
        let mut model = CallModel::test();
        model.tiles = vec![present("8:a", "Ada"), present("8:b", "Bo")];
        model.speaking_mris = vec!["8:b".to_owned()];
        model.toggle_pin("8:a");
        assert_eq!(speaker_tile(&model).map(|tile| tile.name.as_str()), Some("Ada"));
    }

    #[test]
    fn nobody_present_means_no_speaker() {
        let mut model = CallModel::test();
        model.tiles = vec![Tile::invited("8:a", "Ada")];
        assert!(speaker_tile(&model).is_none());
    }
}
