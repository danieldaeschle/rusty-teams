use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use serde_json::Value;
use session::{App, Error, Result, TabCommand, TabEvent, is_login_url};
use tokio::sync::watch;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DevToolsProtocolEventReceivedEventHandler, NavigationCompletedEventHandler,
    ProcessFailedEventHandler, SourceChangedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, Interface, PCWSTR, PWSTR, w};

use crate::fanout::Subscribers;
use crate::host_dialog::{self, Dialog};
use crate::host_embed::{self, Embed};
use crate::transport::{HostState, UiRequest, WebViewTransport};
use crate::{BROWSER_ARGUMENTS, FORWARDED_EVENTS};

const WINDOW_CLASS: PCWSTR = w!("RustyTeamsWebView");
const WM_DRAIN: u32 = WM_APP;
const WM_RESTART: u32 = WM_APP + 1;
const LOGIN_TIMER: usize = 1;
const LOGIN_SHOW_DELAY_MS: u32 = 4000;
const PARK_TIMER: usize = 2;
const PARK_DELAY_MS: u32 = 60_000;
const LOGIN_PROMPT_INTERVAL: Duration = Duration::from_secs(600);
const WINDOW_WIDTH: i32 = 1100;
const WINDOW_HEIGHT: i32 = 800;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub user_data_folder: PathBuf,
    pub window_title: String,
}

struct View {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

pub(crate) struct Shared {
    config: HostConfig,
    windows: HashMap<App, HWND>,
    views: RefCell<HashMap<App, View>>,
    pub(crate) environment: RefCell<Option<ICoreWebView2Environment>>,
    pub(crate) dialogs: RefCell<HashMap<u64, Dialog>>,
    pub(crate) embeds: RefCell<HashMap<u64, Embed>>,
    subscribers: RefCell<Subscribers>,
    state: watch::Sender<HostState>,
    requests: mpsc::Receiver<UiRequest>,
    restarting: Cell<bool>,
    last_login_prompt: Cell<Option<Instant>>,
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

pub(crate) fn shared() -> Option<Rc<Shared>> {
    SHARED.with(|slot| slot.borrow().clone())
}

/// Starts the WebView2 UI thread; the transport answers once both apps are loaded.
pub fn start(config: HostConfig) -> Arc<WebViewTransport> {
    let (requests_sender, requests) = mpsc::channel();
    let (state_sender, state) = watch::channel(HostState::default());
    let (window_sender, window_receiver) = mpsc::channel::<isize>();
    let failure_state = state_sender.clone();
    let spawned = std::thread::Builder::new()
        .name("webview2".into())
        .spawn(move || run(config, requests, state_sender, window_sender));
    if let Err(error) = spawned {
        failure_state
            .send_modify(|state| state.failure = Some(format!("no webview thread: {error}")));
    }
    let window = window_receiver.recv().ok();
    let waker = Arc::new(move || {
        if let Some(raw) = window {
            unsafe {
                let _ = PostMessageW(Some(HWND(raw as *mut _)), WM_DRAIN, WPARAM(0), LPARAM(0));
            }
        }
    });
    Arc::new(WebViewTransport::new(requests_sender, waker, state))
}

fn run(
    config: HostConfig,
    requests: mpsc::Receiver<UiRequest>,
    state: watch::Sender<HostState>,
    window_sender: mpsc::Sender<isize>,
) {
    let fail = |reason: String| state.send_modify(|state| state.failure = Some(reason));
    if let Err(error) = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok() {
        return fail(format!("COM: {error}"));
    }
    let windows = match create_windows(&config.window_title) {
        Ok(windows) => windows,
        Err(error) => return fail(format!("window: {error}")),
    };
    let _ = window_sender.send(windows[&App::Teams].0 as isize);
    let shared = Rc::new(Shared {
        config,
        windows,
        views: RefCell::new(HashMap::new()),
        environment: RefCell::new(None),
        dialogs: RefCell::new(HashMap::new()),
        embeds: RefCell::new(HashMap::new()),
        subscribers: RefCell::new(Subscribers::default()),
        state,
        requests,
        restarting: Cell::new(false),
        last_login_prompt: Cell::new(None),
    });
    SHARED.with(|slot| *slot.borrow_mut() = Some(shared.clone()));
    load(&shared);
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn create_windows(title: &str) -> windows::core::Result<HashMap<App, HWND>> {
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }?.0);
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_procedure),
        hInstance: instance,
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let title = HSTRING::from(title);
    let mut windows = HashMap::new();
    for app in App::ALL {
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                WINDOW_CLASS,
                &title,
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                None,
                None,
                Some(instance),
                None,
            )
        }?;
        windows.insert(app, window);
    }
    Ok(windows)
}

