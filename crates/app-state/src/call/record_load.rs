use gpui_kit::Context;

use super::model::CallKind;
use crate::app_state::AppState;
use crate::runtime;

impl AppState {
    pub fn refresh_recording_policy(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let read = runtime::spawn(async move { engine.recording_policy().await.ok() });
        cx.spawn(async move |this, cx| {
            let Some(policy) = read.await.ok().flatten() else {
                return;
            };
            this.update(cx, |state, _| {
                state.recording_policy = policy;
                if let Some(call) = state.call.as_mut() {
                    call.model.recording_policy = policy;
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn apply_recording_rights(
        &mut self,
        conversation_id: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        call.model.recording_policy = self.recording_policy;
        call.model.own_tenant.clone_from(&self.own_tenant);
        if call.model.kind != CallKind::Meeting || self.own_tenant.is_some() {
            return;
        }
        let (Some(engine), Some(chat_id)) = (self.engine.clone(), conversation_id) else {
            return;
        };
        let chat_id = chat_id.to_owned();
        let call_id = call.id;
        let read = runtime::spawn(async move { engine.own_tenant_id(&chat_id).await.ok() });
        cx.spawn(async move |this, cx| {
            let Some(tenant) = read.await.ok().flatten() else {
                return;
            };
            this.update(cx, |state, cx| {
                state.own_tenant = Some(tenant.clone());
                if let Some(call) = state.call.as_mut().filter(|call| call.id == call_id) {
                    call.model.own_tenant = Some(tenant);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}
