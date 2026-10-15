use std::ffi::c_void;
use std::io::Cursor;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use calling::ShareKind;
use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CLOAKED, DWMWA_COLOR_NONE, DWMWA_EXTENDED_FRAME_BOUNDS,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmGetWindowAttribute,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, DEVMODEW, DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICEW, DeleteObject,
    ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsW, GetMonitorInfoW, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromRect, MonitorFromWindow,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    ITaskbarList3, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME,
    QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, FLASHW_STOP, FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO,
    FlashWindowEx, GW_HWNDPREV, GWL_EXSTYLE, GWL_STYLE, GetWindow, GetWindowLongPtrW,
    GetWindowRect, HICON, HWND_NOTOPMOST, HWND_TOPMOST, ICONINFO, IsIconic, IsWindowVisible,
    LWA_ALPHA, SPI_GETCLIENTAREAANIMATION, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetForegroundWindow, SetLayeredWindowAttributes,
    SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    WDA_EXCLUDEFROMCAPTURE, WS_CAPTION, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, WS_THICKFRAME,
};
use windows::core::{BOOL, PCWSTR, w};

use super::stack_plan::{StackPlan, stack_plan};
use super::{NativeHandle, ScreenRect, ShareGeometry, TrayCommand, WorkArea};
use crate::notify::badge::{BADGE_SIZE, Badge, render_badge};
use crate::notify::ring_tone::{Ringtone, SAMPLE_RATE, ringback_cycle};

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
        let updated = if activatable {
            style & !flag
        } else {
            style | flag
        };
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

pub fn work_area_at(x: i32, y: i32) -> Option<WorkArea> {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let (mut dpi_x, mut dpi_y) = (0, 0);
        let scale = match GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) {
            Ok(()) if dpi_x > 0 => dpi_x as f32 / BASE_DPI,
            _ => 1.,
        };
        let RECT {
            left,
            top,
            right,
            bottom,
        } = info.rcWork;
        Some(WorkArea {
            left,
            top,
            right,
            bottom,
            scale,
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

fn is_topmost(window: HWND) -> bool {
    unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0 }
}

fn band_after(shared_topmost: bool, previous: Option<HWND>) -> HWND {
    match previous {
        Some(previous) if shared_topmost || !is_topmost(previous) => previous,
        _ if shared_topmost => HWND_TOPMOST,
        _ => HWND_NOTOPMOST,
    }
}

fn windows_above(shared: HWND, group: &[NativeHandle]) -> Vec<NativeHandle> {
    let mut above = Vec::new();
    let mut current = shared;
    while let Ok(previous) = unsafe { GetWindow(current, GW_HWNDPREV) } {
        above.push(previous.0 as NativeHandle);
        if !group.contains(&(previous.0 as NativeHandle)) {
            break;
        }
        current = previous;
    }
    above
}

pub fn place_rect(handle: NativeHandle, x: i32, y: i32, width: i32, height: i32) {
    unsafe {
        let _ = SetWindowPos(
            hwnd(handle),
            None,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER,
        );
    }
}

pub fn stack_group_above(shared: NativeHandle, group: &[NativeHandle]) {
    let shared = hwnd(shared);
    let above = windows_above(shared, group);
    let StackPlan::Rebuild { after } = stack_plan(&above, group) else {
        return;
    };
    let mut insert_after = band_after(is_topmost(shared), after.map(hwnd));
    for handle in group {
        unsafe {
            let _ = SetWindowPos(
                hwnd(*handle),
                Some(insert_after),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOMOVE | SWP_NOSIZE,
            );
        }
        insert_after = hwnd(*handle);
    }
}

pub fn show_window_inactive(handle: NativeHandle) {
    unsafe {
        let _ = ShowWindow(hwnd(handle), SW_SHOWNOACTIVATE);
    }
}

pub fn exclude_from_capture(handle: NativeHandle) {
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd(handle), WDA_EXCLUDEFROMCAPTURE);
    }
}

pub fn make_window_click_through(window: &Window) {
    if let Some(handle) = native_handle(window) {
        make_click_through(handle);
    }
}

fn make_click_through(handle: NativeHandle) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd(handle), GWL_EXSTYLE);
        let added = (WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE).0 as isize;
        SetWindowLongPtrW(hwnd(handle), GWL_EXSTYLE, style | added);
        // A layered window stays invisible until its alpha is set.
        let _ = SetLayeredWindowAttributes(hwnd(handle), COLORREF(0), u8::MAX, LWA_ALPHA);
    }
}

