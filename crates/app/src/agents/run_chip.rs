use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::runs::{AgentRun, AgentRuns, RunState, format_elapsed};
use super::spinner::spinner;
use super::{AgentRunsHandle, open_session, retry_run};
use crate::theme;
use crate::views::widgets::icon;

const CHIP_HEIGHT: f32 = 28.;
const CHIP_MAX_WIDTH: f32 = 310.;
const BUTTON_HEIGHT: f32 = 22.;
const STATUS_ICON: f32 = 12.;

fn with_runs(cx: &mut App, change: impl FnOnce(&mut AgentRuns, &mut Context<AgentRuns>)) {
    cx.global::<AgentRunsHandle>().0.clone().update(cx, change);
}

fn chip_button(id: String, label: &'static str, primary: bool) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .h(px(BUTTON_HEIGHT))
        .px(px(8.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .text_color(theme::accent_text())
        .font_weight(FontWeight::SEMIBOLD)
        .when(primary, |button| button.bg(theme::surface_raised()))
        .hover(move |button| {
            button.bg(if primary {
                theme::popup_hover()
            } else {
                theme::surface_raised()
            })
        })
        .child(label)
}

fn dismiss_button(run_id: u64) -> Stateful<Div> {
    div()
        .id(("agent-dismiss", run_id as usize))
        .size(px(BUTTON_HEIGHT))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|button| button.bg(theme::surface_raised()))
        .child(icon(IconName::X, STATUS_ICON, theme::text_faint()))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            with_runs(cx, |runs, cx| runs.dismiss(run_id, cx));
        })
}

fn label(text: String, color: Hsla) -> Div {
    div().flex_none().text_color(color).child(text)
}

fn step_text(text: String) -> Div {
    div()
        .min_w(px(0.))
        .truncate()
        .text_color(theme::text_muted())
        .child(text)
}

fn elapsed_text(run: &AgentRun) -> Div {
    div()
        .flex_none()
        .text_color(theme::text_faint())
        .child(run.elapsed_text())
}

fn chip_row(run: &AgentRun, fill: Hsla) -> Stateful<Div> {
    let run_id = run.id;
    let step = run.current_step.clone();
    let session_id = run.session_id.clone();
    let row = h_flex()
        .id(("agent-chip", run_id as usize))
        .h(px(CHIP_HEIGHT))
        .max_w(px(CHIP_MAX_WIDTH))
        .pl(px(10.))
        .pr(px(4.))
        .gap(px(8.))
        .flex_none()
        .items_center()
        .rounded(px(8.))
        .bg(fill)
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(theme::text_soft())
        .tooltip(move |window, cx| Tooltip::new(step.clone()).build(window, cx))
        .on_click(move |_, _, cx| with_runs(cx, |runs, cx| runs.toggle_expanded(run_id, cx)));
    let action_label = run.action_label.clone();
    match &run.state {
        RunState::Running => row
            .child(spinner(STATUS_ICON))
            .child(label(action_label, theme::text_soft()))
            .child(step_text(run.current_step.clone()))
            .child(elapsed_text(run))
            .child(
                div()
                    .size(px(BUTTON_HEIGHT))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .child(icon(
                        if run.expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        },
                        STATUS_ICON,
                        theme::text_faint(),
                    )),
            ),
        RunState::Done => row
            .child(icon(IconName::Check, STATUS_ICON, theme::accent_text()))
            .child(label(action_label, theme::text_soft()))
            .child(step_text(run.current_step.clone()))
            .child(elapsed_text(run))
            .when(!session_id.is_empty(), |row| {
                row.child(
                    chip_button(format!("agent-open-{run_id}"), "Open session", true).on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            open_session(&session_id, cx);
                        },
                    ),
                )
            })
            .child(dismiss_button(run_id)),
        RunState::Failed { .. } => {
            row.child(icon(IconName::X, STATUS_ICON, theme::red_soft()))
                .child(label("Failed".to_owned(), theme::red_tint()))
                .child(step_text(run.current_step.clone()))
                .child(
                    chip_button(format!("agent-log-{run_id}"), "Log", false).on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            with_runs(cx, |runs, cx| runs.toggle_expanded(run_id, cx));
                        },
                    ),
                )
                .child(
                    chip_button(format!("agent-retry-{run_id}"), "Retry", false).on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            retry_run(run_id, cx);
                        },
                    ),
                )
                .when(!session_id.is_empty(), |row| {
                    row.child(
                        chip_button(format!("agent-open-{run_id}"), "Open session", false)
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                open_session(&session_id, cx);
                            }),
                    )
                })
                .child(dismiss_button(run_id))
        }
        RunState::Stopped => row
            .child(icon(IconName::Square, STATUS_ICON, theme::text_faint()))
            .child(label(action_label, theme::text_soft()))
            .child(step_text("Stopped".to_owned()))
            .child(elapsed_text(run))
            .child(dismiss_button(run_id)),
    }
}

fn step_row(elapsed: String, text: String, current: bool) -> Div {
    h_flex()
        .gap(px(8.))
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(if current {
            theme::text_strong()
        } else {
            theme::text_muted()
        })
        .child(
            div()
                .flex_none()
                .text_color(theme::text_faint())
                .child(elapsed),
        )
        .child(div().min_w(px(0.)).truncate().child(text))
}

fn stop_row(run_id: u64) -> Div {
    h_flex()
        .mt(px(2.))
        .items_center()
        .justify_between()
        .child(
            chip_button(
                format!("agent-stop-open-{run_id}"),
                "Stop and open session",
                false,
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                with_runs(cx, |runs, cx| runs.stop_and_open_session(run_id, cx));
            }),
        )
        .child(
            chip_button(format!("agent-stop-{run_id}"), "Stop", false).on_click(move |_, _, cx| {
                cx.stop_propagation();
                with_runs(cx, |runs, cx| runs.stop(run_id, cx));
            }),
        )
}

fn detail_panel(run: &AgentRun, fill: Hsla) -> Div {
    let panel = v_flex()
        .w(px(CHIP_MAX_WIDTH))
        .py(px(8.))
        .px(px(10.))
        .gap(px(6.))
        .rounded(px(8.))
        .bg(fill)
        .text_size(px(12.))
        .text_color(theme::text_muted());
    if matches!(run.state, RunState::Failed { .. }) {
        let lines = run.log_lines();
        if lines.is_empty() {
            return panel.child("No output");
        }
        return panel.children(lines.iter().map(|line| {
            div()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(line.clone())
        }));
    }
    let steps = run.visible_steps();
    if steps.is_empty() {
        return panel
            .child("No steps yet")
            .when(run.is_running(), |panel| panel.child(stop_row(run.id)));
    }
    let last = steps.len() - 1;
    panel
        .children(steps.iter().enumerate().map(|(index, (elapsed, text))| {
            step_row(format_elapsed(*elapsed), text.clone(), index == last)
        }))
        .when(run.is_running(), |panel| panel.child(stop_row(run.id)))
}

pub fn run_chip(run: &AgentRun, fill: Hsla) -> AnyElement {
    v_flex()
        .gap(px(4.))
        .items_start()
        .child(chip_row(run, fill))
        .when(run.expanded, |column| column.child(detail_panel(run, fill)))
        .into_any_element()
}
