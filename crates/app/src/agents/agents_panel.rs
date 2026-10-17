use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::runs::{AgentRun, AgentRuns, RunState};
use super::spinner::spinner;
use crate::theme;
use crate::views::scroll_edge::with_top_edge;
use crate::views::widgets::icon;

const AGENTS_SCROLL_DIVIDER: &str = "shell/islands/sidebar/agents/divider-scroll";
const DONE_LIMIT: usize = 50;
const HISTORY_WINDOW: usize = 60;
const EMPTY_TEXT: &str = "No agent runs yet. Hover a message and click the robot button.";
const STATUS_ICON: f32 = 12.;

pub enum AgentsPanelEvent {
    Open {
        conversation_id: String,
        message_id: String,
    },
}

pub struct AgentsPanel {
    runs: Entity<AgentRuns>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl EventEmitter<AgentsPanelEvent> for AgentsPanel {}

impl AgentsPanel {
    pub fn new(runs: Entity<AgentRuns>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&runs, |_, _, cx| cx.notify());
        AgentsPanel {
            runs,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn show(&mut self, run: &AgentRun, cx: &mut Context<Self>) {
        cx.emit(AgentsPanelEvent::Open {
            conversation_id: run.conversation_id.clone(),
            message_id: run.message_id.clone(),
        });
    }

    fn row(&self, run: &AgentRun, cx: &mut Context<Self>) -> Stateful<Div> {
        let shown = run.clone();
        let status = match &run.state {
            RunState::Running => div().mt(px(2.)).child(spinner(STATUS_ICON)),
            RunState::Done => {
                div()
                    .mt(px(2.))
                    .child(icon(IconName::Check, STATUS_ICON, theme::accent_text()))
            }
            RunState::Failed { .. } => {
                div()
                    .mt(px(2.))
                    .child(icon(IconName::X, STATUS_ICON, theme::red_soft()))
            }
            RunState::Stopped => {
                div()
                    .mt(px(2.))
                    .child(icon(IconName::Square, STATUS_ICON, theme::text_faint()))
            }
        };
        let trailing = if run.is_running() {
            div()
                .text_size(px(11.))
                .text_color(theme::text_faint())
                .child(run.elapsed_text())
        } else {
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::accent_text())
                .child("Show")
        };
        h_flex()
            .id(("agent-run-row", run.id as usize))
            .mx(px(6.))
            .px(px(10.))
            .py(px(8.))
            .gap(px(12.))
            .items_start()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(status.flex_none())
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(theme::text())
                            .child(format!("{} - {}", run.action_label, run.chat_title)),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child(run.current_step.clone()),
                    ),
            )
            .child(trailing.flex_none())
            .on_click(cx.listener(move |this, _, _, cx| this.show(&shown, cx)))
    }
}

fn section_header(title: &'static str) -> Div {
    div()
        .px(px(16.))
        .pt(px(8.))
        .pb(px(4.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_faint())
        .child(title)
}

pub fn split_runs(runs: Vec<&AgentRun>) -> (Vec<&AgentRun>, Vec<&AgentRun>) {
    let (running, finished): (Vec<&AgentRun>, Vec<&AgentRun>) =
        runs.into_iter().partition(|run| run.is_running());
    (running, finished.into_iter().take(DONE_LIMIT).collect())
}

impl Render for AgentsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let runs = self.runs.read(cx);
        let (running, finished) = split_runs(runs.recent(HISTORY_WINDOW));
        let (running, finished): (Vec<AgentRun>, Vec<AgentRun>) = (
            running.into_iter().cloned().collect(),
            finished.into_iter().cloned().collect(),
        );
        let empty = running.is_empty() && finished.is_empty();
        let list = v_flex()
            .id("agents-list")
            .debug_selector(|| "shell/islands/sidebar/agents".into())
            .flex_1()
            .min_h_0()
            .pb(px(12.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .when(!running.is_empty(), |list| {
                list.child(section_header("Running"))
                    .children(running.iter().map(|run| self.row(run, cx)))
            })
            .when(!finished.is_empty(), |list| {
                list.child(section_header("Done"))
                    .children(finished.iter().map(|run| self.row(run, cx)))
            })
            .when(empty, |list| {
                list.child(
                    div()
                        .py(px(32.))
                        .px(px(24.))
                        .flex()
                        .justify_center()
                        .text_center()
                        .text_size(px(13.))
                        .text_color(theme::text_muted())
                        .child(EMPTY_TEXT),
                )
            });
        with_top_edge(list, &self.scroll, AGENTS_SCROLL_DIVIDER)
    }
}

#[cfg(test)]
mod tests {
    use super::split_runs;
    use crate::agents::runs::RunState;
    use crate::agents::fixtures::finished_run;

    #[test]
    fn running_runs_go_first_and_done_is_capped_at_fifty() {
        let mut runs: Vec<_> = (0..60).map(finished_run).collect();
        runs[3].state = RunState::Running;
        let newest_first: Vec<_> = runs.iter().rev().collect();
        let (running, finished) = split_runs(newest_first);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, 3);
        assert_eq!(finished.len(), 50);
        assert_eq!(finished[0].id, 59);
    }
}