pub fn window_minimized(handle: NativeHandle) -> bool {
    unsafe { IsIconic(hwnd(handle)).as_bool() }
}

fn screen_rect(rect: RECT) -> ScreenRect {
    ScreenRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

pub fn window_rect(handle: NativeHandle) -> Option<ScreenRect> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd(handle), &mut rect).ok()? };
    Some(screen_rect(rect))
}

fn visible_frame(window: HWND) -> Option<ScreenRect> {
    let mut rect = RECT::default();
    unsafe {
        if IsIconic(window).as_bool() || !IsWindowVisible(window).as_bool() {
            return None;
        }
        let mut cloaked = 0u32;
        let queried = DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut c_void,
            size_of::<u32>() as u32,
        );
        if queried.is_ok() && cloaked != 0 {
            return None;
        }
        // Excludes the invisible resize margin that GetWindowRect includes.
        let framed = DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut c_void,
            size_of::<RECT>() as u32,
        );
        if framed.is_err() {
            GetWindowRect(window, &mut rect).ok()?;
        }
    }
    Some(screen_rect(rect))
}

fn screen_frame(index: u32) -> Option<ScreenRect> {
    unsafe {
        let mut device = DISPLAY_DEVICEW {
            cb: size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if !EnumDisplayDevicesW(PCWSTR::null(), index, &mut device, 0).as_bool()
            || device.StateFlags & DISPLAY_DEVICE_ACTIVE == Default::default()
        {
            return None;
        }
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if !EnumDisplaySettingsW(
            PCWSTR(device.DeviceName.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut mode,
        )
        .as_bool()
        {
            return None;
        }
        let position = mode.Anonymous1.Anonymous2.dmPosition;
        Some(ScreenRect {
            left: position.x,
            top: position.y,
            right: position.x + mode.dmPelsWidth as i32,
            bottom: position.y + mode.dmPelsHeight as i32,
        })
    }
}

fn geometry_around(target: ScreenRect) -> Option<ShareGeometry> {
    unsafe {
        let rect = RECT {
            left: target.left,
            top: target.top,
            right: target.right,
            bottom: target.bottom,
        };
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let (mut dpi_x, mut dpi_y) = (0, 0);
        let scale = match GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) {
            Ok(()) if dpi_x > 0 => dpi_x as f32 / BASE_DPI,
            _ => 1.,
        };
        let RECT {
            left,
            top,
            right,
            bottom,
        } = info.rcWork;
        Some(ShareGeometry {
            target,
            work: WorkArea {
                left,
                top,
                right,
                bottom,
                scale,
            },
        })
    }
}

pub fn share_geometry(kind: ShareKind, id: u64) -> Option<ShareGeometry> {
    let target = match kind {
        ShareKind::Screen => screen_frame(u32::try_from(id).ok()?)?,
        ShareKind::Window => visible_frame(hwnd(id as NativeHandle))?,
    };
    geometry_around(target)
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
        let Ok(taskbar) =
            CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
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

pub fn start_ring(ringtone: Ringtone) -> Option<RingTone> {
    start_cycle(ringtone.cycle(SAMPLE_RATE))
}

pub fn start_ringback() -> Option<RingTone> {
    start_cycle(ringback_cycle(SAMPLE_RATE))
}

pub fn play_preview(ringtone: Ringtone) {
    let cycle = ringtone.cycle(SAMPLE_RATE);
    std::thread::spawn(move || {
        let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return;
        };
        sink.log_on_drop(false);
        let (Some(channels), Some(rate)) = (NonZero::new(1u16), NonZero::new(SAMPLE_RATE)) else {
            return;
        };
        let player = rodio::Player::connect_new(sink.mixer());
        player.append(rodio::buffer::SamplesBuffer::new(channels, rate, cycle));
        player.sleep_until_end();
    });
}

fn start_cycle(cycle: Vec<f32>) -> Option<RingTone> {
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
        while !flag.load(Ordering::SeqCst) {
            let player = rodio::Player::connect_new(sink.mixer());
            player.append(rodio::buffer::SamplesBuffer::new(
                channels,
                rate,
                cycle.clone(),
            ));
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
        let distance =
            ((column as f32 - center.0).powi(2) + (row as f32 - center.1).powi(2)).sqrt();
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
