use std::ffi::c_void;
use std::io::Cursor;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, DeleteObject, GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{
    ITaskbarList3, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN,
    SHQueryUserNotificationState, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, FLASHW_STOP, FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GWL_EXSTYLE, GWL_STYLE,
    GetWindowLongPtrW, HICON, HWND_TOPMOST, ICONINFO, IsIconic, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE,
    SPI_GETCLIENTAREAANIMATION, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, WS_CAPTION, WS_POPUP, WS_THICKFRAME, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    WS_EX_NOACTIVATE,
};
use windows::core::{BOOL, w};

use super::{NativeHandle, TrayCommand, WorkArea};
use crate::notify::badge::{BADGE_SIZE, Badge, render_badge};
use crate::notify::ring_tone::{SAMPLE_RATE, ring_cycle};

const SOUND_BYTES: &[u8] = include_bytes!("../../../assets/sounds/notification.mp3");
const TRAY_ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/teams-fast-256.png");
const TRAY_ICON_SIZE: u32 = 32;
const TRAY_DOT_RADIUS: f32 = 6.;
const BASE_DPI: f32 = 96.;
const RING_POLL: Duration = Duration::from_millis(20);

fn hwnd(handle: NativeHandle) -> HWND {
    HWND(handle as *mut c_void)
}

pub fn native_handle(window: &Window) -> Option<NativeHandle> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

pub fn prepare_toast_window(handle: NativeHandle) {
    unsafe {
        // GPUI creates popups with style 0, and Windows adds a caption frame to that on its own.
        let frame = (WS_CAPTION | WS_THICKFRAME).0 as isize;
        let style = GetWindowLongPtrW(hwnd(handle), GWL_STYLE);
        SetWindowLongPtrW(
            hwnd(handle),
            GWL_STYLE,
            (style & !frame) | WS_POPUP.0 as isize,
        );
        let _ = SetWindowPos(
            hwnd(handle),
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let style = GetWindowLongPtrW(hwnd(handle), GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd(handle),
            GWL_EXSTYLE,
            style | WS_EX_NOACTIVATE.0 as isize,
        );
        let no_border = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd(handle),
            DWMWA_BORDER_COLOR,
            &no_border as *const _ as *const c_void,
            size_of::<u32>() as u32,
        );
        let preference = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd(handle),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const c_void,
            size_of_val(&preference) as u32,
        );
    }
}

pub fn set_activatable(handle: NativeHandle, activatable: bool) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd(handle), GWL_EXSTYLE);
        let flag = WS_EX_NOACTIVATE.0 as isize;
        let updated = if activatable { style & !flag } else { style | flag };
        SetWindowLongPtrW(hwnd(handle), GWL_EXSTYLE, updated);
        if activatable {
            let _ = SetForegroundWindow(hwnd(handle));
        }
    }
}

pub fn work_area(handle: NativeHandle) -> Option<WorkArea> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd(handle), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let RECT {
            left,
            top,
            right,
            bottom,
        } = info.rcWork;
        let dpi = GetDpiForWindow(hwnd(handle));
        Some(WorkArea {
            left,
            top,
            right,
            bottom,
            scale: if dpi == 0 { 1. } else { dpi as f32 / BASE_DPI },
        })
    }
}

