use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Once;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationCompletedEventHandler, NewWindowRequestedEventHandler, SourceChangedEventHandler,
    take_pwstr,
};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, HGDIOBJ, RGN_DIFF, SetWindowRgn,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HRESULT, HSTRING, Interface, PCWSTR, PWSTR, w};

use crate::embed::{EmbedEvent, EmbedSpec};
use crate::geometry::{EmbedBounds, local_cutouts};
use crate::host::environment_options;

const CONTAINER_CLASS: PCWSTR = w!("RustyTeamsEmbedContainer");
static REGISTER_CONTAINER_CLASS: Once = Once::new();

enum Environment {
    Idle,
    Creating(Vec<SharedEmbed>),
    Ready(ICoreWebView2Environment),
}

struct HostInner {
    user_data_folder: PathBuf,
    environment: RefCell<Environment>,
}

struct EmbedView {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

struct EmbedState {
    spec: EmbedSpec,
    container: HWND,
    view: Option<EmbedView>,
    bounds: Option<EmbedBounds>,
    visible: bool,
    cutouts: Vec<EmbedBounds>,
    closed: bool,
    events: UnboundedSender<EmbedEvent>,
}

type SharedEmbed = Rc<RefCell<EmbedState>>;

/// Creates embedded WebView2 views on the calling thread, which must pump Win32 messages.
#[derive(Clone)]
pub struct EmbedHost {
    inner: Rc<HostInner>,
}

#[derive(Clone)]
pub struct MainEmbed {
    state: SharedEmbed,
}

impl EmbedHost {
    pub fn new(user_data_folder: PathBuf) -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        EmbedHost {
            inner: Rc::new(HostInner {
                user_data_folder,
                environment: RefCell::new(Environment::Idle),
            }),
        }
    }

    pub fn open(&self, spec: EmbedSpec) -> Option<(MainEmbed, UnboundedReceiver<EmbedEvent>)> {
        let container = create_container(HWND(spec.parent as *mut _)).ok()?;
        let (events, receiver) = unbounded_channel();
        let state = Rc::new(RefCell::new(EmbedState {
            spec,
            container,
            view: None,
            bounds: None,
            visible: false,
            cutouts: Vec::new(),
            closed: false,
            events,
        }));
        self.request_controller(&state);
        Some((MainEmbed { state }, receiver))
    }

    fn request_controller(&self, embed: &SharedEmbed) {
        let ready = match &mut *self.inner.environment.borrow_mut() {
            Environment::Ready(environment) => Some(environment.clone()),
            Environment::Creating(waiting) => {
                waiting.push(embed.clone());
                return;
            }
            environment @ Environment::Idle => {
                *environment = Environment::Creating(vec![embed.clone()]);
                None
            }
        };
        match ready {
            Some(environment) => create_controller(&environment, embed),
            None => start_environment(&self.inner),
        }
    }
}

impl MainEmbed {
    pub fn place(&self, bounds: EmbedBounds, visible: bool) {
        {
            let mut state = self.state.borrow_mut();
            state.bounds = Some(bounds);
            state.visible = visible;
        }
        layout(&self.state);
    }

    pub fn set_cutouts(&self, cutouts: Vec<EmbedBounds>) {
        self.state.borrow_mut().cutouts = cutouts;
        layout(&self.state);
    }

    pub fn navigate(&self, url: String) {
        let webview = {
            let mut state = self.state.borrow_mut();
            match &state.view {
                Some(view) => Some((view.webview.clone(), url)),
                None => {
                    state.spec.url = url;
                    None
                }
            }
        };
        if let Some((webview, url)) = webview {
            unsafe {
                let _ = webview.Navigate(&HSTRING::from(url));
            }
        }
    }

    pub fn close(&self) {
        teardown(&self.state);
    }
}

fn start_environment(host: &Rc<HostInner>) {
    let completed = host.clone();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |code, environment| {
            environment_ready(&completed, code.ok().and(environment));
            Ok(())
        },
    ));
    let folder = HSTRING::from(host.user_data_folder.as_os_str());
    let options = environment_options();
    let started = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &folder, &options, &handler)
    };
    if started.is_err() {
        environment_ready(host, None);
    }
}

fn environment_ready(host: &Rc<HostInner>, environment: Option<ICoreWebView2Environment>) {
    let next = match &environment {
        Some(environment) => Environment::Ready(environment.clone()),
        None => Environment::Idle,
    };
    let Environment::Creating(waiting) = host.environment.replace(next) else {
        return;
    };
    for embed in waiting {
        match &environment {
            Some(environment) => create_controller(environment, &embed),
            None => fail(&embed),
        }
    }
}

fn create_controller(environment: &ICoreWebView2Environment, embed: &SharedEmbed) {
    let container = embed.borrow().container;
    let target = embed.clone();
    let handler =
        CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |code, controller| {
            let attached = code
                .and_then(|()| controller.ok_or_else(|| windows::core::Error::from(HRESULT(-1))))
                .and_then(|controller| {
                    attach(&target, controller.clone()).inspect_err(|_| unsafe {
                        let _ = controller.Close();
                    })
                });
            if attached.is_err() {
                fail(&target);
            }
            Ok(())
        }));
    if unsafe { environment.CreateCoreWebView2Controller(container, &handler) }.is_err() {
        fail(embed);
    }
}

