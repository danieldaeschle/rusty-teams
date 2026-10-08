use std::rc::Rc;

use tokio::sync::mpsc::UnboundedSender;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
    NewWindowRequestedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
    MONITOR_FROM_FLAGS, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, Interface, PWSTR, w};

use crate::dialog::{DialogCommand, DialogEvent, DialogSpec};
use crate::host::{Shared, fit, shared as running_host};

const DIALOG_CLASS: windows::core::PCWSTR = w!("RustyTeamsDialog");
const DIALOG_STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_CAPTION.0 | WS_SYSMENU.0);
const BASE_DPI: u32 = 96;
const VIRTUAL_KEY_ESCAPE: u32 = 0x1B;
const PAGE_HEADERS: windows::core::PCWSTR =
    w!("Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store");
const NEW_WINDOW_KIND: &str = "newWindow";

struct DialogView {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

pub(crate) struct Dialog {
    window: HWND,
    view: Option<DialogView>,
    events: UnboundedSender<DialogEvent>,
}

struct HostPage {
    url: String,
    html: String,
}

pub(crate) fn open(
    shared: &Rc<Shared>,
    id: u64,
    spec: DialogSpec,
    events: UnboundedSender<DialogEvent>,
) {
    let Some(environment) = shared.environment.borrow().clone() else {
        let _ = events.send(DialogEvent::Closed);
        return;
    };
    let Ok(window) = create_window(&spec) else {
        let _ = events.send(DialogEvent::Closed);
        return;
    };
    shared.dialogs.borrow_mut().insert(
        id,
        Dialog {
            window,
            view: None,
            events,
        },
    );
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
    let page = HostPage {
        url: spec.page_url,
        html: spec.page_html,
    };
    let creator = environment.clone();
    let handler =
        CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |code, controller| {
            let Some(shared) = running_host() else {
                return Ok(());
            };
            let attached = code
                .and_then(|()| {
                    controller.ok_or_else(|| windows::core::Error::from(windows::core::HRESULT(-1)))
                })
                .and_then(|controller| {
                    attach(&shared, id, &environment, controller, page, spec.background)
                });
            if attached.is_err() {
                close(&shared, id);
            }
            Ok(())
        }));
    if unsafe { creator.CreateCoreWebView2Controller(window, &handler) }.is_err() {
        close(shared, id);
    }
}

pub(crate) fn run(shared: &Rc<Shared>, id: u64, command: DialogCommand) {
    match command {
        DialogCommand::Post(json) => {
            let webview = shared
                .dialogs
                .borrow()
                .get(&id)
                .and_then(|dialog| dialog.view.as_ref().map(|view| view.webview.clone()));
            if let Some(webview) = webview {
                unsafe {
                    let _ = webview.PostWebMessageAsJson(&HSTRING::from(json));
                }
            }
        }
        DialogCommand::Resize { width, height } => resize(shared, id, width, height),
        DialogCommand::Close => close(shared, id),
    }
}

pub(crate) fn close(shared: &Rc<Shared>, id: u64) {
    let Some(dialog) = shared.dialogs.borrow_mut().remove(&id) else {
        return;
    };
    unsafe {
        if let Some(view) = &dialog.view {
            let _ = view.controller.Close();
        }
        let _ = DestroyWindow(dialog.window);
    }
    let _ = dialog.events.send(DialogEvent::Closed);
}

pub(crate) fn close_all(shared: &Rc<Shared>) {
    let ids: Vec<u64> = shared.dialogs.borrow().keys().copied().collect();
    for id in ids {
        close(shared, id);
    }
}

fn id_of(shared: &Shared, window: HWND) -> Option<u64> {
    shared
        .dialogs
        .borrow()
        .iter()
        .find_map(|(&id, dialog)| (dialog.window == window).then_some(id))
}

fn emit(id: u64, event: DialogEvent) {
    if let Some(shared) = running_host()
        && let Some(dialog) = shared.dialogs.borrow().get(&id)
    {
        let _ = dialog.events.send(event);
    }
}

