use std::sync::{Arc, OnceLock};

use gpui_kit::*;
use serde_json::json;
use teams_core::DialogIdentity;
use webview::{DialogEvent, DialogHandle, DialogMode, DialogSpec, WebViewTransport};

use super::sdk::{self, Effect, HostContext, HostPage};
use super::{UrlDialog, UrlDialogNext};
use crate::app_state::{AppHandle, AppState};
use crate::backend::Engine;
use crate::{notify, runtime, theme};

static HOST: OnceLock<Arc<WebViewTransport>> = OnceLock::new();
const CALL_WINDOW_WIDTH: u32 = 1100;
const CALL_WINDOW_HEIGHT: u32 = 760;

pub fn install_host(transport: Arc<WebViewTransport>) {
    let _ = HOST.set(transport);
}

pub fn open(dialog: UrlDialog, engine: Option<Arc<Engine>>, cx: &mut App) {
    let Some(host) = HOST.get().cloned() else {
        cx.open_url(&dialog.fallback_url);
        return;
    };
    let state = cx.global::<AppHandle>().0.clone();
    let owner = cx
        .active_window()
        .and_then(|window| {
            window
                .update(cx, |_, window, _| notify::native_handle(window))
                .ok()
        })
        .flatten();
    cx.spawn(async move |cx| run(host, dialog, state, engine, owner, cx).await)
        .detach();
}

/// Teams on the web in its own window, signed in through the shared WebView2 profile.
pub fn open_call_window(url: String, title: String, cx: &mut App) -> bool {
    let Some(host) = HOST.get().cloned() else {
        return false;
    };
    let (_handle, mut events) = host.open_dialog(DialogSpec {
        mode: DialogMode::Call,
        title,
        width: CALL_WINDOW_WIDTH,
        height: CALL_WINDOW_HEIGHT,
        background: theme::background_hex(),
        owner: None,
        page_url: url,
        page_html: String::new(),
    });
    cx.spawn(async move |cx| {
        while let Some(DialogEvent::Message(raw)) = events.recv().await {
            let link = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|message| message.get("url")?.as_str().map(str::to_owned));
            if let Some(link) = link {
                cx.update(|cx| cx.open_url(&link));
            }
        }
    })
    .detach();
    true
}

async fn run(
    host: Arc<WebViewTransport>,
    dialog: UrlDialog,
    state: Entity<AppState>,
    engine: Option<Arc<Engine>>,
    owner: Option<isize>,
    cx: &mut AsyncApp,
) {
    let identity = fetch_identity(engine, &dialog.scope.conversation_id).await;
    let context = HostContext {
        user_id: identity.user_id,
        user_display_name: identity.display_name,
        tenant_id: identity.tenant_id,
        conversation_id: dialog.scope.conversation_id.clone(),
        app_id: dialog.app_id.clone(),
        session_id: uuid::Uuid::new_v4().to_string(),
        dark: true,
        web_application_resource: dialog.web_application_resource.clone(),
    };
    let page_html = HostPage {
        task_url: &dialog.url,
        background: theme::background_hex(),
        token_acquirer_script: &session::token_acquirer_script(),
    }
    .html();
    let (handle, mut events) = host.open_dialog(DialogSpec {
        mode: DialogMode::Hosted,
        title: dialog.title.clone(),
        width: dialog.width,
        height: dialog.height,
        background: theme::background_hex(),
        owner,
        page_url: sdk::host_page_url(),
        page_html,
    });
    while let Some(DialogEvent::Message(raw)) = events.recv().await {
        let handled = sdk::handle(&raw, &context);
        for directive in &handled.directives {
            handle.post(directive.to_string());
        }
        if let Some(effect) = handled.effect {
            apply(effect, &dialog, &state, &handle, cx).await;
        }
    }
}

async fn fetch_identity(engine: Option<Arc<Engine>>, conversation_id: &str) -> DialogIdentity {
    let empty = DialogIdentity {
        user_id: String::new(),
        display_name: String::new(),
        tenant_id: String::new(),
    };
    let Some(engine) = engine else {
        return empty;
    };
    let conversation_id = conversation_id.to_owned();
    let lookup = runtime::spawn(async move { engine.dialog_identity(&conversation_id).await.ok() });
    lookup.await.ok().flatten().unwrap_or(empty)
}

async fn apply(
    effect: Effect,
    dialog: &UrlDialog,
    state: &Entity<AppState>,
    handle: &DialogHandle,
    cx: &mut AsyncApp,
) {
    match effect {
        Effect::Resize { width, height } => handle.resize(width, height),
        Effect::OpenLink(link) => {
            cx.update(|cx| cx.open_url(&link));
        }
        Effect::Submit(result) => {
            let scope = dialog.scope.clone();
            let next = state
                .update(cx, |state, cx| state.submit_url_dialog(scope, result, cx))
                .await;
            match next {
                UrlDialogNext::Close => handle.close(),
                UrlDialogNext::Navigate { url, width, height } => {
                    handle.resize(Some(width), Some(height));
                    handle.post(json!({"kind": "load", "url": url}).to_string());
                }
            }
        }
    }
}