fn attach(embed: &SharedEmbed, controller: ICoreWebView2Controller) -> windows::core::Result<()> {
    let (spec, events, closed) = {
        let state = embed.borrow();
        (state.spec.clone(), state.events.clone(), state.closed)
    };
    if closed {
        unsafe { controller.Close() }?;
        return Ok(());
    }
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
    watch_source(&webview, events.clone())?;
    watch_loaded(&webview, events.clone())?;
    forward_new_windows(&webview, events)?;
    embed.borrow_mut().view = Some(EmbedView {
        controller,
        webview: webview.clone(),
    });
    layout(embed);
    unsafe { webview.Navigate(&HSTRING::from(spec.url.as_str())) }
}

fn fail(embed: &SharedEmbed) {
    let events = embed.borrow().events.clone();
    if teardown(embed) {
        let _ = events.send(EmbedEvent::Closed);
    }
}

fn teardown(embed: &SharedEmbed) -> bool {
    let (controller, container) = {
        let mut state = embed.borrow_mut();
        if state.closed {
            return false;
        }
        state.closed = true;
        (
            state.view.take().map(|view| view.controller),
            state.container,
        )
    };
    unsafe {
        if let Some(controller) = controller {
            let _ = controller.Close();
        }
        let _ = DestroyWindow(container);
    }
    true
}

fn layout(embed: &SharedEmbed) {
    let (container, bounds, visible, cutouts, controller) = {
        let state = embed.borrow();
        if state.closed {
            return;
        }
        (
            state.container,
            state.bounds,
            state.visible,
            state.cutouts.clone(),
            state.view.as_ref().map(|view| view.controller.clone()),
        )
    };
    let Some(bounds) = bounds else {
        return;
    };
    let shown = visible && bounds.has_area();
    unsafe {
        let _ = SetWindowPos(
            container,
            None,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        apply_region(container, bounds, &cutouts);
        let _ = ShowWindow(container, if shown { SW_SHOWNA } else { SW_HIDE });
        if let Some(controller) = controller {
            let _ = controller.SetBounds(RECT {
                left: 0,
                top: 0,
                right: bounds.width,
                bottom: bounds.height,
            });
            let _ = controller.SetIsVisible(shown);
        }
    }
}

unsafe fn apply_region(container: HWND, content: EmbedBounds, cutouts: &[EmbedBounds]) {
    let local = local_cutouts(content, cutouts);
    if local.is_empty() {
        unsafe { SetWindowRgn(container, None, true) };
        return;
    }
    unsafe {
        let region = CreateRectRgn(0, 0, content.width, content.height);
        for cutout in local {
            let hole = CreateRectRgn(
                cutout.x,
                cutout.y,
                cutout.x + cutout.width,
                cutout.y + cutout.height,
            );
            CombineRgn(Some(region), Some(region), Some(hole), RGN_DIFF);
            let _ = DeleteObject(HGDIOBJ(hole.0));
        }
        SetWindowRgn(container, Some(region), true);
    }
}

fn create_container(parent: HWND) -> windows::core::Result<HWND> {
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }?.0);
    REGISTER_CONTAINER_CLASS.call_once(|| {
        let class = WNDCLASSW {
            lpfnWndProc: Some(container_procedure),
            hInstance: instance,
            lpszClassName: CONTAINER_CLASS,
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
    });
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CONTAINER_CLASS,
            w!(""),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(instance),
            None,
        )
    }
}

extern "system" fn container_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn watch_source(
    webview: &ICoreWebView2,
    events: UnboundedSender<EmbedEvent>,
) -> windows::core::Result<()> {
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            let mut source = PWSTR::null();
            if unsafe { sender.Source(&mut source) }.is_ok() {
                let _ = events.send(EmbedEvent::Navigated(take_pwstr(source)));
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_SourceChanged(&handler, &mut token) }
}

fn watch_loaded(
    webview: &ICoreWebView2,
    events: UnboundedSender<EmbedEvent>,
) -> windows::core::Result<()> {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_, _| {
        let _ = events.send(EmbedEvent::Loaded);
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NavigationCompleted(&handler, &mut token) }
}

fn forward_new_windows(
    webview: &ICoreWebView2,
    events: UnboundedSender<EmbedEvent>,
) -> windows::core::Result<()> {
    let handler = NewWindowRequestedEventHandler::create(Box::new(move |_, arguments| {
        let Some(arguments) = arguments else {
            return Ok(());
        };
        let mut target = PWSTR::null();
        unsafe {
            arguments.Uri(&mut target)?;
            arguments.SetHandled(true)?;
        }
        let _ = events.send(EmbedEvent::NewWindow(take_pwstr(target)));
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NewWindowRequested(&handler, &mut token) }
}
