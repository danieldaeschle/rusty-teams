use app_ui_kit::settings_controls::PAGE_PADDING;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::devices::DevicesView;
use super::mcp::McpView;
use super::nav::{NavLayout, label, render_nav};
use super::rows::ROW_INSET;
use super::{about, calls, chats, general, notifications};
use crate::agents::AgentsSettings;
use crate::app_state::AppState;
use crate::notify::NotificationCenter;
use crate::settings_section::SettingsSection;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::widgets::icon;

const BACKDROP_OPACITY: f32 = 0.55;
pub(super) const PANEL_RADIUS: f32 = 10.;
const CONTENT_MAX_WIDTH: f32 = 744.;
const CLOSE_SIZE: f32 = 32.;
const COMPACT_BELOW_WIDTH: f32 = 1100.;
const COMPACT_BELOW_HEIGHT: f32 = 720.;

pub enum SettingsDialogEvent {
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogMetrics {
    pub inset: f32,
    pub nav_width: f32,
    pub padding: f32,
}

pub fn metrics(window_width: f32, window_height: f32) -> DialogMetrics {
    if window_width < COMPACT_BELOW_WIDTH || window_height < COMPACT_BELOW_HEIGHT {
        DialogMetrics {
            inset: 24.,
            nav_width: 216.,
            padding: 18.,
        }
    } else {
        DialogMetrics {
            inset: 48.,
            nav_width: 264.,
            padding: 24.,
        }
    }
}

pub fn available(section: SettingsSection, agents_available: bool) -> SettingsSection {
    if section == SettingsSection::Agents && !agents_available {
        SettingsSection::General
    } else {
        section
    }
}

pub struct SettingsDialog {
    app: Entity<AppState>,
    center: Entity<NotificationCenter>,
    section: SettingsSection,
    devices: Option<Entity<DevicesView>>,
    mcp: Option<Entity<McpView>>,
    agents: Option<Entity<AgentsSettings>>,
    focus_handle: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<SettingsDialogEvent> for SettingsDialog {}

impl SettingsDialog {
    pub fn new(
        app: Entity<AppState>,
        center: Entity<NotificationCenter>,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = [
            cx.observe(&center, |_, _, cx| cx.notify()),
            cx.observe(&app, |_, _, cx| cx.notify()),
        ];
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let mut dialog = SettingsDialog {
            app,
            center,
            section,
            devices: None,
            mcp: None,
            agents: None,
            focus_handle,
            _subscriptions: subscriptions,
        };
        dialog.show(section, window, cx);
        dialog
    }

    pub fn show(&mut self, section: SettingsSection, window: &mut Window, cx: &mut Context<Self>) {
        let section = available(section, crate::agents::runs_handle(cx).is_some());
        self.section = section;
        if section == SettingsSection::Devices {
            if self.devices.is_none() {
                let app = self.app.clone();
                self.devices = Some(cx.new(|cx| DevicesView::new(app, cx)));
            }
        } else {
            self.devices = None;
        }
        if section == SettingsSection::Mcp && self.mcp.is_none() {
            let app = self.app.clone();
            self.mcp = Some(cx.new(|cx| McpView::new(app, window, cx)));
        }
        if section == SettingsSection::Agents && self.agents.is_none() {
            self.agents = crate::agents::runs_handle(cx)
                .map(|runs| cx.new(|cx| AgentsSettings::new(runs, cx)));
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(SettingsDialogEvent::Close);
            cx.stop_propagation();
        }
    }

    fn content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            SettingsSection::General => {
                general::render(&self.center, &self.app, cx).into_any_element()
            }
            SettingsSection::Notifications => {
                notifications::render(&self.center, cx).into_any_element()
            }
            SettingsSection::Chats => chats::render(&self.app, cx).into_any_element(),
            SettingsSection::Calls => calls::render(&self.center, cx).into_any_element(),
            SettingsSection::Devices => self
                .devices
                .clone()
                .map_or_else(|| div().into_any_element(), |view| view.into_any_element()),
            SettingsSection::Agents => self
                .agents
                .clone()
                .map_or_else(|| div().into_any_element(), |view| view.into_any_element()),
            SettingsSection::Mcp => self
                .mcp
                .clone()
                .map_or_else(|| div().into_any_element(), |view| view.into_any_element()),
            SettingsSection::About => about::render().into_any_element(),
        }
    }

    fn header(&self, metrics: DialogMetrics, cx: &mut Context<Self>) -> Div {
        h_flex()
            .debug_selector(|| "settings/main/header".into())
            .w_full()
            .flex_none()
            .items_center()
            .pt(px(metrics.padding))
            .pr(px(metrics.padding))
            .pb(px(metrics.padding / 2.))
            .pl(px(metrics.padding + ROW_INSET))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(20.))
                    .line_height(px(28.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(label(self.section)),
            )
            .child(
                div()
                    .id("settings-close")
                    .size(px(CLOSE_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::popup_hover()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsDialogEvent::Close)))
                    .child(icon(IconName::X, 16., theme::text_muted())),
            )
    }
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let metrics = metrics(f32::from(viewport.width), f32::from(viewport.height));
        let dialog = cx.entity();
        let nav = render_nav(
            self.section,
            crate::agents::runs_handle(cx).is_some(),
            NavLayout {
                width: metrics.nav_width,
                padding_y: metrics.padding,
            },
            move |section, window, cx| {
                dialog.update(cx, |dialog, cx| dialog.show(section, window, cx))
            },
        );
        let body_inset = if self.section == SettingsSection::Agents {
            metrics.padding + ROW_INSET - PAGE_PADDING
        } else {
            metrics.padding
        };
        let body = div()
            .id(SharedString::from(format!(
                "settings-body-{}",
                label(self.section)
            )))
            .debug_selector(|| "settings/main/body".into())
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(body_inset.max(0.)))
            .pb(px(metrics.padding))
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH))
                    .child(self.content(cx)),
            );
        div()
            .id("settings-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                div()
                    .id("settings-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsDialogEvent::Close))),
            )
            .child(
                h_flex()
                    .id("settings-dialog")
                    .debug_selector(|| "settings".into())
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .absolute()
                    .top(px(metrics.inset))
                    .left(px(metrics.inset))
                    .right(px(metrics.inset))
                    .bottom(px(metrics.inset))
                    .overflow_hidden()
                    .rounded(px(PANEL_RADIUS))
                    .dialog_surface()
                    .text_color(theme::text())
                    .occlude()
                    .child(nav)
                    .child(
                        v_flex()
                            .debug_selector(|| "settings/main".into())
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.header(metrics, cx))
                            .child(body),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{available, metrics};
    use crate::settings_section::SettingsSection;

    #[test]
    fn small_windows_get_the_compact_inset() {
        assert_eq!(metrics(1280., 800.).inset, 48.);
        assert_eq!(metrics(1920., 1080.).nav_width, 264.);
        assert_eq!(metrics(960., 640.).inset, 24.);
        assert_eq!(metrics(1280., 640.).nav_width, 216.);
    }

    #[test]
    fn agents_falls_back_to_general_without_agent_runs() {
        assert_eq!(
            available(SettingsSection::Agents, false),
            SettingsSection::General
        );
        assert_eq!(
            available(SettingsSection::Agents, true),
            SettingsSection::Agents
        );
        assert_eq!(available(SettingsSection::Mcp, false), SettingsSection::Mcp);
    }
}
