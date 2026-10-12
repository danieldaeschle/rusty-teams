use gpui_kit::assets::IconName;
use gpui_kit::base::PopoverState;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::runs::{AgentRun, AgentRuns, RunState};
use super::spinner::spinner;
use crate::app_state::AppState;
use crate::notify::selection_for;
use crate::theme;
use crate::views::widgets::icon;

const POPOVER_WIDTH: f32 = 300.;
const DONE_LIMIT: usize = 10;
const HISTORY_WINDOW: usize = 60;
const STATUS_ICON: f32 = 12.;

pub struct RunsPopover {
    app: Entity<AppState>,
    runs: Entity<AgentRuns>,
    popover: Option<WeakEntity<PopoverState>>,
    _subscription: Subscription,
}

impl RunsPopover {
    pub fn new(app: Entity<AppState>, runs: Entity<AgentRuns>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&runs, |_, _, cx| cx.notify());
        RunsPopover {
            app,
            runs,
            popover: None,
            _subscription: subscription,
        }
    }

    pub fn set_popover(&mut self, popover: WeakEntity<PopoverState>) {
        self.popover = Some(popover);
    }

    fn show(&mut self, run: &AgentRun, window: &mut Window, cx: &mut Context<Self>) {
        let selection = selection_for(&self.app.read(cx).sidebar, &run.conversation_id);
        if let Some(selection) = selection {
            let message_id = run.message_id.clone();
            self.app.update(cx, |state, cx| {
                state.jump_to_message(selection, message_id, cx)
            });
        }
        if let Some(popover) = &self.popover {
            popover
                .update(cx, |state, cx| state.dismiss(window, cx))
                .ok();
        }
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
            .w_full()
            .px(px(10.))
            .py(px(6.))
            .gap(px(10.))
            .items_start()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::popup_hover()))
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
            .on_click(cx.listener(move |this, _, window, cx| this.show(&shown, window, cx)))
    }
}

fn section_header(title: &'static str) -> Div {
    div()
        .px(px(10.))
        .pt(px(6.))
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

impl Render for RunsPopover {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let runs = self.runs.read(cx);
        let (running, finished) = split_runs(runs.recent(HISTORY_WINDOW));
        let (running, finished): (Vec<AgentRun>, Vec<AgentRun>) = (
            running.into_iter().cloned().collect(),
            finished.into_iter().cloned().collect(),
        );
        let empty = running.is_empty() && finished.is_empty();
        v_flex()
            .w(px(POPOVER_WIDTH))
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
                        .px(px(10.))
                        .py(px(8.))
                        .text_size(px(12.))
                        .text_color(theme::text_faint())
                        .child("No agent runs yet"),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::split_runs;
    use crate::agents::runs::RunState;
    use crate::agents::runs::tests::finished_run;

    #[test]
    fn running_runs_go_first_and_done_is_capped_at_ten() {
        let mut runs: Vec<_> = (0..14).map(finished_run).collect();
        runs[3].state = RunState::Running;
        let newest_first: Vec<_> = runs.iter().rev().collect();
        let (running, finished) = split_runs(newest_first);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, 3);
        assert_eq!(finished.len(), 10);
        assert_eq!(finished[0].id, 13);
    }
}
