use calling::CallCommand;
use gpui_kit::*;

use super::share_view::{
    SHARE_MOTION_META_KEY, SHARE_STANDOUT_META_KEY, STRIP_WIDTH_META_KEY, StripDrag, clamp_strip_width,
};
use crate::app_state::{AppEvent, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomStep {
    In,
    Out,
    Reset,
}

fn flag_text(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

impl AppState {
    fn change_share_stage(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut super::share_view::ShareStage)) {
        let Some(call) = self.call.as_mut().filter(|call| call.model.screen_sharer.is_some()) else {
            return;
        };
        change(&mut call.share_stage);
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn zoom_share_wheel(&mut self, scroll_pixels: f32, position: (f32, f32), cx: &mut Context<Self>) {
        self.change_share_stage(cx, |stage| {
            let (pointer, size) = (stage.pointer(position), stage.size());
            stage.view.zoom_wheel(scroll_pixels, pointer, size);
        });
    }

    pub fn zoom_share(&mut self, step: ZoomStep, cx: &mut Context<Self>) {
        self.change_share_stage(cx, |stage| {
            let size = stage.size();
            match step {
                ZoomStep::In => stage.view.step_in(size),
                ZoomStep::Out => stage.view.step_out(size),
                ZoomStep::Reset => stage.view.reset(),
            }
        });
    }

    pub fn begin_share_pan(&mut self, position: (f32, f32), cx: &mut Context<Self>) {
        self.change_share_stage(cx, |stage| stage.pan_from = Some(position));
    }

    pub fn pan_share_to(&mut self, position: (f32, f32), cx: &mut Context<Self>) {
        self.change_share_stage(cx, |stage| {
            let Some(from) = stage.pan_from.replace(position) else {
                return;
            };
            let size = stage.size();
            stage.view.pan_by((position.0 - from.0, position.1 - from.1), size);
        });
    }

    pub fn end_share_pan(&mut self, cx: &mut Context<Self>) {
        self.change_share_stage(cx, |stage| stage.pan_from = None);
    }

    pub fn begin_strip_drag(&mut self, pointer_x: f32, cx: &mut Context<Self>) {
        let start_width = self.call_strip_width;
        self.change_share_stage(cx, |stage| stage.strip_drag = Some(StripDrag { start_pointer: pointer_x, start_width }));
    }

    pub fn drag_strip_to(&mut self, pointer_x: f32, cx: &mut Context<Self>) {
        let Some(drag) = self.call.as_ref().and_then(|call| call.share_stage.strip_drag) else {
            return;
        };
        self.call_strip_width = drag.width_at(pointer_x);
        cx.notify();
    }

    pub fn end_strip_drag(&mut self, cx: &mut Context<Self>) {
        if self.call.as_ref().is_none_or(|call| call.share_stage.strip_drag.is_none()) {
            return;
        }
        let width = clamp_strip_width(self.call_strip_width);
        self.call_strip_width = width;
        let _ = self.store.set_meta(STRIP_WIDTH_META_KEY, &width.to_string());
        self.change_share_stage(cx, |stage| stage.strip_drag = None);
    }

    pub fn pop_out_stage(&mut self, cx: &mut Context<Self>) {
        self.change_share_stage(cx, |stage| stage.popped = true);
    }

    pub fn return_stage_from_pop_out(&mut self, cx: &mut Context<Self>) {
        self.change_share_stage(cx, |stage| stage.popped = false);
    }

    pub fn set_call_share_motion(&mut self, on: bool, cx: &mut Context<Self>) {
        self.call_share_motion = on;
        let _ = self.store.set_meta(SHARE_MOTION_META_KEY, flag_text(on));
        self.send_call_command(CallCommand::SetShareMotion(on));
        cx.notify();
    }

    pub fn set_call_share_standout(&mut self, on: bool, cx: &mut Context<Self>) {
        self.call_share_standout = on;
        let _ = self.store.set_meta(SHARE_STANDOUT_META_KEY, flag_text(on));
        self.send_call_command(CallCommand::SetShareStandout(on));
        cx.notify();
    }
}
