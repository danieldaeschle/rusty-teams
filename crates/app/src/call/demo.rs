use std::time::{Duration, Instant};

use calling::{
    AudioDevice, CallCommand, CallControl, CallHandle, CallState, CallUpdate, DeviceChoice,
    DeviceLists, EndReason, call_channel,
};
use tokio::time::{interval, sleep};

const CONNECT_DELAY: Duration = Duration::from_millis(2200);
const LEVEL_PERIOD: Duration = Duration::from_millis(200);
const CYCLE_TICKS: u32 = 40;
const REMOTE_SPEAKS: std::ops::Range<u32> = 0..18;
const LOCAL_SPEAKS: std::ops::Range<u32> = 24..32;
const SPEAKING_LEVEL: f32 = 0.3;

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

pub fn start_demo_call() -> CallHandle {
    let (handle, control) = call_channel();
    crate::runtime::handle().spawn(run_demo_call(control));
    handle
}

async fn run_demo_call(mut control: CallControl) {
    if control.until_hangup(sleep(CONNECT_DELAY)).await.is_err() {
        end(&control, EndReason::Cancelled);
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
    let mut muted = false;
    let mut input = DeviceChoice::SystemDefault;
    let mut output = DeviceChoice::SystemDefault;
    let mut ticks = 0u32;
    let mut tick = interval(LEVEL_PERIOD);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let phase = ticks % CYCLE_TICKS;
                ticks += 1;
                control.send(CallUpdate::Levels {
                    local: if LOCAL_SPEAKS.contains(&phase) && !muted { SPEAKING_LEVEL } else { 0.0 },
                    remote: if REMOTE_SPEAKS.contains(&phase) { SPEAKING_LEVEL } else { 0.0 },
                });
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
                Some(CallCommand::Hangup) | None => {
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