fn work_area(window: HWND, flags: MONITOR_FROM_FLAGS) -> RECT {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let monitor = MonitorFromWindow(window, flags);
        let _ = GetMonitorInfoW(monitor, &mut info);
    }
    info.rcWork
}

fn outer_size(client_width: i32, client_height: i32, dpi: u32, work: RECT) -> (i32, i32) {
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: client_width,
        bottom: client_height,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(
            &mut frame,
            DIALOG_STYLE,
            false,
            WINDOW_EX_STYLE::default(),
            dpi,
        );
    }
    (
        (frame.right - frame.left).min(work.right - work.left),
        (frame.bottom - frame.top).min(work.bottom - work.top),
    )
}

fn scaled(logical: u32, dpi: u32) -> i32 {
    (logical as u64 * dpi as u64 / BASE_DPI as u64) as i32
}

fn create_window(spec: &DialogSpec) -> windows::core::Result<HWND> {
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }?.0);
    let blue_green_red =
        (spec.background & 0xff) << 16 | (spec.background & 0xff00) | (spec.background >> 16);
    let class = WNDCLASSW {
        lpfnWndProc: Some(dialog_procedure),
        hInstance: instance,
        lpszClassName: DIALOG_CLASS,
        hbrBackground: unsafe { CreateSolidBrush(COLORREF(blue_green_red)) },
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let owner = spec.owner.map(|raw| HWND(raw as *mut _));
    let (dpi, work, owner_bounds) = match owner {
        Some(owner) => {
            let mut bounds = RECT::default();
            unsafe {
                let _ = GetWindowRect(owner, &mut bounds);
            }
            let dpi = unsafe { GetDpiForWindow(owner) }.max(BASE_DPI);
            (
                dpi,
                work_area(owner, MONITOR_DEFAULTTONEAREST),
                Some(bounds),
            )
        }
        None => (
            BASE_DPI,
            work_area(HWND::default(), MONITOR_DEFAULTTOPRIMARY),
            None,
        ),
    };
    let (width, height) = outer_size(scaled(spec.width, dpi), scaled(spec.height, dpi), dpi, work);
    let anchor = owner_bounds.unwrap_or(work);
    let left = (anchor.left + (anchor.right - anchor.left - width) / 2)
        .clamp(work.left, (work.right - width).max(work.left));
    let top = (anchor.top + (anchor.bottom - anchor.top - height) / 2)
        .clamp(work.top, (work.bottom - height).max(work.top));
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            DIALOG_CLASS,
            &HSTRING::from(spec.title.as_str()),
            DIALOG_STYLE,
            left,
            top,
            width,
            height,
            owner,
            None,
            Some(instance),
            None,
        )
    }
}

fn resize(shared: &Rc<Shared>, id: u64, width: Option<u32>, height: Option<u32>) {
    let Some(window) = shared.dialogs.borrow().get(&id).map(|dialog| dialog.window) else {
        return;
    };
    let dpi = unsafe { GetDpiForWindow(window) }.max(BASE_DPI);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(window, &mut client);
    }
    let client_width = width.map_or(client.right - client.left, |width| scaled(width, dpi));
    let client_height = height.map_or(client.bottom - client.top, |height| scaled(height, dpi));
    let (outer_width, outer_height) = outer_size(
        client_width,
        client_height,
        dpi,
        work_area(window, MONITOR_DEFAULTTONEAREST),
    );
    unsafe {
        let _ = SetWindowPos(
            window,
            None,
            0,
            0,
            outer_width,
            outer_height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn attach(
    shared: &Rc<Shared>,
    id: u64,
    environment: &ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    page: HostPage,
    background: u32,
) -> windows::core::Result<()> {
    let Some(window) = shared.dialogs.borrow().get(&id).map(|dialog| dialog.window) else {
        unsafe { controller.Close() }?;
        return Ok(());
    };
    let webview = unsafe { controller.CoreWebView2() }?;
    fit(window, &controller);
    unsafe {
        controller
            .cast::<ICoreWebView2Controller2>()?
            .SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 255,
                R: (background >> 16) as u8,
                G: (background >> 8) as u8,
                B: background as u8,
            })?;
        webview.Settings()?.SetIsStatusBarEnabled(false)?;
    }
    serve_host_page(&webview, environment, &page)?;
    forward_messages(&webview, id, &page.url)?;
    forward_new_windows(&webview, id)?;
    close_on_escape(&controller, window)?;
    unsafe {
        controller.SetIsVisible(true)?;
        webview.Navigate(&HSTRING::from(page.url.as_str()))?;
    }
    if let Some(dialog) = shared.dialogs.borrow_mut().get_mut(&id) {
        dialog.view = Some(DialogView {
            controller,
            webview,
        });
    }
    Ok(())
}