pub fn place(handle: NativeHandle, x: i32, y: i32, width: i32, height: i32) {
    unsafe {
        let _ = SetWindowPos(
            hwnd(handle),
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

pub fn system_quiet() -> bool {
    let state = unsafe { SHQueryUserNotificationState() };
    matches!(
        state,
        Ok(QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE | QUNS_QUIET_TIME)
    )
}

pub fn restore_and_raise(handle: NativeHandle) {
    unsafe {
        let command = if IsIconic(hwnd(handle)).as_bool() {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        let _ = ShowWindow(hwnd(handle), command);
        let _ = SetForegroundWindow(hwnd(handle));
    }
}

pub fn hide_window(handle: NativeHandle) {
    unsafe {
        let _ = ShowWindow(hwnd(handle), SW_HIDE);
    }
}

pub fn flash(handle: NativeHandle) {
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd: hwnd(handle),
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    unsafe {
        let _ = FlashWindowEx(&info);
    }
}

pub fn stop_flash(handle: NativeHandle) {
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd: hwnd(handle),
        dwFlags: FLASHW_STOP,
        uCount: 0,
        dwTimeout: 0,
    };
    unsafe {
        let _ = FlashWindowEx(&info);
    }
}

pub fn set_badge(handle: NativeHandle, badge: Option<&Badge>) {
    unsafe {
        let Ok(taskbar) = CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
        else {
            return;
        };
        if taskbar.HrInit().is_err() {
            return;
        }
        match badge {
            None => {
                let _ = taskbar.SetOverlayIcon(hwnd(handle), HICON::default(), w!(""));
            }
            Some(badge) => {
                let Some(icon) = icon_from_badge(badge) else {
                    return;
                };
                let _ = taskbar.SetOverlayIcon(hwnd(handle), icon, w!("Unread chats"));
                let _ = DestroyIcon(icon);
            }
        }
    }
}

unsafe fn icon_from_badge(badge: &Badge) -> Option<HICON> {
    let pixels = render_badge(badge);
    unsafe {
        let color = CreateBitmap(
            BADGE_SIZE as i32,
            BADGE_SIZE as i32,
            1,
            32,
            Some(pixels.as_ptr() as *const c_void),
        );
        let mask = CreateBitmap(BADGE_SIZE as i32, BADGE_SIZE as i32, 1, 1, None);
        let info = ICONINFO {
            fIcon: BOOL(1),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

pub struct RingTone {
    stop: Arc<AtomicBool>,
}

impl Drop for RingTone {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn start_ring() -> Option<RingTone> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return;
        };
        sink.log_on_drop(false);
        let (Some(channels), Some(rate)) = (NonZero::new(1u16), NonZero::new(SAMPLE_RATE)) else {
            return;
        };
        let cycle = ring_cycle(SAMPLE_RATE);
        while !flag.load(Ordering::SeqCst) {
            let player = rodio::Player::connect_new(sink.mixer());
            player.append(rodio::buffer::SamplesBuffer::new(channels, rate, cycle.clone()));
            while !player.empty() {
                if flag.load(Ordering::SeqCst) {
                    player.stop();
                    return;
                }
                std::thread::sleep(RING_POLL);
            }
        }
    });
    Some(RingTone { stop })
}

pub fn play_sound() {
    std::thread::spawn(|| {
        let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return;
        };
        sink.log_on_drop(false);
        if let Ok(player) = rodio::play(sink.mixer(), Cursor::new(SOUND_BYTES)) {
            player.sleep_until_end();
        }
    });
}

const ID_OPEN: &str = "open";
const ID_SOUND: &str = "sound";
const ID_DO_NOT_DISTURB: &str = "do_not_disturb";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";

pub struct Tray {
    icon: TrayIcon,
    sound: CheckMenuItem,
    do_not_disturb: CheckMenuItem,
    plain: Icon,
    with_dot: Icon,
}

impl Tray {
    pub fn new(sound_on: bool, do_not_disturb: bool) -> Option<Self> {
        let (plain, with_dot) = tray_icons()?;
        let open = MenuItem::with_id(ID_OPEN, "Open", true, None);
        let sound = CheckMenuItem::with_id(ID_SOUND, "Mute sound", true, !sound_on, None);
        let dnd = CheckMenuItem::with_id(
            ID_DO_NOT_DISTURB,
            "Do not disturb",
            true,
            do_not_disturb,
            None,
        );
        let settings = MenuItem::with_id(ID_SETTINGS, "Notifications", true, None);
        let quit = MenuItem::with_id(ID_QUIT, "Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &sound,
            &dnd,
            &settings,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .ok()?;
        let icon = TrayIconBuilder::new()
            .with_icon(plain.clone())
            .with_tooltip(crate::APP_NAME)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;
        Some(Tray {
            icon,
            sound,
            do_not_disturb: dnd,
            plain,
            with_dot,
        })
    }

    pub fn poll(&self) -> Vec<TrayCommand> {
        let mut commands = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let command = match event.id.0.as_str() {
                ID_OPEN => TrayCommand::Open,
                ID_SOUND => TrayCommand::ToggleSound,
                ID_DO_NOT_DISTURB => TrayCommand::ToggleDoNotDisturb,
                ID_SETTINGS => TrayCommand::Settings,
                ID_QUIT => TrayCommand::Quit,
                _ => continue,
            };
            commands.push(command);
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                commands.push(TrayCommand::Toggle);
            }
        }
        commands
    }

    pub fn sync(&self, sound_on: bool, do_not_disturb: bool, mention: bool) {
        self.sound.set_checked(!sound_on);
        self.do_not_disturb.set_checked(do_not_disturb);
        let icon = if mention { &self.with_dot } else { &self.plain };
        let _ = self.icon.set_icon(Some(icon.clone()));
    }
}

fn tray_icons() -> Option<(Icon, Icon)> {
    let decoded = image::load_from_memory(TRAY_ICON_PNG).ok()?;
    let resized = decoded
        .resize_exact(
            TRAY_ICON_SIZE,
            TRAY_ICON_SIZE,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgba8();
    let plain = resized.clone().into_raw();
    let mut dotted = resized;
    let size = TRAY_ICON_SIZE as f32;
    let center = (size - TRAY_DOT_RADIUS - 1., size - TRAY_DOT_RADIUS - 1.);
    for (column, row, pixel) in dotted.enumerate_pixels_mut() {
        let distance = ((column as f32 - center.0).powi(2) + (row as f32 - center.1).powi(2)).sqrt();
        if distance <= TRAY_DOT_RADIUS {
            *pixel = image::Rgba([0xef, 0x44, 0x44, 0xff]);
        }
    }
    Some((
        Icon::from_rgba(plain, TRAY_ICON_SIZE, TRAY_ICON_SIZE).ok()?,
        Icon::from_rgba(dotted.into_raw(), TRAY_ICON_SIZE, TRAY_ICON_SIZE).ok()?,
    ))
}

pub fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || enabled.as_bool()
}
