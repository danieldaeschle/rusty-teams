use std::time::{Duration, Instant};

use calling::camera::{DEFAULT_CAMERA_LABEL, list_cameras};
use calling::{DeviceChoice, DeviceEntry, camera_entries};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::Button,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::center::NotificationCenter;
use super::platform;
use super::ring_tone::Ringtone;
use super::rules::Settings;
use super::settings_mcp::McpView;
use crate::app_state::AppState;
use crate::call::{BackgroundPick, CameraPreview, preview_backgrounds};
use crate::theme;

const FPS_TICK: Duration = Duration::from_millis(500);
const SELECT_WIDTH: f32 = 170.;
const PREVIEW_WIDTH: f32 = 380.;
const PREVIEW_HEIGHT: f32 = 214.;
const PREVIEW_RADIUS: f32 = 10.;
const CAMERA_IN_USE: &str = "Camera is in use by the call";

fn camera_in_call(state: &AppState) -> bool {
    state.call.is_some()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Notifications,
    Devices,
    Mcp,
}

impl Section {
    const ALL: [Section; 3] = [Section::Notifications, Section::Devices, Section::Mcp];

    fn label(self) -> &'static str {
        match self {
            Section::Notifications => "Notifications",
            Section::Devices => "Devices",
            Section::Mcp => "MCP",
        }
    }
}

type Accessor = fn(&mut Settings) -> &mut bool;

struct Row {
    title: &'static str,
    detail: &'static str,
    field: Accessor,
}

const ROWS: [Row; 5] = [
    Row {
        title: "Sound",
        detail: "Messages and mentions",
        field: |settings| &mut settings.sound,
    },
    Row {
        title: "Mentions only",
        detail: "Direct messages and @mentions",
        field: |settings| &mut settings.mentions_only,
    },
    Row {
        title: "Show preview",
        detail: "Off: only \"New message\"",
        field: |settings| &mut settings.preview,
    },
    Row {
        title: "Flash taskbar",
        detail: "On new messages, until you open the window",
        field: |settings| &mut settings.flash,
    },
    Row {
        title: "Minimize to tray on close",
        detail: "The title bar X hides the window",
        field: |settings| &mut settings.close_to_tray,
    },
];

pub struct SettingsView {
    center: Entity<NotificationCenter>,
    section: Section,
    mcp: Entity<McpView>,
    preview: CameraPreview,
    cameras: Vec<DeviceEntry>,
    cameras_requested: bool,
    camera: DeviceChoice,
    background: BackgroundPick,
    preview_blocked: bool,
    _subscription: Subscription,
    _app_subscription: Subscription,
    _release: Subscription,
}

impl SettingsView {
    pub const WIDTH: f32 = 420.;
    pub const HEIGHT: f32 = 540.;

