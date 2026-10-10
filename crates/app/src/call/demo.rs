use std::time::{Duration, Instant};

use calling::{
    AudioDevice, CallCommand, CallControl, CallHandle, CallState, CallUpdate, Caller, DeviceChoice, DeviceLists, EndReason,
    IncomingRing, Progress, RosterEntry, call_channel,
};
use tokio::time::{interval, sleep};

use super::model::{ECHO_MRI, ECHO_NAME};

const CONNECT_DELAY: Duration = Duration::from_millis(2200);
const RING_START: Duration = Duration::from_millis(1400);
const ANSWER_DELAY: Duration = Duration::from_millis(3200);
const INCOMING_CONNECT_DELAY: Duration = Duration::from_millis(600);
const LOBBY_WAIT: Duration = Duration::from_secs(6);
const LEVEL_PERIOD: Duration = Duration::from_millis(200);
const CYCLE_TICKS: u32 = 40;
const REMOTE_SPEAKS: std::ops::Range<u32> = 0..18;
const LOCAL_SPEAKS: std::ops::Range<u32> = 24..32;
const SPEAKING_LEVEL: f32 = 0.3;
const SPEAKER_TICKS: u32 = 10;
pub const DEMO_OWN_MRI: &str = "8:orgid:demo-me";
pub const DEMO_RING_ID: u64 = 9001;
const DEMO_CALLER: (&str, &str) = ("8:orgid:demo-mara", "Mara Lindqvist");
const DEMO_GUESTS: [&str; 11] = [
    "Mara Lindqvist",
    "Jonas Ortega",
    "Priya Nair",
    "Lea Schneider",
    "Tobias Klein",
    "Ines Duarte",
    "Noor Haddad",
    "Felix Brandt",
    "Yuki Tanaka",
    "Carlos Mendez",
    "Sofia Rossi",
];

pub type Person = (String, String);

pub enum DemoCall {
    Test,
    People(Vec<Person>),
    Meeting { guests: Vec<Person>, lobby: bool },
    Incoming(Person),
}

pub fn demo_guests(count: usize) -> Vec<Person> {
    DEMO_GUESTS
        .iter()
        .take(count)
        .enumerate()
        .map(|(index, name)| (format!("8:orgid:demo-guest-{index}"), (*name).to_owned()))
        .collect()
}

pub fn demo_caller() -> Person {
    (DEMO_CALLER.0.to_owned(), DEMO_CALLER.1.to_owned())
}

pub fn demo_ring() -> IncomingRing {
    let (mri, display_name) = demo_caller();
    IncomingRing {
        ring_id: DEMO_RING_ID,
        caller: Caller { mri, display_name },
        thread_id: Some("demo-chat-mara".to_owned()),
        video: false,
        is_group: false,
        subject: None,
    }
}

fn device(index: u16, name: &str) -> AudioDevice {
    AudioDevice {
        index,
        guid: format!("demo-{index}"),
        name: name.to_owned(),
    }
}

fn demo_devices() -> DeviceLists {
    DeviceLists {
        inputs: vec![device(0, "Headset Microphone"), device(1, "Built-in Microphone")],
        outputs: vec![device(0, "Headset"), device(1, "Speakers")],
    }
}

pub fn start_demo_call(script: DemoCall) -> CallHandle {
    let (handle, control) = call_channel();
    crate::runtime::handle().spawn(run_demo_call(control, script));
    handle
}

fn roster_of(people: &[Person]) -> Vec<RosterEntry> {
    people
        .iter()
        .enumerate()
        .map(|(index, (mri, name))| RosterEntry {
            mri: mri.clone(),
            display_name: name.clone(),
            muted: index % 3 == 1,
            in_lobby: false,
        })
        .collect()
}

