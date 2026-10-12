use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use chatsvc::{
    Attendance, Occurrence, Recap, RecordingDetails, TranscriptFormat, TranscriptTurn,
};
use chrono::Utc;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::ConversationView;
use crate::app_state::Selection;
use crate::channel_files::SharedContent;
use crate::demo_recap;
use crate::downloads::{self, DownloadState};
use crate::notice::short_error;
use crate::recap::{DELETE_QUESTION, Load, save_name};
use crate::runtime;
use crate::views::channel_tabs::{ChannelPane, render_meeting_bar};
use crate::views::recap_tab::{RecapCommand, RecapDownload, RecapRun, RecapView, render_recap};
use crate::views::shared_tab::{SharedCommand, SharedRun};

const SEARCH_PLACEHOLDER: &str = "Search transcript";
const RECORDING_EXTENSION: &str = "mp4";

pub(super) fn search_input(
    window: &mut Window,
    cx: &mut Context<ConversationView>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(SEARCH_PLACEHOLDER))
}

pub(super) struct RecapState {
    listing: Load<Vec<Occurrence>>,
    selected: usize,
    details: Load<RecordingDetails>,
    transcript: Load<Vec<TranscriptTurn>>,
    attendance: Load<Attendance>,
    query: String,
    downloads: HashMap<RecapDownload, DownloadState>,
    generation: u64,
}

impl Default for RecapState {
    fn default() -> Self {
        RecapState {
            listing: Load::Loading,
            selected: 0,
            details: Load::Loading,
            transcript: Load::Loading,
            attendance: Load::Loading,
            query: String::new(),
            downloads: HashMap::new(),
            generation: 0,
        }
    }
}

enum SaveEvent {
    Progress(u8),
    Finished(Result<PathBuf, String>),
}

impl ConversationView {
    fn is_meeting_chat(&self, cx: &App) -> bool {
        let Some(Selection::Chat(chat_id)) = self.current.as_ref().map(|current| &current.selection)
        else {
            return false;
        };
        self.app
            .read(cx)
            .sidebar
            .chats
            .iter()
            .any(|chat| &chat.id == chat_id && chat.kind.eq_ignore_ascii_case("meeting"))
    }

    fn occurrences(&self) -> &[Occurrence] {
        self.recap.listing.ready().map_or(&[], Vec::as_slice)
    }

    fn selected_occurrence(&self) -> Option<Occurrence> {
        self.occurrences().get(self.recap.selected).cloned()
    }

    pub(super) fn recap_active(&self) -> bool {
        self.pane == ChannelPane::Recap && !self.occurrences().is_empty()
    }