fn load(shared: &Rc<Shared>) {
    match create_views(shared) {
        Ok(()) => shared.state.send_modify(|state| {
            state.ready = true;
            state.failure = None;
        }),
        Err(reason) => shared.state.send_modify(|state| {
            state.ready = false;
            state.failure = Some(reason);
        }),
    }
}

fn create_views(shared: &Rc<Shared>) -> std::result::Result<(), String> {
    let environment = create_environment(&shared.config.user_data_folder)?;
    *shared.environment.borrow_mut() = Some(environment.clone());
    for app in App::ALL {
        let view = create_view(shared, &environment, app)
            .map_err(|error| format!("{app} webview: {error}"))?;
        shared.views.borrow_mut().insert(app, view);
    }
    Ok(())
}

fn create_environment(
    user_data_folder: &std::path::Path,
) -> std::result::Result<ICoreWebView2Environment, String> {
    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        options.set_additional_browser_arguments(BROWSER_ARGUMENTS.to_owned());
        options.set_allow_single_sign_on_using_os_primary_account(true);
    }
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let folder = HSTRING::from(user_data_folder.as_os_str());
    let (sender, receiver) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &folder, &options, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |code, environment| {
            code?;
            let _ = sender.send(environment);
            Ok(())
        }),
    )
    .map_err(|error| format!("WebView2 runtime: {error}"))?;
    receiver
        .recv()
        .ok()
        .flatten()
        .ok_or_else(|| "WebView2 runtime returned no environment".to_owned())
}

fn create_view(
    shared: &Rc<Shared>,
    environment: &ICoreWebView2Environment,
    app: App,
) -> windows::core::Result<View> {
    let window = shared.windows[&app];
    let (sender, receiver) = mpsc::channel();
    let creator = environment.clone();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            creator
                .CreateCoreWebView2Controller(window, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |code, controller| {
            code?;
            let _ = sender.send(controller);
            Ok(())
        }),
    )
    .map_err(|error| match error {
        webview2_com::Error::WindowsError(error) => error,
        other => windows::core::Error::new(windows::core::HRESULT(-1), other.to_string()),
    })?;
    let controller =
        receiver.recv().ok().flatten().ok_or_else(|| {
            windows::core::Error::new(windows::core::HRESULT(-1), "no controller")
        })?;
    let webview = unsafe { controller.CoreWebView2() }?;
    fit(window, &controller);
    unsafe {
        controller.SetIsVisible(true)?;
        webview.Settings()?.SetIsStatusBarEnabled(false)?;
    }
    watch_source(&webview, app)?;
    watch_loads(&webview, app)?;
    watch_failures(&webview, app)?;
    forward_events(&webview, app)?;
    unsafe { webview.Navigate(&HSTRING::from(app.start_url())) }?;
    Ok(View {
        controller,
        webview,
    })
}

fn watch_source(webview: &ICoreWebView2, app: App) -> windows::core::Result<()> {
    let mut token = 0;
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
        if let (Some(sender), Some(shared)) = (sender, shared()) {
            on_source_changed(&shared, app, source_of(&sender));
        }
        Ok(())
    }));
    unsafe { webview.add_SourceChanged(&handler, &mut token) }
}

fn watch_loads(webview: &ICoreWebView2, app: App) -> windows::core::Result<()> {
    let mut token = 0;
    let handler = NavigationCompletedEventHandler::create(Box::new(move |sender, _| {
        let (Some(sender), Some(shared)) = (sender, shared()) else {
            return Ok(());
        };
        let url = current_url(&shared, app);
        if is_running_app(app, &url) {
            unsafe { SetTimer(Some(shared.windows[&app]), PARK_TIMER, PARK_DELAY_MS, None) };
        } else if app.is_park_url(&url) {
            release_memory(&sender);
        }
        Ok(())
    }));
    unsafe { webview.add_NavigationCompleted(&handler, &mut token) }
}

fn is_running_app(app: App, url: &str) -> bool {
    app.origins().iter().any(|origin| url.starts_with(origin)) && !app.is_park_url(url)
}

