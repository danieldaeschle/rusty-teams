use std::rc::Rc;

use chatsvc::{
    Attendance, AttendanceRecord, Occurrence, RecordingDetails, TranscriptFormat, TranscriptTurn,
    days_until_expiry,
};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, Offset as _, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::shared_tab::{SharedRun, download_label, item_row};
use super::widgets::icon;
use crate::channel_files::SharedItem;
use crate::downloads::DownloadState;
use crate::recap::{
    ATTENDANCE_FAILED, ATTENDANCE_OFF, EMPTY_RECAP, Load, NO_TRANSCRIPT, clock_label,
    expiry_is_urgent, expiry_label, highlight_segments, matching_turns, occurrence_label,
    shows_recording, span_label, time_label,
};
use crate::theme;

const SIDE_PADDING: f32 = 20.;
const SECTION_GAP: f32 = 24.;
const CARD_WIDTH: f32 = 360.;
const CARD_HEIGHT: f32 = 202.;
const CARD_RADIUS: f32 = 10.;
const PLAY_SIZE: f32 = 52.;
const PLAY_GLYPH: f32 = 24.;
const SEARCH_WIDTH: f32 = 260.;
const TIME_WIDTH: f32 = 48.;
const ATTENDANCE_COLUMN: f32 = 90.;
const NAME_COLUMN: f32 = 220.;

pub const RECAP_REGION: &str = "shell/islands/main/recap";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecapDownload {
    Recording,
    Transcript(TranscriptFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecapCommand {
    Occurrence(usize),
    Play,
    Download(RecapDownload),
    AskDelete,
    Retry,
}

pub type RecapRun = Rc<dyn Fn(RecapCommand, &mut Window, &mut App)>;

pub struct RecapView<'a> {
    pub occurrences: &'a [Occurrence],
    pub selected: usize,
    pub details: &'a Load<RecordingDetails>,
    pub transcript: &'a Load<Vec<TranscriptTurn>>,
    pub attendance: &'a Load<Attendance>,
    pub search: &'a Entity<InputState>,
    pub query: &'a str,
    pub can_delete: bool,
    pub recording_state: Option<&'a DownloadState>,
    pub vtt_state: Option<&'a DownloadState>,
    pub docx_state: Option<&'a DownloadState>,
    pub files: &'a [SharedItem],
    pub file_labels: Vec<Option<String>>,
    pub file_run: SharedRun,
    pub now: DateTime<Utc>,
}

fn section(title: &'static str) -> Div {
    v_flex()
        .w_full()
        .flex_none()
        .gap(px(10.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(title),
        )
}

fn muted(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.5))
        .text_color(theme::text_muted())
        .child(text.into())
}

fn action(
    id: &'static str,
    label: String,
    glyph: IconName,
    command: RecapCommand,
    run: &RecapRun,
) -> Button {
    let run = run.clone();
    Button::new(id)
        .ghost()
        .compact()
        .icon(glyph)
        .label(label)
        .on_click(move |_, window, cx| run(command, window, cx))
}

fn download_action(
    id: &'static str,
    label: &'static str,
    download: RecapDownload,
    state: Option<&DownloadState>,
    run: &RecapRun,
) -> Button {
    let text = download_label(state).map_or_else(
        || label.to_owned(),
        |progress| format!("{label} - {progress}"),
    );
    action(
        id,
        text,
        IconName::Download,
        RecapCommand::Download(download),
        run,
    )
}

fn picker(view: &RecapView, offset: FixedOffset, today: NaiveDate, run: &RecapRun) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .gap(px(6.))
        .flex_wrap()
        .children(view.occurrences.iter().enumerate().map(|(index, occurrence)| {
            let run = run.clone();
            let active = index == view.selected;
            div()
                .id(ElementId::NamedInteger("recap-occurrence".into(), index as u64))
                .px(px(12.))
                .py(px(4.))
                .rounded_full()
                .cursor_pointer()
                .text_size(px(12.))
                .bg(if active {
                    theme::accent()
                } else {
                    theme::surface_raised()
                })
                .text_color(if active {
                    theme::on_accent()
                } else {
                    theme::text_soft()
                })
                .on_click(move |_, window, cx| {
                    run(RecapCommand::Occurrence(index), window, cx)
                })
                .child(occurrence_label(occurrence, today, offset))
        }))
}

