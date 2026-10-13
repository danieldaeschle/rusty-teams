use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::call_stage::{StageMode, render_stage};
use super::shortcuts::{ZoomShareIn, ZoomShareOut, ZoomShareReset};
use crate::app_state::AppState;
use crate::call::ZoomStep;
use crate::theme;

pub const POP_OUT_TITLE: &str = "Shared content";
pub const POP_OUT_SIZE: (f32, f32) = (960., 600.);
const KEY_CONTEXT: &str = "SharePopOut";

pub struct SharePopOutView {
    app: Entity<AppState>,
    focus: FocusHandle,
    _app_subscription: Subscription,
    _media_subscription: Option<Subscription>,
}

impl SharePopOutView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app_subscription = cx.observe(&app, |_, _, cx| cx.notify());
        let media_subscription = app
            .read(cx)
            .call_media
            .clone()
            .map(|media| cx.observe(&media, |_, _, cx| cx.notify()));
        let closing_app = app.clone();
        window.on_window_should_close(cx, move |_, cx| {
            closing_app.update(cx, |state, cx| state.return_stage_from_pop_out(cx));
            true
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        SharePopOutView {
            app,
            focus,
            _app_subscription: app_subscription,
            _media_subscription: media_subscription,
        }
    }
}

pub fn zoom_listener<A: Action>(
    app: &Entity<AppState>,
    step: ZoomStep,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let app = app.clone();
    move |_, _, cx| app.update(cx, |state, cx| state.zoom_share(step, cx))
}

impl Render for SharePopOutView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.app.read(cx);
        v_flex()
            .id("share-pop-out")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(theme::background())
            .on_action(zoom_listener::<ZoomShareIn>(&self.app, ZoomStep::In))
            .on_action(zoom_listener::<ZoomShareOut>(&self.app, ZoomStep::Out))
            .on_action(zoom_listener::<ZoomShareReset>(&self.app, ZoomStep::Reset))
            .children(render_stage(&self.app, state, StageMode::PopOut))
    }
}