    pub(super) fn on_recap_search_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.recap.query = input.read(cx).value().to_string();
            cx.notify();
        }
    }

    pub(super) fn reset_recap(&mut self, cx: &mut Context<Self>) {
        self.recap = RecapState {
            generation: self.recap.generation + 1,
            ..RecapState::default()
        };
        if !self.is_meeting_chat(cx) {
            return;
        }
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        if demo {
            self.recap.listing = Load::Ready(demo_recap::occurrences(&conversation_id, Utc::now()));
            return;
        }
        let Some(engine) = engine else {
            return;
        };
        let generation = self.recap.generation;
        let receiver = runtime::spawn(async move {
            Recap::new(engine.remote().session())
                .occurrences(&conversation_id)
                .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(occurrences)) => Ok(occurrences),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(error) => Err(short_error(&error)),
            };
            this.update(cx, |this, cx| {
                if generation != this.recap.generation {
                    return;
                }
                this.recap.listing = match outcome {
                    Ok(occurrences) => Load::Ready(occurrences),
                    Err(reason) => Load::Failed(reason),
                };
                if this.pane == ChannelPane::Recap {
                    this.select_occurrence(0, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_recap_pane(&mut self, cx: &mut Context<Self>) {
        self.refresh_shared_items(cx);
        if matches!(self.recap.details, Load::Loading) {
            self.select_occurrence(0, cx);
        }
    }

    fn select_occurrence(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(occurrence) = self.occurrences().get(index).cloned() else {
            return;
        };
        self.recap.selected = index;
        self.recap.details = Load::Loading;
        self.recap.transcript = Load::Loading;
        self.recap.attendance = Load::Loading;
        self.recap.downloads.clear();
        self.recap.generation += 1;
        let generation = self.recap.generation;
        cx.notify();
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        if demo {
            let now = Utc::now();
            self.recap.details = Load::Ready(demo_recap::details(&occurrence, now));
            self.recap.transcript = Load::Ready(demo_recap::transcript());
            self.recap.attendance = Load::Ready(demo_recap::attendance(now));
            return;
        }
        let (Some(engine), Some(conversation_id)) = (engine, self.conversation_id()) else {
            return;
        };
        let attendance_engine = engine.clone();
        let loaded = runtime::spawn(async move {
            let recap = Recap::new(engine.remote().session());
            let details = recap.details(&occurrence).await?;
            let transcript = recap.transcript(&details).await.ok();
            Ok::<_, chatsvc::TranscriptError>((details, transcript))
        });
        let attendance = runtime::spawn(async move {
            Recap::new(attendance_engine.remote().session())
                .attendance(&conversation_id)
                .await
        });
        cx.spawn(async move |this, cx| {
            let loaded = match loaded.await {
                Ok(Ok(loaded)) => Ok(loaded),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(error) => Err(short_error(&error)),
            };
            this.update(cx, |this, cx| {
                if generation != this.recap.generation {
                    return;
                }
                match loaded {
                    Ok((details, transcript)) => {
                        this.recap.details = Load::Ready(details);
                        this.recap.transcript = Load::Ready(transcript.unwrap_or_default());
                    }
                    Err(reason) => {
                        this.recap.details = Load::Failed(reason.clone());
                        this.recap.transcript = Load::Failed(reason);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let outcome = match attendance.await {
                Ok(Ok(attendance)) => Ok(attendance),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(error) => Err(short_error(&error)),
            };
            this.update(cx, |this, cx| {
                if generation != this.recap.generation {
                    return;
                }
                this.recap.attendance = match outcome {
                    Ok(attendance) => Load::Ready(attendance),
                    Err(reason) => Load::Failed(reason),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_meeting_tab_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.occurrences().is_empty() || !self.is_meeting_chat(cx) {
            return None;
        }
        let view = cx.weak_entity();
        let select = Rc::new(move |pane, cx: &mut App| {
            view.update(cx, |this, cx| this.select_pane(pane, cx)).ok();
        });
        Some(render_meeting_bar(self.pane, select))
    }

    pub(super) fn render_recap(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.weak_entity();
        let run: RecapRun = Rc::new(move |command, window, cx| {
            view.update(cx, |this, cx| this.run_recap(command, window, cx))
                .ok();
        });
        let file_indices: Rc<Vec<usize>> = Rc::new(
            self.shared
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| matches!(item.content, SharedContent::File(_)))
                .map(|(index, _)| index)
                .collect(),
        );
        let file_states = self.item_download_states();
        let files = file_indices
            .iter()
            .map(|index| self.shared.items[*index].clone())
            .collect::<Vec<_>>();
        let file_labels = file_indices
            .iter()
            .map(|index| {
                crate::views::shared_tab::download_label(
                    file_states.get(*index).and_then(Option::as_ref),
                )
            })
            .collect();
        let file_run: SharedRun = {
            let view = cx.weak_entity();
            Rc::new(move |command, window, cx| {
                let command = match command {
                    SharedCommand::OpenItem(index) => SharedCommand::OpenItem(file_indices[index]),
                    SharedCommand::JumpToPost(index) => {
                        SharedCommand::JumpToPost(file_indices[index])
                    }
                    other => other,
                };
                view.update(cx, |this, cx| {
                    if matches!(command, SharedCommand::JumpToPost(_)) {
                        this.pane = ChannelPane::Posts;
                    }
                    this.run_shared(command, window, cx)
                })
                .ok();
            })
        };
        let can_delete = self.can_delete_recording(cx);
        render_recap(
            RecapView {
                occurrences: self.occurrences(),
                selected: self.recap.selected,
                details: &self.recap.details,
                transcript: &self.recap.transcript,
                attendance: &self.recap.attendance,
                search: &self.recap_search,
                query: &self.recap.query,
                can_delete,
                recording_state: self.recap.downloads.get(&RecapDownload::Recording),
                vtt_state: self
                    .recap
                    .downloads
                    .get(&RecapDownload::Transcript(TranscriptFormat::Vtt)),
                docx_state: self
                    .recap
                    .downloads
                    .get(&RecapDownload::Transcript(TranscriptFormat::Docx)),
                files: &files,
                file_labels,
                file_run,
                now: Utc::now(),
            },
            &run,
        )
    }

    fn can_delete_recording(&self, cx: &App) -> bool {
        let my_user_id = self.app.read(cx).directory.me.as_ref().map(|me| me.user_id.as_str());
        match (self.selected_occurrence(), my_user_id) {
            (Some(occurrence), Some(my_user_id)) => {
                occurrence.organizer_id.as_deref() == Some(my_user_id)
            }
            _ => false,
        }
    }

    fn run_recap(&mut self, command: RecapCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            RecapCommand::Occurrence(index) => self.select_occurrence(index, cx),
            RecapCommand::Retry => self.select_occurrence(self.recap.selected, cx),
            RecapCommand::Play => {
                if let Some(occurrence) = self.selected_occurrence() {
                    cx.open_url(&occurrence.recording_url);
                }
            }
            RecapCommand::Download(download) => self.pick_save_target(download, cx),
            RecapCommand::AskDelete => self.confirm_delete(window, cx),
        }
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let cancel = Button::new("recap-delete-cancel")
                .label("Cancel")
                .on_click(|_, window, cx| window.close_dialog(cx));
            let delete = Button::new("recap-delete-confirm")
                .label("Delete")
                .danger()
                .on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| this.delete_recording(cx)).ok();
                    window.close_dialog(cx);
                });
            alert
                .title(DELETE_QUESTION)
                .footer(DialogFooter::new().child(cancel).child(delete))
                .on_ok(|_, _, _| false)
        });
    }

    fn delete_recording(&mut self, cx: &mut Context<Self>) {
        let (Some(occurrence), Some(details)) = (
            self.selected_occurrence(),
            self.recap.details.ready().cloned(),
        ) else {
            return;
        };
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        if demo {
            self.finish_delete(&occurrence, Ok(()), cx);
            return;
        }
        let Some(engine) = engine else {
            return;
        };
        let receiver = runtime::spawn(async move {
            Recap::new(engine.remote().session())
                .delete_recording(&details)
                .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(error) => Err(short_error(&error)),
            };
            this.update(cx, |this, cx| this.finish_delete(&occurrence, outcome, cx))
                .ok();
        })
        .detach();
    }

    fn finish_delete(
        &mut self,
        occurrence: &Occurrence,
        outcome: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if let Err(reason) = outcome {
            self.notice = Some(format!("Recording not deleted: {reason}"));
            cx.notify();
            return;
        }
        let remaining: Vec<Occurrence> = self
            .occurrences()
            .iter()
            .filter(|other| other.message_id != occurrence.message_id)
            .cloned()
            .collect();
        let empty = remaining.is_empty();
        self.recap.listing = Load::Ready(remaining);
        if empty {
            self.pane = ChannelPane::Posts;
            cx.notify();
        } else {
            self.select_occurrence(0, cx);
        }
    }

    fn pick_save_target(&mut self, download: RecapDownload, cx: &mut Context<Self>) {
        let Some(occurrence) = self.selected_occurrence() else {
            return;
        };
        if matches!(
            self.recap.downloads.get(&download),
            Some(DownloadState::Saving(_))
        ) {
            return;
        }
        let extension = match download {
            RecapDownload::Recording => RECORDING_EXTENSION,
            RecapDownload::Transcript(format) => format.query(),
        };
        let name = save_name(&occurrence, extension);
        let receiver = cx.prompt_for_new_path(&downloads::downloads_directory(), Some(&name));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(target))) = receiver.await {
                this.update(cx, |this, cx| this.save_to(download, occurrence, target, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn save_to(
        &mut self,
        download: RecapDownload,
        occurrence: Occurrence,
        target: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.recap
            .downloads
            .insert(download, DownloadState::Saving(0));
        cx.notify();
        let generation = self.recap.generation;
        let state = self.app.read(cx);
        let (demo, engine) = (state.mode.demo, state.engine.clone());
        let details = self.recap.details.ready().cloned();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<SaveEvent>();
        if demo {
            let outcome = std::fs::write(&target, demo_recap::DEMO_FILE_BYTES)
                .map(|_| target)
                .map_err(|error| short_error(&error));
            self.recap.downloads.insert(download, finished_state(outcome));
            return;
        }
        let (Some(engine), Some(details)) = (engine, details) else {
            self.recap
                .downloads
                .insert(download, DownloadState::Failed("not connected".to_owned()));
            return;
        };
        drop(runtime::spawn(async move {
            let outcome = save_download(&engine, download, &occurrence, &details, target, &sender)
                .await;
            let _ = sender.send(SaveEvent::Finished(outcome));
        }));
        cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.recv().await {
                let updated = this.update(cx, |this, cx| {
                    if generation == this.recap.generation {
                        match event {
                            SaveEvent::Progress(percent) => {
                                this.recap
                                    .downloads
                                    .insert(download, DownloadState::Saving(percent));
                            }
                            SaveEvent::Finished(outcome) => {
                                this.recap.downloads.insert(download, finished_state(outcome));
                            }
                        }
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    return;
                }
            }
        })
        .detach();
    }
}

fn finished_state(outcome: Result<PathBuf, String>) -> DownloadState {
    match outcome {
        Ok(path) => DownloadState::Saved(path),
        Err(reason) => DownloadState::Failed(reason),
    }
}

async fn save_download(
    engine: &crate::backend::Engine,
    download: RecapDownload,
    occurrence: &Occurrence,
    details: &RecordingDetails,
    target: PathBuf,
    events: &tokio::sync::mpsc::UnboundedSender<SaveEvent>,
) -> Result<PathBuf, String> {
    match download {
        RecapDownload::Transcript(format) => {
            let bytes = Recap::new(engine.remote().session())
                .transcript_file(details, format)
                .await
                .map_err(|error| short_error(&error))?;
            std::fs::write(&target, bytes).map_err(|error| short_error(&error))?;
        }
        RecapDownload::Recording => {
            let mut file = std::fs::File::create(&target).map_err(|error| short_error(&error))?;
            let progress = |percent| {
                let _ = events.send(SaveEvent::Progress(percent));
            };
            let result = engine
                .download_file(
                    &occurrence.recording_url,
                    |bytes: &[u8]| file.write_all(bytes),
                    progress,
                )
                .await;
            if let Err(error) = result {
                drop(file);
                let _ = std::fs::remove_file(&target);
                return Err(short_error(&error));
            }
        }
    }
    Ok(target)
}
