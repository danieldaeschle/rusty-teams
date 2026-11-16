use super::*;

pub(super) fn is_own_meeting(own: Option<&MeetingTarget>, target: &MeetingTarget) -> bool {
    own.is_some_and(|own| own.thread_id == target.thread_id)
}

pub(super) async fn save_meeting_options(
    session: &session::Session,
    target: &MeetingTarget,
    writes: &[OptionWrite],
) -> std::result::Result<meeting_options::MeetingOptions, String> {
    for change in writes {
        meeting_options::write(session, target, change)
            .await
            .map_err(|error| error.to_string())?;
    }
    meeting_options::fetch(session, target)
        .await
        .map_err(|error| error.to_string())
}

pub(super) fn target_of<'a>(roster: &'a Roster, mri: &'a str) -> Target<'a> {
    let display_name = roster
        .members()
        .iter()
        .find(|member| member.mri == mri)
        .map_or("", |member| member.display_name.as_str());
    Target { mri, display_name }
}

pub(super) const RECORDING_STOP_RETRIES: usize = 6;

pub(super) const RECORDING_STOP_RETRY_WAIT: Duration = Duration::from_secs(5);

/// The recorder answers 409 to a stop sent while its start is still settling.
pub(super) fn recorder_busy(error: &Error) -> bool {
    matches!(error, Error::Signaling(reason) if reason.contains("HTTP 409"))
}

pub(super) fn report_organizer_result(
    result: Result<u16>,
    label: &str,
    notice: &'static str,
    control: &CallControl,
    timeline: &Timeline,
) {
    if let Err(error) = &result {
        timeline.record(format!("{label} failed"), error.to_string());
        control.send(CallUpdate::Notice(notice.to_owned()));
    }
    control.send(CallUpdate::Organizer {
        action: label.to_owned(),
        outcome: result.map_err(|error| error.to_string()),
    });
}

pub(super) fn named_caption(mut entry: CaptionEntry, roster: &Roster) -> CaptionEntry {
    if entry.display_name.is_empty() {
        entry.display_name = roster
            .name_of(&entry.user_id)
            .unwrap_or_default()
            .to_owned();
    }
    entry
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BotJob {
    Captions,
    Recording,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_bot_steps(
    job: BotJob,
    flow: &mut CaptionFlow,
    roster: &Roster,
    remote: &RemoteCall,
    links: &CallbackLinks,
    meeting_chat: Option<&str>,
    plan: &RecordingPlan,
    bot_pending: &mut bool,
    control: &CallControl,
    timeline: &Timeline,
) {
    let is_active: fn(&crate::roster::CaptionBot) -> bool = match job {
        BotJob::Captions => |bot| bot.active,
        BotJob::Recording => |bot| bot.recording,
    };
    let fail = |flow: &mut CaptionFlow, reason: &str| {
        flow.failed();
        control.send(match job {
            BotJob::Captions => CallUpdate::Captions(CaptionState::Failed(reason.to_owned())),
            BotJob::Recording => CallUpdate::Notice(reason.to_owned()),
        });
        if job == BotJob::Recording {
            control.send(CallUpdate::RecordingPending(false));
        }
    };
    let (join_failure, start_failure) = match job {
        BotJob::Captions => (CAPTIONS_JOIN_FAILED_NOTICE, CAPTIONS_START_FAILED_NOTICE),
        BotJob::Recording => (RECORDING_JOIN_FAILED_NOTICE, RECORDING_START_FAILED_NOTICE),
    };
    loop {
        match flow.next_for(roster.caption_bot(), is_active) {
            CaptionStep::Wait => return,
            CaptionStep::AddBot => {
                if *bot_pending {
                    continue;
                }
                let Some(thread) = meeting_chat else {
                    return fail(
                        flow,
                        "This call has no chat for the captions and the recording",
                    );
                };
                *bot_pending = true;
                let added = remote
                    .signaling
                    .add_caption_bot(
                        &remote.conversation,
                        &remote.participant,
                        thread,
                        links,
                        remote.threadless,
                    )
                    .await;
                if job == BotJob::Recording {
                    report_organizer_result(
                        added
                            .as_ref()
                            .map(|status| *status)
                            .map_err(|error| Error::Signaling(error.to_string())),
                        "recording add bot",
                        join_failure,
                        control,
                        timeline,
                    );
                }
                if let Err(error) = added {
                    timeline.record("bot invitation failed", error.to_string());
                    *bot_pending = false;
                    return fail(flow, join_failure);
                }
            }
            CaptionStep::Start => {
                let Some(url) = roster.caption_bot().and_then(|bot| bot.command_url.clone()) else {
                    return;
                };
                match job {
                    BotJob::Captions => {
                        if let Err(error) = remote
                            .signaling
                            .caption_command(&url, BotAction::Start, &remote.participant)
                            .await
                        {
                            timeline.record("caption start failed", error.to_string());
                            return fail(flow, start_failure);
                        }
                        if let Err(error) = remote
                            .signaling
                            .set_caption_preference(&remote.conversation, &remote.participant, true)
                            .await
                        {
                            timeline.record("caption preference failed", error.to_string());
                        }
                    }
                    BotJob::Recording => {
                        let started = remote
                            .signaling
                            .recording_command(&url, plan.start(), &remote.participant, &plan.title)
                            .await;
                        let failed = started.is_err();
                        report_organizer_result(
                            started,
                            "recording start",
                            start_failure,
                            control,
                            timeline,
                        );
                        if failed {
                            return fail(flow, start_failure);
                        }
                    }
                }
            }
            CaptionStep::Stop => {
                let Some(url) = roster.caption_bot().and_then(|bot| bot.command_url.clone()) else {
                    continue;
                };
                match job {
                    BotJob::Captions => {
                        if let Err(error) = remote
                            .signaling
                            .caption_command(&url, BotAction::Stop, &remote.participant)
                            .await
                        {
                            timeline.record("caption stop failed", error.to_string());
                        }
                        if let Err(error) = remote
                            .signaling
                            .set_caption_preference(
                                &remote.conversation,
                                &remote.participant,
                                false,
                            )
                            .await
                        {
                            timeline.record("caption preference failed", error.to_string());
                        }
                    }
                    BotJob::Recording => {
                        let remote = remote.clone();
                        let updates = control.updates();
                        let (command, title) = (plan.stop(), plan.title.clone());
                        tokio::spawn(async move {
                            let mut stopped = remote
                                .signaling
                                .recording_command(&url, command, &remote.participant, &title)
                                .await;
                            for _ in 0..RECORDING_STOP_RETRIES {
                                if !stopped.as_ref().is_err_and(recorder_busy) {
                                    break;
                                }
                                tokio::time::sleep(RECORDING_STOP_RETRY_WAIT).await;
                                stopped = remote
                                    .signaling
                                    .recording_command(&url, command, &remote.participant, &title)
                                    .await;
                            }
                            if stopped.is_err() {
                                let _ = updates
                                    .send(CallUpdate::Notice(RECORDING_STOP_FAILED_NOTICE.into()));
                            }
                            let _ = updates.send(CallUpdate::Organizer {
                                action: "recording stop".into(),
                                outcome: stopped.map_err(|error| error.to_string()),
                            });
                        });
                    }
                }
            }
            CaptionStep::ReportOn => {
                if job == BotJob::Captions {
                    control.send(CallUpdate::Captions(CaptionState::On));
                }
            }
        }
    }
}