fn park(shared: &Shared, app: App) {
    if let Some(webview) = webview_of(shared, app)
        && is_running_app(app, &current_url(shared, app))
    {
        set_memory_level(&webview, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
        unsafe {
            let _ = webview.Navigate(&HSTRING::from(app.park_url()));
        }
    }
}

fn set_memory_level(webview: &ICoreWebView2, level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL) {
    if let Ok(webview) = webview.cast::<ICoreWebView2_19>() {
        unsafe {
            let _ = webview.SetMemoryUsageTargetLevel(level);
        }
    }
}

// The park page stays same-site, so the renderer keeps the app heap until it is told to shrink.
fn release_memory(webview: &ICoreWebView2) {
    let ignore = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    unsafe {
        let _ = webview.CallDevToolsProtocolMethod(
            w!("Memory.simulatePressureNotification"),
            w!(r#"{"level":"critical"}"#),
            &ignore,
        );
    }
}

fn watch_failures(webview: &ICoreWebView2, app: App) -> windows::core::Result<()> {
    let mut token = 0;
    let handler = ProcessFailedEventHandler::create(Box::new(move |sender, arguments| {
        let (Some(sender), Some(arguments), Some(shared)) = (sender, arguments, shared()) else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
        unsafe { arguments.ProcessFailedKind(&mut kind) }?;
        if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
            if !shared.restarting.replace(true) {
                unsafe {
                    PostMessageW(Some(shared.windows[&app]), WM_RESTART, WPARAM(0), LPARAM(0))
                }?;
            }
        } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
            || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
        {
            unsafe { sender.Reload() }?;
        }
        Ok(())
    }));
    unsafe { webview.add_ProcessFailed(&handler, &mut token) }
}

fn forward_events(webview: &ICoreWebView2, app: App) -> windows::core::Result<()> {
    for name in FORWARDED_EVENTS {
        let receiver = unsafe { webview.GetDevToolsProtocolEventReceiver(&HSTRING::from(name)) }?;
        let mut token = 0;
        let handler =
            DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, arguments| {
                let (Some(arguments), Some(shared)) = (arguments, shared()) else {
                    return Ok(());
                };
                let mut parameters = PWSTR::null();
                unsafe { arguments.ParameterObjectAsJson(&mut parameters) }?;
                let event = TabEvent {
                    method: name.to_owned(),
                    params: serde_json::from_str(&take_pwstr(parameters)).unwrap_or(Value::Null),
                };
                shared.subscribers.borrow_mut().publish(app, &event);
                Ok(())
            }));
        unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) }?;
    }
    Ok(())
}

fn source_of(webview: &ICoreWebView2) -> String {
    let mut source = PWSTR::null();
    match unsafe { webview.Source(&mut source) } {
        Ok(()) => take_pwstr(source),
        Err(_) => String::new(),
    }
}

fn on_source_changed(shared: &Shared, app: App, url: String) {
    let window = shared.windows[&app];
    let login = is_login_url(&url);
    let on_app = app.origins().iter().any(|origin| url.starts_with(origin));
    shared.state.send_modify(|state| {
        state.urls.insert(app, url);
    });
    unsafe {
        if login {
            SetTimer(Some(window), LOGIN_TIMER, LOGIN_SHOW_DELAY_MS, None);
        } else if on_app {
            let _ = KillTimer(Some(window), LOGIN_TIMER);
            let _ = ShowWindow(window, SW_HIDE);
        }
    }
}

fn webview_of(shared: &Shared, app: App) -> Option<ICoreWebView2> {
    shared
        .views
        .borrow()
        .get(&app)
        .map(|view| view.webview.clone())
}

fn current_url(shared: &Shared, app: App) -> String {
    shared
        .state
        .borrow()
        .urls
        .get(&app)
        .cloned()
        .unwrap_or_default()
}

fn drain(shared: &Rc<Shared>) {
    while let Ok(request) = shared.requests.try_recv() {
        match request {
            UiRequest::Call { app, command } => call(shared, app, command),
            UiRequest::Subscribe { app, sink } => shared.subscribers.borrow_mut().add(app, sink),
            UiRequest::Wake { app } => wake(shared, app),
            UiRequest::Retry => {
                if !shared.state.borrow().ready && !shared.restarting.replace(true) {
                    restart(shared);
                }
            }
            UiRequest::OpenDialog { id, spec, events } => {
                host_dialog::open(shared, id, spec, events)
            }
            UiRequest::Dialog { id, command } => host_dialog::run(shared, id, command),
            UiRequest::OpenEmbed { id, spec, events } => host_embed::open(shared, id, spec, events),
            UiRequest::Embed { id, command } => host_embed::run(shared, id, command),
        }
    }
}