fn recording_card(details: &RecordingDetails) -> Div {
    let labels = h_flex()
        .absolute()
        .left(px(12.))
        .right(px(12.))
        .bottom(px(10.))
        .justify_between()
        .text_size(px(12.))
        .text_color(gpui_kit::white())
        .child(div().child(
            details
                .duration_seconds
                .map(clock_label)
                .unwrap_or_default(),
        ))
        .when(details.audio_only, |row| row.child(div().child("Audio only")));
    div()
        .relative()
        .w(px(CARD_WIDTH))
        .h(px(CARD_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(CARD_RADIUS))
        .bg(hsla(0., 0., 0.07, 1.))
        .child(
            div()
                .size(px(PLAY_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(gpui_kit::white().opacity(0.18))
                .child(icon(IconName::Play, PLAY_GLYPH, gpui_kit::white())),
        )
        .child(labels)
}

fn recording_section(view: &RecapView, run: &RecapRun) -> Div {
    let body = match view.details {
        Load::Loading => muted("Loading recording ..").into_any_element(),
        Load::Failed(reason) => failure(format!("Recording not available: {reason}"), run),
        Load::Ready(details) => {
            let days = details
                .expires_at
                .map(|expires_at| days_until_expiry(expires_at, view.now));
            let expiry = days.map(|days| {
                div()
                    .text_size(px(12.))
                    .text_color(if expiry_is_urgent(days) {
                        theme::amber()
                    } else {
                        theme::text_muted()
                    })
                    .child(expiry_label(days))
            });
            h_flex()
                .gap(px(16.))
                .items_start()
                .child(recording_card(details))
                .child(
                    v_flex()
                        .gap(px(8.))
                        .items_start()
                        .children(expiry)
                        .child(action(
                            "recap-play",
                            "Play".to_owned(),
                            IconName::Play,
                            RecapCommand::Play,
                            run,
                        ))
                        .child(download_action(
                            "recap-download",
                            "Download",
                            RecapDownload::Recording,
                            view.recording_state,
                            run,
                        ))
                        .when(view.can_delete, |column| {
                            column.child(
                                action(
                                    "recap-delete",
                                    "Delete".to_owned(),
                                    IconName::Trash,
                                    RecapCommand::AskDelete,
                                    run,
                                )
                                .danger(),
                            )
                        }),
                )
                .into_any_element()
        }
    };
    section("Recording").child(body)
}

fn failure(reason: String, run: &RecapRun) -> AnyElement {
    let run = run.clone();
    h_flex()
        .gap(px(8.))
        .items_center()
        .child(muted(reason))
        .child(
            Button::new("recap-retry")
                .ghost()
                .compact()
                .label("Retry")
                .on_click(move |_, window, cx| run(RecapCommand::Retry, window, cx)),
        )
        .into_any_element()
}

fn highlighted(text: &str, query: &str) -> StyledText {
    let mut start = 0;
    let mut ranges = Vec::new();
    for (segment, hit) in highlight_segments(text, query) {
        let end = start + segment.len();
        if hit {
            ranges.push((
                start..end,
                HighlightStyle {
                    background_color: Some(theme::amber().opacity(0.35)),
                    ..HighlightStyle::default()
                },
            ));
        }
        start = end;
    }
    StyledText::new(text.to_owned()).with_highlights(ranges)
}

fn turn_row(turn: &TranscriptTurn, query: &str) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .gap(px(12.))
        .items_start()
        .text_size(px(13.))
        .child(
            div()
                .w(px(TIME_WIDTH))
                .flex_none()
                .text_size(px(12.))
                .text_color(theme::text_faint())
                .child(clock_label(turn.start_seconds as u64)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text())
                        .child(highlighted(&turn.speaker, query)),
                )
                .child(
                    div()
                        .text_color(theme::text_soft())
                        .child(highlighted(&turn.text, query)),
                ),
        )
}

fn transcript_section(view: &RecapView, run: &RecapRun) -> Div {
    let downloads = h_flex()
        .gap(px(4.))
        .child(download_action(
            "recap-vtt",
            "Download .vtt",
            RecapDownload::Transcript(TranscriptFormat::Vtt),
            view.vtt_state,
            run,
        ))
        .child(download_action(
            "recap-docx",
            "Download .docx",
            RecapDownload::Transcript(TranscriptFormat::Docx),
            view.docx_state,
            run,
        ));
    let body = match view.transcript {
        Load::Loading => muted("Loading transcript ..").into_any_element(),
        Load::Failed(reason) => failure(format!("Transcript not available: {reason}"), run),
        Load::Ready(turns) if turns.is_empty() => muted(NO_TRANSCRIPT).into_any_element(),
        Load::Ready(turns) => {
            let matches = matching_turns(turns, view.query);
            let list = if matches.is_empty() {
                muted("No lines match.").into_any_element()
            } else {
                v_flex()
                    .w_full()
                    .gap(px(10.))
                    .children(
                        matches
                            .into_iter()
                            .map(|index| turn_row(&turns[index], view.query)),
                    )
                    .into_any_element()
            };
            v_flex()
                .w_full()
                .gap(px(12.))
                .child(
                    h_flex()
                        .w(px(SEARCH_WIDTH))
                        .h(px(30.))
                        .px(px(8.))
                        .gap(px(6.))
                        .items_center()
                        .rounded(px(8.))
                        .bg(theme::surface_raised())
                        .child(icon(IconName::Search, 14., theme::text_muted()))
                        .child(Input::new(view.search).appearance(false).bordered(false)),
                )
                .child(list)
                .into_any_element()
        }
    };
    section("Transcript")
        .child(downloads)
        .child(body)
}

fn attendance_row(record: &AttendanceRecord, offset: FixedOffset) -> Div {
    let cell = |text: String, width: f32| {
        div()
            .w(px(width))
            .flex_none()
            .truncate()
            .text_color(theme::text_muted())
            .child(text)
    };
    h_flex()
        .w_full()
        .flex_none()
        .gap(px(12.))
        .text_size(px(12.5))
        .child(
            div()
                .w(px(NAME_COLUMN))
                .flex_none()
                .truncate()
                .text_color(theme::text())
                .child(record.name.clone()),
        )
        .child(cell(time_label(record.joined_at, offset), ATTENDANCE_COLUMN))
        .child(cell(time_label(record.left_at, offset), ATTENDANCE_COLUMN))
        .child(cell(span_label(record.duration_seconds), ATTENDANCE_COLUMN))
}

fn attendance_header() -> Div {
    let cell = |text: &'static str, width: f32| div().w(px(width)).flex_none().child(text);
    h_flex()
        .w_full()
        .flex_none()
        .gap(px(12.))
        .text_size(px(11.5))
        .text_color(theme::text_faint())
        .child(cell("Name", NAME_COLUMN))
        .child(cell("Joined", ATTENDANCE_COLUMN))
        .child(cell("Left", ATTENDANCE_COLUMN))
        .child(cell("Duration", ATTENDANCE_COLUMN))
}

fn attendance_section(view: &RecapView, offset: FixedOffset) -> Div {
    let body = match view.attendance {
        Load::Loading => muted("Loading attendance ..").into_any_element(),
        Load::Failed(_) => muted(ATTENDANCE_FAILED).into_any_element(),
        Load::Ready(Attendance::TurnedOff) => muted(ATTENDANCE_OFF).into_any_element(),
        Load::Ready(Attendance::Report(records)) if records.is_empty() => {
            muted("No attendees recorded.").into_any_element()
        }
        Load::Ready(Attendance::Report(records)) => v_flex()
            .w_full()
            .gap(px(6.))
            .child(attendance_header())
            .children(records.iter().map(|record| attendance_row(record, offset)))
            .into_any_element(),
    };
    section("Attendance").child(body)
}

fn files_section(view: &RecapView) -> Option<Div> {
    (!view.files.is_empty()).then(|| {
        v_flex()
            .w_full()
            .flex_none()
            .gap(px(10.))
            .child(
                div()
                    .px(px(SIDE_PADDING))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child("Files"),
            )
            .children(view.files.iter().enumerate().map(|(index, item)| {
                let label = view.file_labels.get(index).cloned().flatten();
                item_row(index, item, label, &view.file_run)
            }))
    })
}

pub fn render_recap(view: RecapView, run: &RecapRun) -> AnyElement {
    let now_local = Local::now();
    let (offset, today) = (now_local.offset().fix(), now_local.date_naive());
    let body = if view.occurrences.is_empty() && view.files.is_empty() {
        div()
            .w_full()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(13.))
            .text_color(theme::text_muted())
            .child(EMPTY_RECAP)
            .into_any_element()
    } else {
        v_flex()
            .id("recap-scroll")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .py(px(16.))
            .gap(px(SECTION_GAP))
            .when(!view.occurrences.is_empty(), |column| {
                column.child(
                    v_flex()
                        .w_full()
                        .flex_none()
                        .px(px(SIDE_PADDING))
                        .gap(px(SECTION_GAP))
                        .child(picker(&view, offset, today, run))
                        .when(shows_recording(view.details), |column| {
                            column.child(recording_section(&view, run))
                        })
                        .child(transcript_section(&view, run))
                        .child(attendance_section(&view, offset)),
                )
            })
            .children(files_section(&view))
            .into_any_element()
    };
    v_flex()
        .size_full()
        .debug_selector(|| RECAP_REGION.into())
        .child(body)
        .into_any_element()
}