async fn run_demo_call(mut control: CallControl, script: DemoCall) {
    control.send(CallUpdate::OwnIdentity {
        mri: DEMO_OWN_MRI.to_owned(),
    });
    let (people, lobby) = match &script {
        DemoCall::Test => (vec![(ECHO_MRI.to_owned(), ECHO_NAME.to_owned())], false),
        DemoCall::People(callees) => (callees.clone(), false),
        DemoCall::Meeting { guests, lobby } => (guests.clone(), *lobby),
        DemoCall::Incoming(caller) => (vec![caller.clone()], false),
    };
    if !ring_until_answered(&mut control, &script).await {
        control.send(CallUpdate::State(CallState::Ended {
            reason: EndReason::Cancelled,
        }));
        return;
    }
    control.send(CallUpdate::ListenOnly(false));
    control.send(CallUpdate::Devices(demo_devices()));
    control.send(CallUpdate::Selected {
        input: DeviceChoice::SystemDefault,
        output: DeviceChoice::SystemDefault,
    });
    control.send(CallUpdate::State(CallState::Connected {
        since: Instant::now(),
    }));
    if lobby {
        control.send(CallUpdate::Lobby(true));
        if control.until_hangup(sleep(LOBBY_WAIT)).await.is_err() {
            end(&control, EndReason::LocalHangup);
            return;
        }
        control.send(CallUpdate::Lobby(false));
    }
    control.send(CallUpdate::Roster(roster_of(&people)));
    speak_until_hangup(control, people).await;
}

async fn ring_until_answered(control: &mut CallControl, script: &DemoCall) -> bool {
    let steps: Vec<(Option<Progress>, Duration)> = match script {
        DemoCall::People(_) => vec![
            (Some(Progress::Calling), RING_START),
            (Some(Progress::Ringing), ANSWER_DELAY - RING_START),
        ],
        DemoCall::Incoming(_) => vec![(None, INCOMING_CONNECT_DELAY)],
        DemoCall::Test | DemoCall::Meeting { .. } => vec![(None, CONNECT_DELAY)],
    };
    for (progress, wait) in steps {
        if let Some(progress) = progress {
            control.send(CallUpdate::Progress(progress));
        }
        if control.until_hangup(sleep(wait)).await.is_err() {
            return false;
        }
    }
    true
}

async fn speak_until_hangup(mut control: CallControl, people: Vec<Person>) {
    let mut muted = false;
    let mut input = DeviceChoice::SystemDefault;
    let mut output = DeviceChoice::SystemDefault;
    let mut ticks = 0u32;
    let mut tick = interval(LEVEL_PERIOD);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let phase = ticks % CYCLE_TICKS;
                let remote_speaks = REMOTE_SPEAKS.contains(&phase);
                control.send(CallUpdate::Levels {
                    local: if LOCAL_SPEAKS.contains(&phase) && !muted { SPEAKING_LEVEL } else { 0.0 },
                    remote: if remote_speaks { SPEAKING_LEVEL } else { 0.0 },
                });
                if people.len() > 1 && ticks.is_multiple_of(SPEAKER_TICKS) {
                    let speaker = &people[(ticks / SPEAKER_TICKS) as usize % people.len()];
                    control.send(CallUpdate::Speakers(vec![speaker.0.clone()]));
                }
                ticks += 1;
            }
            command = control.recv() => match command {
                Some(CallCommand::Mute(command)) => {
                    muted = command.target(muted);
                    control.send(CallUpdate::Muted(muted));
                }
                Some(CallCommand::SelectInput(choice)) => {
                    input = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                }
                Some(CallCommand::SelectOutput(choice)) => {
                    output = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                }
                Some(CallCommand::RefreshDevices) => control.send(CallUpdate::Devices(demo_devices())),
                Some(CallCommand::Hangup | CallCommand::EndMeeting) | None => {
                    end(&control, EndReason::LocalHangup);
                    return;
                }
            },
        }
    }
}

fn end(control: &CallControl, reason: EndReason) {
    control.send(CallUpdate::State(CallState::Ended { reason }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_guests_have_distinct_mris_and_names() {
        let guests = demo_guests(11);
        assert_eq!(guests.len(), 11);
        let mut mris: Vec<&String> = guests.iter().map(|guest| &guest.0).collect();
        mris.sort();
        mris.dedup();
        assert_eq!(mris.len(), 11);
    }

    #[test]
    fn the_demo_ring_names_a_caller_and_a_chat() {
        let ring = demo_ring();
        assert_eq!(ring.caller.display_name, "Mara Lindqvist");
        assert_eq!(ring.thread_id.as_deref(), Some("demo-chat-mara"));
    }
}
