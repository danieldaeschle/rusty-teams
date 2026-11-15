//! Menu-bar / system-tray icon via `tray-icon` (Windows and macOS).

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use super::{TrayCommand, TrayPresence};
use crate::notify::tray_status;

const TRAY_ICON_SIZE: u32 = 32;
const TRAY_DOT_RADIUS: f32 = 6.;

const ID_OPEN: &str = "open";
const ID_NOTIFICATIONS: &str = "notifications";
const ID_QUIT: &str = "quit";
const ID_RESET_STATUS: &str = "status:reset";

pub struct Tray {
    icon: TrayIcon,
    open: MenuItem,
    status: Submenu,
    status_items: Vec<CheckMenuItem>,
    duration_items: Vec<MenuItem>,
    duration_menu: Submenu,
    reset: MenuItem,
    notifications: MenuItem,
    quit: MenuItem,
    plain: Icon,
    with_dot: Icon,
}

fn tray_menu_label(key: &str) -> String {
    crate::ui_locale::tr(key)
}

impl Tray {
    pub fn new(presence: TrayPresence, icon_png: &[u8]) -> Option<Self> {
        let (plain, with_dot) = tray_icons(icon_png)?;
        let open = MenuItem::with_id(ID_OPEN, tray_menu_label("tray.open"), true, None);
        let status_items: Vec<CheckMenuItem> = tray_status::STATUS_CHOICES
            .into_iter()
            .map(|kind| {
                CheckMenuItem::with_id(
                    tray_status::kind_id(kind),
                    tray_status::choice_label(kind),
                    true,
                    presence.forced == Some(kind),
                    None,
                )
            })
            .collect();
        let duration_enabled = tray_status::duration_target(presence.forced).is_some();
        let duration_items: Vec<MenuItem> = tray_status::STATUS_DURATIONS
            .into_iter()
            .map(|duration| {
                MenuItem::with_id(
                    tray_status::duration_id(duration),
                    duration.label(),
                    duration_enabled,
                    None,
                )
            })
            .collect();
        let duration_refs: Vec<&dyn tray_icon::menu::IsMenuItem> = duration_items
            .iter()
            .map(|item| item as &dyn tray_icon::menu::IsMenuItem)
            .collect();
        let duration_menu = Submenu::with_items(
            tray_status::duration_menu_label(),
            duration_enabled,
            &duration_refs,
        )
        .ok()?;
        let reset = MenuItem::with_id(ID_RESET_STATUS, tray_status::reset_label(), true, None);
        let mut status_entry_refs: Vec<&dyn tray_icon::menu::IsMenuItem> = status_items
            .iter()
            .map(|item| item as &dyn tray_icon::menu::IsMenuItem)
            .collect();
        let separator = PredefinedMenuItem::separator();
        status_entry_refs.push(&separator);
        status_entry_refs.push(&duration_menu);
        status_entry_refs.push(&reset);
        let status = Submenu::with_items(&presence.title, true, &status_entry_refs).ok()?;
        let notifications = MenuItem::with_id(
            ID_NOTIFICATIONS,
            tray_menu_label("tray.notifications"),
            true,
            None,
        );
        let quit = MenuItem::with_id(ID_QUIT, tray_menu_label("tray.quit"), true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &status,
            &notifications,
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
            open,
            status,
            status_items,
            duration_items,
            duration_menu,
            reset,
            notifications,
            quit,
            plain,
            with_dot,
        })
    }

    pub fn relocalize(&self) {
        let _ = self.open.set_text(tray_menu_label("tray.open"));
        let _ = self
            .notifications
            .set_text(tray_menu_label("tray.notifications"));
        let _ = self.quit.set_text(tray_menu_label("tray.quit"));
        let _ = self.reset.set_text(tray_status::reset_label());
        let _ = self
            .duration_menu
            .set_text(tray_status::duration_menu_label());
        for (item, kind) in self
            .status_items
            .iter()
            .zip(tray_status::STATUS_CHOICES.into_iter())
        {
            let _ = item.set_text(tray_status::choice_label(kind));
        }
        for (item, duration) in self
            .duration_items
            .iter()
            .zip(tray_status::STATUS_DURATIONS.into_iter())
        {
            let _ = item.set_text(duration.label());
        }
    }

    pub fn poll(&self) -> Vec<TrayCommand> {
        let mut commands = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id.0.as_str();
            let command = match id {
                ID_OPEN => TrayCommand::Open,
                ID_NOTIFICATIONS => TrayCommand::Notifications,
                ID_QUIT => TrayCommand::Quit,
                ID_RESET_STATUS => TrayCommand::ResetStatus,
                other => {
                    if let Some(kind) = tray_status::parse_kind_id(other) {
                        TrayCommand::SetStatus(kind)
                    } else if let Some(duration) = tray_status::parse_duration_id(other) {
                        TrayCommand::SetStatusDuration(duration)
                    } else {
                        continue;
                    }
                }
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

    pub fn set_app_icon(&mut self, icon_png: &[u8], mention: bool) {
        let Some((plain, with_dot)) = tray_icons(icon_png) else {
            return;
        };
        self.plain = plain;
        self.with_dot = with_dot;
        let icon = if mention { &self.with_dot } else { &self.plain };
        let _ = self.icon.set_icon(Some(icon.clone()));
    }

    pub fn sync(&self, presence: TrayPresence) {
        self.relocalize();
        let _ = self.status.set_text(&presence.title);
        for (item, kind) in self
            .status_items
            .iter()
            .zip(tray_status::STATUS_CHOICES.into_iter())
        {
            item.set_checked(presence.forced == Some(kind));
        }
        let duration_enabled = tray_status::duration_target(presence.forced).is_some();
        let _ = self.duration_menu.set_enabled(duration_enabled);
        for item in &self.duration_items {
            let _ = item.set_enabled(duration_enabled);
        }
        let icon = if presence.mention {
            &self.with_dot
        } else {
            &self.plain
        };
        let _ = self.icon.set_icon(Some(icon.clone()));
    }
}

fn tray_icons(icon_png: &[u8]) -> Option<(Icon, Icon)> {
    let decoded = image::load_from_memory(icon_png).ok()?;
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
