//! Tray icon on Linux through the StatusNotifierItem D-Bus protocol (KDE Plasma
//! natively; GNOME with the AppIndicator extension).

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use chatsvc::ForcedKind;
use ksni::blocking::{Handle, TrayMethods as _};
use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::{Category, Icon, MenuItem, ToolTip};

use super::{TrayCommand, TrayPresence};
use crate::linux_desktop::DESKTOP_ID;
use crate::notify::tray_status;

const TRAY_ICON_SIZES: [u32; 3] = [22, 32, 64];
const DOT_COLOR: [u8; 4] = [0xef, 0x44, 0x44, 0xff];

#[derive(Clone)]
struct Labels {
    open: String,
    notifications: String,
    quit: String,
    duration: String,
    reset: String,
}

impl Labels {
    fn current() -> Labels {
        let tr = crate::ui_locale::tr;
        Labels {
            open: tr("tray.open"),
            notifications: tr("tray.notifications"),
            quit: tr("tray.quit"),
            duration: tray_status::duration_menu_label().to_owned(),
            reset: tray_status::reset_label().to_owned(),
        }
    }
}

struct Item {
    commands: Sender<TrayCommand>,
    labels: Labels,
    title: String,
    forced: Option<ForcedKind>,
    mention: bool,
    plain: Vec<Icon>,
    dotted: Vec<Icon>,
}

impl Item {
    fn send(&self, command: TrayCommand) {
        let _ = self.commands.send(command);
    }
}

/// Escapes the mnemonic underscores of a menu label.
fn menu_label(text: &str) -> String {
    text.replace('_', "__")
}

