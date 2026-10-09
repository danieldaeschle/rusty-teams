use std::rc::Rc;

use tokio::sync::mpsc::UnboundedSender;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, NewWindowRequestedEventHandler,
    SourceChangedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::core::{HSTRING, Interface, PWSTR};

use crate::embed::{EmbedBounds, EmbedCommand, EmbedEvent, EmbedSpec};
use crate::host::{Shared, shared as running_host};

struct EmbedView {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

pub(crate) struct Embed {
    view: Option<EmbedView>,
    bounds: Option<EmbedBounds>,
    visible: bool,
    events: UnboundedSender<EmbedEvent>,
}

pub(crate) fn open(
    shared: &Rc<Shared>,
    id: u64,
    spec: EmbedSpec,
    events: UnboundedSender<EmbedEvent>,
) {
    let Some(environment) = shared.environment.borrow().clone() else {
        let _ = events.send(EmbedEvent::Closed);
        return;
    };
    shared.embeds.borrow_mut().insert(
        id,
        Embed {
            view: None,
            bounds: None,
            visible: false,
            events,
        },
    );
    let parent = HWND(spec.parent as *mut _);
    let handler =
        CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |code, controller| {
            let Some(shared) = running_host() else {
                return Ok(());
            };
            let attached = code
                .and_then(|()| {
                    controller.ok_or_else(|| windows::core::Error::from(windows::core::HRESULT(-1)))
                })
                .and_then(|controller| attach(&shared, id, controller, &spec));
            if attached.is_err() {
                close(&shared, id);
            }
            Ok(())
        }));
    if unsafe { environment.CreateCoreWebView2Controller(parent, &handler) }.is_err() {
        close(shared, id);
    }
}

pub(crate) fn run(shared: &Rc<Shared>, id: u64, command: EmbedCommand) {
    match command {
        EmbedCommand::Bounds(bounds) => {
            let controller = update(shared, id, |embed| embed.bounds = Some(bounds));
            if let Some(controller) = controller {
                apply_bounds(&controller, bounds);
            }
        }
        EmbedCommand::Visible(visible) => {
            let controller = update(shared, id, |embed| embed.visible = visible);
            if let Some(controller) = controller {
                unsafe {
                    let _ = controller.SetIsVisible(visible);
                }
            }
        }
        EmbedCommand::Navigate(url) => {
            let webview = shared
                .embeds
                .borrow()
                .get(&id)
                .and_then(|embed| embed.view.as_ref().map(|view| view.webview.clone()));
            if let Some(webview) = webview {
                unsafe {
                    let _ = webview.Navigate(&HSTRING::from(url));
                }
            }
        }
        EmbedCommand::Close => close(shared, id),
    }
}

pub(crate) fn close(shared: &Rc<Shared>, id: u64) {
    let Some(embed) = shared.embeds.borrow_mut().remove(&id) else {
        return;
    };
    if let Some(view) = &embed.view {
        unsafe {
            let _ = view.controller.Close();
        }
    }
    let _ = embed.events.send(EmbedEvent::Closed);
}

pub(crate) fn close_all(shared: &Rc<Shared>) {
    let ids: Vec<u64> = shared.embeds.borrow().keys().copied().collect();
    for id in ids {
        close(shared, id);
    }
}

fn update(
    shared: &Shared,
    id: u64,
    change: impl FnOnce(&mut Embed),
) -> Option<ICoreWebView2Controller> {
    let mut embeds = shared.embeds.borrow_mut();
    let embed = embeds.get_mut(&id)?;
    change(embed);
    embed.view.as_ref().map(|view| view.controller.clone())
}

fn emit(id: u64, event: EmbedEvent) {
    if let Some(shared) = running_host()
        && let Some(embed) = shared.embeds.borrow().get(&id)
    {
        let _ = embed.events.send(event);
    }
}

fn apply_bounds(controller: &ICoreWebView2Controller, bounds: EmbedBounds) {
    unsafe {
        let _ = controller.SetBounds(RECT {
            left: bounds.x,
            top: bounds.y,
            right: bounds.x + bounds.width,
            bottom: bounds.y + bounds.height,
        });
    }
}

fn attach(
    shared: &Rc<Shared>,
    id: u64,
    controller: ICoreWebView2Controller,
    spec: &EmbedSpec,
) -> windows::core::Result<()> {
    let Some((bounds, visible)) = shared
        .embeds
        .borrow()
        .get(&id)
        .map(|embed| (embed.bounds, embed.visible))
    else {
        unsafe { controller.Close() }?;
        return Ok(());
    };
    let webview = unsafe { controller.CoreWebView2() }?;
    unsafe {
        controller
            .cast::<ICoreWebView2Controller2>()?
            .SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 255,
                R: (spec.background >> 16) as u8,
                G: (spec.background >> 8) as u8,
                B: spec.background as u8,
            })?;
        webview.Settings()?.SetIsStatusBarEnabled(false)?;
    }
    if let Some(bounds) = bounds {
        apply_bounds(&controller, bounds);
    }
    watch_source(&webview, id)?;
    forward_new_windows(&webview, id)?;
    unsafe {
        controller.SetIsVisible(visible)?;
        webview.Navigate(&HSTRING::from(spec.url.as_str()))?;
    }
    if let Some(embed) = shared.embeds.borrow_mut().get_mut(&id) {
        embed.view = Some(EmbedView {
            controller,
            webview,
        });
    }
    Ok(())
}

fn watch_source(webview: &ICoreWebView2, id: u64) -> windows::core::Result<()> {
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            let mut source = PWSTR::null();
            if unsafe { sender.Source(&mut source) }.is_ok() {
                emit(id, EmbedEvent::Navigated(take_pwstr(source)));
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_SourceChanged(&handler, &mut token) }
}

fn forward_new_windows(webview: &ICoreWebView2, id: u64) -> windows::core::Result<()> {
    let handler = NewWindowRequestedEventHandler::create(Box::new(move |_, arguments| {
        let Some(arguments) = arguments else {
            return Ok(());
        };
        let mut target = PWSTR::null();
        unsafe {
            arguments.Uri(&mut target)?;
            arguments.SetHandled(true)?;
        }
        emit(id, EmbedEvent::NewWindow(take_pwstr(target)));
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NewWindowRequested(&handler, &mut token) }
}