fn serve_host_page(
    webview: &ICoreWebView2,
    environment: &ICoreWebView2Environment,
    page: &HostPage,
) -> windows::core::Result<()> {
    let filter = HSTRING::from(page.url.as_str());
    unsafe {
        match webview.cast::<ICoreWebView2_22>() {
            Ok(webview) => webview.AddWebResourceRequestedFilterWithRequestSourceKinds(
                &filter,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?,
            Err(_) => webview.AddWebResourceRequestedFilter(
                &filter,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
            )?,
        }
    }
    let environment = environment.clone();
    let (url, html) = (page.url.clone(), page.html.clone());
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, arguments| {
        let Some(arguments) = arguments else {
            return Ok(());
        };
        let request = unsafe { arguments.Request() }?;
        let mut requested = PWSTR::null();
        unsafe { request.Uri(&mut requested) }?;
        if take_pwstr(requested) != url {
            return Ok(());
        }
        let stream = unsafe { SHCreateMemStream(Some(html.as_bytes())) }
            .ok_or_else(|| windows::core::Error::from(windows::core::HRESULT(-1)))?;
        let response =
            unsafe { environment.CreateWebResourceResponse(&stream, 200, w!("OK"), PAGE_HEADERS) }?;
        unsafe { arguments.SetResponse(&response) }
    }));
    let mut token = 0;
    unsafe { webview.add_WebResourceRequested(&handler, &mut token) }
}

fn forward_messages(webview: &ICoreWebView2, id: u64, page_url: &str) -> windows::core::Result<()> {
    let page_url = page_url.to_owned();
    let handler = WebMessageReceivedEventHandler::create(Box::new(move |_, arguments| {
        let Some(arguments) = arguments else {
            return Ok(());
        };
        let mut source = PWSTR::null();
        unsafe { arguments.Source(&mut source) }?;
        if take_pwstr(source) != page_url {
            return Ok(());
        }
        let mut message = PWSTR::null();
        if unsafe { arguments.TryGetWebMessageAsString(&mut message) }.is_ok() {
            emit(id, DialogEvent::Message(take_pwstr(message)));
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_WebMessageReceived(&handler, &mut token) }
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
        let message = serde_json::json!({"kind": NEW_WINDOW_KIND, "url": take_pwstr(target)});
        emit(id, DialogEvent::Message(message.to_string()));
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NewWindowRequested(&handler, &mut token) }
}

fn close_on_escape(
    controller: &ICoreWebView2Controller,
    window: HWND,
) -> windows::core::Result<()> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, arguments| {
        let Some(arguments) = arguments else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
        let mut key = 0;
        unsafe {
            arguments.KeyEventKind(&mut kind)?;
            arguments.VirtualKey(&mut key)?;
        }
        if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && key == VIRTUAL_KEY_ESCAPE {
            unsafe {
                arguments.SetHandled(true)?;
                PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))?;
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) }
}

extern "system" fn dialog_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Some(shared) = running_host() else {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    match message {
        WM_SIZE => {
            let id = id_of(&shared, window);
            let controller = id.and_then(|id| {
                shared
                    .dialogs
                    .borrow()
                    .get(&id)
                    .and_then(|dialog| dialog.view.as_ref().map(|view| view.controller.clone()))
            });
            if let Some(controller) = controller {
                fit(window, &controller);
            }
            LRESULT(0)
        }
        WM_CLOSE | WM_DESTROY => {
            if let Some(id) = id_of(&shared, window) {
                close(&shared, id);
            }
            if message == WM_CLOSE {
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