fn status_submenu(item: &Item) -> MenuItem<Item> {
    let mut entries = Vec::new();
    for kind in tray_status::STATUS_CHOICES {
        let label = tray_status::choice_label(kind);
        entries.push(
            CheckmarkItem {
                label: menu_label(label),
                checked: item.forced == Some(kind),
                activate: Box::new(move |item: &mut Item| {
                    item.send(TrayCommand::SetStatus(kind))
                }),
                ..Default::default()
            }
            .into(),
        );
    }
    entries.push(MenuItem::Separator);
    let duration_enabled = tray_status::duration_target(item.forced).is_some();
    let mut duration_items = Vec::new();
    for duration in tray_status::STATUS_DURATIONS {
        duration_items.push(
            StandardItem {
                label: menu_label(tray_status::duration_label(duration)),
                enabled: duration_enabled,
                activate: Box::new(move |item: &mut Item| {
                    item.send(TrayCommand::SetStatusDuration(duration))
                }),
                ..Default::default()
            }
            .into(),
        );
    }
    entries.push(
        SubMenu {
            label: menu_label(&item.labels.duration),
            enabled: duration_enabled,
            submenu: duration_items,
            ..Default::default()
        }
        .into(),
    );
    entries.push(
        StandardItem {
            label: menu_label(&item.labels.reset),
            activate: Box::new(|item: &mut Item| item.send(TrayCommand::ResetStatus)),
            ..Default::default()
        }
        .into(),
    );
    SubMenu {
        label: menu_label(&item.title),
        submenu: entries,
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        DESKTOP_ID.to_owned()
    }

    fn title(&self) -> String {
        crate::APP_NAME.to_owned()
    }

    fn category(&self) -> Category {
        Category::Communications
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        if self.mention {
            self.dotted.clone()
        } else {
            self.plain.clone()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: crate::APP_NAME.to_owned(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayCommand::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: menu_label(&self.labels.open),
                activate: Box::new(|item: &mut Self| item.send(TrayCommand::Open)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            status_submenu(self),
            StandardItem {
                label: menu_label(&self.labels.notifications),
                activate: Box::new(|item: &mut Self| item.send(TrayCommand::Notifications)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: menu_label(&self.labels.quit),
                icon_name: "application-exit".into(),
                activate: Box::new(|item: &mut Self| item.send(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct Tray {
    handle: Handle<Item>,
    commands: Mutex<Receiver<TrayCommand>>,
}

impl Tray {
    /// `None` when there is no session bus or no tray host (no StatusNotifierWatcher).
    pub fn new(presence: TrayPresence, icon_png: &[u8]) -> Option<Self> {
        let (plain, dotted) = tray_icons(icon_png)?;
        let (sender, receiver) = channel();
        let item = Item {
            commands: sender,
            labels: Labels::current(),
            title: presence.title,
            forced: presence.forced,
            mention: presence.mention,
            plain,
            dotted,
        };
        let handle = match item.spawn() {
            Ok(handle) => handle,
            Err(error) => {
                log::warn!("no tray icon: {error}");
                return None;
            }
        };
        Some(Tray {
            handle,
            commands: Mutex::new(receiver),
        })
    }

    pub fn relocalize(&self) {
        let labels = Labels::current();
        self.handle.update(move |item| item.labels = labels);
    }

    pub fn poll(&self) -> Vec<TrayCommand> {
        let Ok(receiver) = self.commands.lock() else {
            return Vec::new();
        };
        receiver.try_iter().collect()
    }

    pub fn set_app_icon(&mut self, icon_png: &[u8], mention: bool) {
        let Some((plain, dotted)) = tray_icons(icon_png) else {
            return;
        };
        self.handle.update(move |item| {
            item.plain = plain;
            item.dotted = dotted;
            item.mention = mention;
        });
    }

    pub fn sync(&self, presence: TrayPresence) {
        let labels = Labels::current();
        self.handle.update(move |item| {
            item.labels = labels;
            item.title = presence.title;
            item.forced = presence.forced;
            item.mention = presence.mention;
        });
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.handle.shutdown();
    }
}

/// The icon in every tray size, plain and with the unread dot, as ARGB32 big endian.
fn tray_icons(icon_png: &[u8]) -> Option<(Vec<Icon>, Vec<Icon>)> {
    let decoded = image::load_from_memory(icon_png).ok()?;
    let mut plain = Vec::new();
    let mut dotted = Vec::new();
    for size in TRAY_ICON_SIZES {
        let resized = decoded
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let mut with_dot = resized.clone();
        let radius = size as f32 * 0.22;
        let center = size as f32 - radius - 0.5;
        for (column, row, pixel) in with_dot.enumerate_pixels_mut() {
            let distance = ((column as f32 + 0.5 - center).powi(2)
                + (row as f32 + 0.5 - center).powi(2))
            .sqrt();
            if distance <= radius {
                *pixel = image::Rgba(DOT_COLOR);
            }
        }
        plain.push(argb_icon(size, resized.as_raw()));
        dotted.push(argb_icon(size, with_dot.as_raw()));
    }
    Some((plain, dotted))
}

fn argb_icon(size: u32, rgba: &[u8]) -> Icon {
    let data = rgba
        .chunks_exact(4)
        .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
        .collect();
    Icon {
        width: size as i32,
        height: size as i32,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_come_in_every_size_as_argb() {
        let png = crate::appearance::AppIcon::RustyTeams.png();
        let (plain, dotted) = tray_icons(png).unwrap();
        assert_eq!(plain.len(), TRAY_ICON_SIZES.len());
        for (icon, size) in plain.iter().zip(TRAY_ICON_SIZES) {
            assert_eq!((icon.width, icon.height), (size as i32, size as i32));
            assert_eq!(icon.data.len(), (size * size * 4) as usize);
        }
        let last = dotted.last().unwrap();
        let bottom_right = last.data.len() - 4 * (last.width as usize * 12 + 12);
        assert_eq!(
            &last.data[bottom_right..bottom_right + 4],
            &[0xff, 0xef, 0x44, 0x44]
        );
        assert!(tray_icons(b"not a png").is_none());
    }

    #[test]
    fn argb_moves_alpha_first() {
        let icon = argb_icon(1, &[1, 2, 3, 4]);
        assert_eq!(icon.data, vec![4, 1, 2, 3]);
    }

    #[test]
    fn underscores_stay_visible_in_menu_labels() {
        assert_eq!(menu_label("Do_not_disturb"), "Do__not__disturb");
    }

    #[test]
    fn status_submenu_lists_every_choice_and_reset() {
        let (sender, _receiver) = channel();
        let item = Item {
            commands: sender,
            labels: Labels::current(),
            title: "Available".into(),
            forced: Some(ForcedKind::Busy),
            mention: false,
            plain: Vec::new(),
            dotted: Vec::new(),
        };
        let MenuItem::SubMenu(menu) = status_submenu(&item) else {
            panic!("expected status submenu");
        };
        assert_eq!(menu.label, "Available");
        let labels: Vec<_> = menu
            .submenu
            .iter()
            .filter_map(|entry| match entry {
                MenuItem::Checkmark(item) => Some(item.label.as_str()),
                MenuItem::Standard(item) => Some(item.label.as_str()),
                MenuItem::SubMenu(item) => Some(item.label.as_str()),
                _ => None,
            })
            .collect();
        for kind in tray_status::STATUS_CHOICES {
            assert!(labels.contains(&tray_status::choice_label(kind)));
        }
        assert!(labels.contains(&tray_status::reset_label()));
        assert!(labels.contains(&tray_status::duration_menu_label()));
    }
}
