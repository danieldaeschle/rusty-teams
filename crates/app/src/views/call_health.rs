use calling::NetworkLevel;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::call_organizer::pill;
use super::call_view::{BANNER_HEIGHT, MENU_WIDTH};
use super::widgets::symbol;
use crate::app_state::AppState;
use crate::call::{CallModel, HealthBanner, level_text, media_text, network_rows, transport_text};
use crate::theme;

const BAR_WIDTH: f32 = 3.;
const BAR_HEIGHTS: [f32; 3] = [6., 10., 14.];
const BAR_GAP: f32 = 2.;
const INACTIVE_BAR: f32 = 0.3;
const INDICATOR_HEIGHT: f32 = 28.;
const POPOVER_PADDING: f32 = 12.;
const BANNER_TEXT: f32 = 13.;
const NETWORK_TEXT: &str = "Your network is unstable. Turning off incoming video may help.";
const CPU_TEXT: &str = "Your computer is overloaded. Turning off video effects may help.";

fn dark_text() -> Hsla {
    hsla(0., 0., 0.08, 1.)
}

fn level_color(level: NetworkLevel) -> Hsla {
    match level {
        NetworkLevel::Good => theme::text_muted(),
        NetworkLevel::Poor => theme::amber(),
        NetworkLevel::Bad => theme::red(),
    }
}

fn lit_bars(level: NetworkLevel) -> usize {
    match level {
        NetworkLevel::Good => BAR_HEIGHTS.len(),
        NetworkLevel::Poor => 2,
        NetworkLevel::Bad => 1,
    }
}

fn signal_bars(level: NetworkLevel) -> Div {
    let lit = lit_bars(level);
    h_flex()
        .h(px(BAR_HEIGHTS[2]))
        .gap(px(BAR_GAP))
        .items_end()
        .children(BAR_HEIGHTS.iter().enumerate().map(|(index, height)| {
            let color = level_color(level);
            div()
                .w(px(BAR_WIDTH))
                .h(px(*height))
                .rounded(px(1.))
                .bg(if index < lit {
                    color
                } else {
                    color.opacity(INACTIVE_BAR)
                })
        }))
}

fn section_title(title: &'static str) -> Div {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(title)
}

fn row(label: &'static str, value: String) -> Div {
    h_flex()
        .w_full()
        .justify_between()
        .text_size(px(13.))
        .child(div().text_color(theme::text_muted()).child(label))
        .child(value)
}

fn health_popover(model: &CallModel) -> Div {
    let health = model.health.latest.clone();
    let level = model.health.level();
    let network = match &health {
        Some(health) => v_flex()
            .gap(px(4.))
            .child(row("Quality", level_text(level).to_owned()))
            .children(
                network_rows(&health.sample)
                    .into_iter()
                    .map(|(label, value)| row(label, value)),
            ),
        None => v_flex().child(row("Quality", "Measuring...".to_owned())),
    };
    let transport = health.iter().flat_map(|health| {
        [Some(transport_text(health)), media_text(health, &model.peer_name)]
            .into_iter()
            .flatten()
    });
    v_flex()
        .debug_selector(|| "shell/islands/main/call-health-popover".into())
        .w(px(MENU_WIDTH))
        .p(px(POPOVER_PADDING))
        .gap(px(12.))
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Call health"),
        )
        .child(v_flex().gap(px(6.)).child(section_title("Network")).child(network))
        .child(
            v_flex()
                .gap(px(6.))
                .child(section_title("Encryption"))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Secured by Teams encryption"),
                )
                .children(transport.map(|line| {
                    div()
                        .text_size(px(13.))
                        .text_color(theme::text_muted())
                        .child(line)
                })),
        )
}

pub fn health_indicator(model: &CallModel) -> Option<AnyElement> {
    model.health.latest.as_ref()?;
    let level = model.health.level();
    let model = model.clone();
    Some(
        Popover::new("call-health-popover")
            .anchor(Anchor::TopLeft)
            .offset(px(8.))
            .trigger(
                Button::new("call-health")
                    .ghost()
                    .p_0()
                    .h(px(INDICATOR_HEIGHT))
                    .px(px(6.))
                    .child(
                        h_flex()
                            .debug_selector(|| "shell/islands/main/call-health".into())
                            .gap(px(8.))
                            .items_center()
                            .child(signal_bars(level))
                            .child(symbol("shield", 18., theme::text_muted())),
                    ),
            )
            .content(move |_, _, _| health_popover(&model))
            .into_any_element(),
    )
}

pub fn health_banner(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    let banner = model.health_banner()?;
    let (text, action_label) = match banner {
        HealthBanner::Network => (NETWORK_TEXT, "Turn off incoming video"),
        HealthBanner::Cpu => (CPU_TEXT, "Turn off effects"),
    };
    let (action_app, close_app) = (app.clone(), app.clone());
    Some(
        h_flex()
            .id("call-health-banner")
            .debug_selector(|| "shell/islands/main/call-health-banner".into())
            .w_full()
            .h(px(BANNER_HEIGHT))
            .flex_none()
            .px(px(20.))
            .gap(px(12.))
            .items_center()
            .justify_center()
            .bg(theme::amber())
            .text_color(dark_text())
            .text_size(px(BANNER_TEXT))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text)
            .child(
                pill("call-health-action", action_label, true).on_click(move |_, _, cx| {
                    action_app.update(cx, |state, cx| match banner {
                        HealthBanner::Network => state.set_call_incoming_video(false, cx),
                        HealthBanner::Cpu => state.turn_off_call_effects(cx),
                    })
                }),
            )
            .child(
                div()
                    .id("call-health-close")
                    .size(px(24.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|button| button.bg(dark_text().opacity(0.12)))
                    .child(symbol("close", 16., dark_text()))
                    .on_click(move |_, _, cx| {
                        close_app
                            .update(cx, |state, cx| state.close_call_health_banner(banner, cx))
                    }),
            )
            .into_any_element(),
    )
}