fn call(shared: &Shared, app: App, command: TabCommand) {
    let Some(webview) = webview_of(shared, app) else {
        return command.respond(Err(Error::Cdp("webview is restarting".into())));
    };
    let method = HSTRING::from(command.method.as_str());
    let parameters = HSTRING::from(command.params.to_string());
    let pending = Rc::new(RefCell::new(Some(command)));
    let answered = pending.clone();
    let handler =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |code, json| {
            if let Some(command) = answered.borrow_mut().take() {
                command.respond(answer(code, &json));
            }
            Ok(())
        }));
    if let Err(error) =
        unsafe { webview.CallDevToolsProtocolMethod(&method, &parameters, &handler) }
        && let Some(command) = pending.borrow_mut().take()
    {
        let reason = format!("{}: {error}", command.method);
        command.respond(Err(Error::Cdp(reason)));
    }
}

fn answer(code: windows::core::Result<()>, json: &str) -> Result<Value> {
    let parsed = serde_json::from_str::<Value>(json).unwrap_or(Value::Null);
    match code {
        Ok(()) => Ok(parsed),
        Err(error) => {
            let reason = parsed
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| error.to_string());
            Err(Error::Cdp(reason))
        }
    }
}

fn wake(shared: &Shared, app: App) {
    let Some(webview) = webview_of(shared, app) else {
        return;
    };
    let url = current_url(shared, app);
    if is_login_url(&url) {
        prompt_login(shared, app);
        return;
    }
    set_memory_level(&webview, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
    unsafe {
        let _ = if is_running_app(app, &url) {
            webview.Reload()
        } else {
            webview.Navigate(&HSTRING::from(app.start_url()))
        };
    }
}

fn prompt_login(shared: &Shared, app: App) {
    let recently = shared
        .last_login_prompt
        .get()
        .is_some_and(|shown| shown.elapsed() < LOGIN_PROMPT_INTERVAL);
    let visible = unsafe { IsWindowVisible(shared.windows[&app]) }.as_bool();
    if !recently && !visible {
        shared.last_login_prompt.set(Some(Instant::now()));
        show(shared, app);
    }
}

fn show(shared: &Shared, app: App) {
    let window = shared.windows[&app];
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
    if let Some(view) = shared.views.borrow().get(&app) {
        fit(window, &view.controller);
    }
}

fn restart(shared: &Rc<Shared>) {
    shared.state.send_modify(|state| state.ready = false);
    shared.subscribers.borrow_mut().clear();
    host_dialog::close_all(shared);
    host_embed::close_all(shared);
    let views: Vec<View> = shared
        .views
        .borrow_mut()
        .drain()
        .map(|(_, view)| view)
        .collect();
    for view in views {
        unsafe {
            let _ = view.controller.Close();
        }
    }
    load(shared);
    shared.restarting.set(false);
}

pub(crate) fn fit(window: HWND, controller: &ICoreWebView2Controller) {
    let mut bounds = RECT::default();
    unsafe {
        if GetClientRect(window, &mut bounds).is_ok() {
            let _ = controller.SetBounds(bounds);
        }
    }
}

fn app_of(shared: &Shared, window: HWND) -> Option<App> {
    shared
        .windows
        .iter()
        .find_map(|(&app, &candidate)| (candidate == window).then_some(app))
}

extern "system" fn window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Some(shared) = shared() else {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    match message {
        WM_DRAIN => drain(&shared),
        WM_RESTART => restart(&shared),
        WM_SIZE => {
            if let Some(app) = app_of(&shared, window)
                && let Some(view) = shared.views.borrow().get(&app)
            {
                fit(window, &view.controller);
            }
        }
        WM_TIMER if wparam.0 == LOGIN_TIMER => {
            unsafe {
                let _ = KillTimer(Some(window), LOGIN_TIMER);
            }
            if let Some(app) = app_of(&shared, window)
                && is_login_url(&current_url(&shared, app))
            {
                show(&shared, app);
                shared.last_login_prompt.set(Some(Instant::now()));
            }
        }
        WM_TIMER if wparam.0 == PARK_TIMER => {
            unsafe {
                let _ = KillTimer(Some(window), PARK_TIMER);
            }
            if let Some(app) = app_of(&shared, window) {
                park(&shared, app);
            }
        }
        WM_CLOSE => unsafe {
            let _ = ShowWindow(window, SW_HIDE);
        },
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}
