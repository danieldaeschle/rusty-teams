//! System-wide idle time on Linux: Wayland `ext-idle-notify-v1`, else the X11 screensaver
//! extension. `None` where neither is offered, as before.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

/// The compositor reports idle only after this much quiet, so it is added back on.
const NOTIFY_AFTER: Duration = Duration::from_secs(5);
const INPUT_IDLE_VERSION: u32 = 2;

#[derive(Default)]
struct WaylandIdle {
    available: bool,
    idle_since: Option<Instant>,
}

pub fn system_idle() -> Option<Duration> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return wayland_idle();
    }
    x11_idle()
}

fn wayland_idle() -> Option<Duration> {
    static STATE: OnceLock<Arc<Mutex<WaylandIdle>>> = OnceLock::new();
    let state = STATE.get_or_init(|| {
        let state = Arc::new(Mutex::new(WaylandIdle::default()));
        let watched = state.clone();
        let _ = std::thread::Builder::new()
            .name("wayland-idle".into())
            .spawn(move || watch_wayland(watched));
        state
    });
    let state = state.lock().ok()?;
    state
        .available
        .then(|| idle_duration(state.idle_since, Instant::now()))
}

pub fn idle_duration(idle_since: Option<Instant>, now: Instant) -> Duration {
    idle_since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
}

struct Watcher {
    state: Arc<Mutex<WaylandIdle>>,
}

fn watch_wayland(state: Arc<Mutex<WaylandIdle>>) -> Option<()> {
    let connection = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<Watcher>(&connection).ok()?;
    let handle = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&handle, 1..=1, ()).ok()?;
    let notifier: ExtIdleNotifierV1 = globals.bind(&handle, 1..=INPUT_IDLE_VERSION, ()).ok()?;
    let timeout = NOTIFY_AFTER.as_millis() as u32;
    // Version 2 counts input only, like Windows' last-input time; version 1 also honours
    // idle inhibitors such as a playing video.
    let _notification = if notifier.version() >= INPUT_IDLE_VERSION {
        notifier.get_input_idle_notification(timeout, &seat, &handle, ())
    } else {
        notifier.get_idle_notification(timeout, &seat, &handle, ())
    };
    state.lock().ok()?.available = true;
    let mut watcher = Watcher { state };
    loop {
        queue.blocking_dispatch(&mut watcher).ok()?;
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for Watcher {
    fn event(
        watcher: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Ok(mut state) = watcher.state.lock() else {
            return;
        };
        match event {
            ext_idle_notification_v1::Event::Idled => {
                state.idle_since = Instant::now().checked_sub(NOTIFY_AFTER);
            }
            ext_idle_notification_v1::Event::Resumed => state.idle_since = None,
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Watcher {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Watcher {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for Watcher {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn x11_idle() -> Option<Duration> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::screensaver::ConnectionExt as _;

    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let info = connection.screensaver_query_info(root).ok()?.reply().ok()?;
    Some(Duration::from_millis(u64::from(info.ms_since_user_input)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_counts_from_the_reported_start_and_is_zero_while_active() {
        let now = Instant::now();
        assert_eq!(idle_duration(None, now), Duration::ZERO);
        let since = now.checked_sub(Duration::from_secs(400)).unwrap();
        assert_eq!(idle_duration(Some(since), now), Duration::from_secs(400));
    }
}