    pub fn new(
        center: Entity<NotificationCenter>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&center, |_, _, cx| cx.notify());
        let app = center.read(cx).app().clone();
        let app_subscription = cx.observe(&app, |view, _, cx| view.sync_preview_block(cx));
        let release = cx.on_release(|view, cx| view.preview.stop(cx));
        let (preview, mut updates) = CameraPreview::new();
        let mcp = cx.new(|cx| McpView::new(app.clone(), window, cx));
        let state = app.read(cx);
        let camera = state.call_camera.clone();
        let preview_blocked = camera_in_call(state);
        let background = Some(state.call_background.clone())
            .filter(|pick| preview_backgrounds(&state.call_backgrounds).iter().any(|(known, _)| known == pick))
            .unwrap_or_default();
        cx.spawn(async move |this, cx| {
            while updates.recv().await.is_some() {
                if this.update(cx, |view, cx| view.take_frames(cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FPS_TICK).await;
                if this.update(cx, |view, cx| view.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        SettingsView {
            center,
            section: Section::Notifications,
            mcp,
            preview,
            cameras: camera_entries(&[]),
            cameras_requested: false,
            camera,
            background,
            preview_blocked,
            _subscription: subscription,
            _app_subscription: app_subscription,
            _release: release,
        }
    }

    fn take_frames(&mut self, cx: &mut Context<Self>) {
        self.preview.take_frames(cx);
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.section == Section::Devices {
            self.preview.tick(Instant::now());
            cx.notify();
        }
    }

    fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section == section {
            return;
        }
        self.section = section;
        match section {
            Section::Devices => {
                self.load_cameras(cx);
                self.apply_background(cx);
                self.preview_blocked = camera_in_call(self.center.read(cx).app().read(cx));
                self.start_preview();
            }
            Section::Notifications | Section::Mcp => self.preview.stop(cx),
        }
        cx.notify();
    }

    fn load_cameras(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.cameras_requested, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let cameras = cx.background_executor().spawn(async { list_cameras() }).await;
            this.update(cx, |view, cx| {
                view.cameras = camera_entries(&cameras);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_preview(&mut self) {
        if !self.preview_blocked {
            self.preview.start(&self.camera);
        }
    }

    fn sync_preview_block(&mut self, cx: &mut Context<Self>) {
        let blocked = camera_in_call(self.center.read(cx).app().read(cx));
        if blocked == self.preview_blocked {
            return;
        }
        self.preview_blocked = blocked;
        if blocked {
            self.preview.stop(cx);
        } else if self.section == Section::Devices {
            self.start_preview();
        }
        cx.notify();
    }

    fn select_camera(&mut self, camera: DeviceChoice, cx: &mut Context<Self>) {
        self.camera = camera.clone();
        let app = self.center.read(cx).app().clone();
        app.update(cx, |state, cx| state.select_call_camera(camera, cx));
        self.start_preview();
        cx.notify();
    }

    fn select_background(&mut self, pick: BackgroundPick, cx: &mut Context<Self>) {
        self.background = pick.clone();
        let app = self.center.read(cx).app().clone();
        app.update(cx, |state, cx| state.set_call_background(pick, cx));
        self.apply_background(cx);
        cx.notify();
    }

    fn apply_background(&self, cx: &App) {
        let state = self.center.read(cx).app().read(cx);
        if let Some(choice) = state.call_backgrounds.choice_for(&self.background) {
            self.preview.set_background(choice);
        }
    }
}

pub(super) fn switch(on: bool) -> Div {
    div()
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .rounded_full()
        .relative()
        .bg(if on {
            theme::accent()
        } else {
            theme::border_strong()
        })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 18. } else { 2. }))
                .size(px(16.))
                .rounded_full()
                .bg(theme::text()),
        )
}

fn button(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .h(px(32.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(theme::border_strong())
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .hover(|button| button.bg(theme::row_hover()))
        .child(label)
}

fn titled(title: &'static str, detail: impl Into<SharedString>) -> Div {
    titled_with(title, detail, theme::text_muted())
}

pub(super) fn titled_with(
    title: &'static str,
    detail: impl Into<SharedString>,
    detail_color: Hsla,
) -> Div {
    v_flex()
        .flex_1()
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(detail_color)
                .child(detail.into()),
        )
}

pub(super) fn control_row(
    title: &'static str,
    detail: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    h_flex()
        .w_full()
        .px(px(20.))
        .py(px(10.))
        .gap(px(12.))
        .items_center()
        .child(titled(title, detail))
        .child(control)
}

fn section_tab(section: Section, active: bool, view: Entity<SettingsView>) -> Stateful<Div> {
    div()
        .id(section.label())
        .h(px(32.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if active { theme::on_accent() } else { theme::text() })
        .when(active, |tab| tab.bg(theme::accent()))
        .when(!active, |tab| tab.hover(|tab| tab.bg(theme::row_hover())))
        .child(section.label())
        .on_click(move |_, _, cx| view.update(cx, |view, cx| view.select_section(section, cx)))
}

impl SettingsView {
    fn ringtone_row(&self, current: Ringtone) -> Div {
        let center = self.center.clone();
        control_row(
            "Ringtone",
            "Incoming calls",
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    Button::new("ringtone-select")
                        .outline()
                        .w(px(110.))
                        .label(current.label())
                        .icon(IconName::ChevronDown)
                        .dropdown_menu(move |menu, _, _| {
                            Ringtone::ALL.iter().fold(menu, |menu, ringtone| {
                                let (center, ringtone) = (center.clone(), *ringtone);
                                menu.item(PopupMenuItem::new(ringtone.label()).checked(ringtone == current).on_click(
                                    move |_, _, cx| {
                                        center.update(cx, |center, cx| {
                                            center.update_settings(cx, |settings| settings.ringtone = ringtone)
                                        });
                                        platform::play_preview(ringtone);
                                    },
                                ))
                            })
                        }),
                )
                .child(button("Play").on_click(move |_, _, _| platform::play_preview(current))),
        )
    }

    fn devices_section(&self, cx: &mut Context<Self>) -> Div {
        let view = cx.entity();
        let camera_label = self
            .cameras
            .iter()
            .find(|entry| entry.choice == self.camera)
            .map_or(DEFAULT_CAMERA_LABEL.to_owned(), |entry| entry.label.clone());
        let cameras = self.cameras.clone();
        let current_camera = self.camera.clone();
        let camera_view = view.clone();
        let backgrounds = preview_backgrounds(&self.center.read(cx).app().read(cx).call_backgrounds);
        let background_label = backgrounds
            .iter()
            .find(|(pick, _)| *pick == self.background)
            .map_or("None".to_owned(), |(_, label)| label.clone());
        let current_background = self.background.clone();
        let image = self.preview.image(Instant::now());
        let status = if self.preview_blocked {
            CAMERA_IN_USE.to_owned()
        } else {
            self.preview.failure().map_or_else(
                || {
                    self.preview
                        .fps()
                        .map_or("Starting camera...".to_owned(), |fps| format!("{fps:.0} fps"))
                },
                |reason| format!("Camera could not start: {reason}"),
            )
        };
        v_flex()
            .w_full()
            .child(control_row(
                "Camera",
                "Preview stays on this computer",
                Button::new("camera-select")
                    .outline()
                    .w(px(SELECT_WIDTH))
                    .label(camera_label)
                    .icon(IconName::ChevronDown)
                    .dropdown_menu(move |menu, _, _| {
                        cameras.iter().fold(menu, |menu, entry| {
                            let (view, choice) = (camera_view.clone(), entry.choice.clone());
                            menu.item(
                                PopupMenuItem::new(entry.label.clone())
                                    .checked(entry.choice == current_camera)
                                    .on_click(move |_, _, cx| {
                                        view.update(cx, |view, cx| view.select_camera(choice.clone(), cx))
                                    }),
                            )
                        })
                    }),
            ))
            .child(control_row(
                "Background",
                "Blur or a picture behind you",
                Button::new("background-select")
                    .outline()
                    .w(px(SELECT_WIDTH))
                    .label(background_label)
                    .icon(IconName::ChevronDown)
                    .dropdown_menu(move |menu, _, _| {
                        backgrounds.iter().fold(menu, |menu, (pick, label)| {
                            let (view, pick) = (view.clone(), pick.clone());
                            menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(pick == current_background)
                                    .on_click(move |_, _, cx| {
                                        view.update(cx, |view, cx| view.select_background(pick.clone(), cx))
                                    }),
                            )
                        })
                    }),
            ))
            .child(
                v_flex()
                    .w_full()
                    .px(px(20.))
                    .pt(px(8.))
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(PREVIEW_WIDTH))
                            .h(px(PREVIEW_HEIGHT))
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .rounded(px(PREVIEW_RADIUS))
                            .bg(theme::surface())
                            .children(image.map(|image| {
                                img(ImageSource::Render(image))
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .rounded(px(PREVIEW_RADIUS))
                            })),
                    )
                    .child(div().text_size(px(12.)).text_color(theme::text_muted()).child(status)),
            )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let tabs = h_flex()
            .w_full()
            .px(px(20.))
            .gap(px(8.))
            .children(Section::ALL.map(|section| section_tab(section, section == self.section, view.clone())));
        let body = match self.section {
            Section::Notifications => self.notifications_section(cx).into_any_element(),
            Section::Devices => self.devices_section(cx).into_any_element(),
            Section::Mcp => self.mcp.clone().into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(theme::background())
            .pt(px(12.))
            .child(tabs)
            .child(body)
    }
}

impl SettingsView {
    fn notifications_section(&self, cx: &App) -> Div {
        let mut current = self.center.read(cx).settings().clone();
        let corner = current.corner;
        let ringtone = current.ringtone;
        let rows = ROWS.iter().enumerate().map(|(index, row)| {
            let on = *(row.field)(&mut current);
            let center = self.center.clone();
            let field = row.field;
            h_flex()
                .id(("setting", index))
                .w_full()
                .px(px(20.))
                .py(px(10.))
                .gap(px(12.))
                .items_center()
                .cursor_pointer()
                .hover(|row| row.bg(theme::row_hover()))
                .on_click(move |_, _, cx| {
                    center.update(cx, |center, cx| {
                        center.update_settings(cx, |settings| {
                            let value = field(settings);
                            *value = !*value;
                        })
                    });
                })
                .child(
                    v_flex()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text())
                                .child(row.title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(row.detail),
                        ),
                )
                .child(switch(on))
        });
        let position_center = self.center.clone();
        let test_center = self.center.clone();
        v_flex()
            .w_full()
            .pt(px(8.))
            .children(rows)
            .child(self.ringtone_row(ringtone))
            .child(control_row(
                "Position",
                "Corner of the main screen",
                button(corner.label()).on_click(move |_, _, cx| {
                    position_center.update(cx, |center, cx| {
                        center.update_settings(cx, |settings| {
                            settings.corner = settings.corner.next()
                        })
                    });
                }),
            ))
            .child(
                h_flex()
                    .w_full()
                    .px(px(20.))
                    .pt(px(12.))
                    .child(button("Send test notification").on_click(move |_, _, cx| {
                        test_center.update(cx, |center, cx| center.send_test_notification(cx));
                    })),
            )
    }
}
