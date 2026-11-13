use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use session::App;

pub(crate) const SLEEP_AFTER: Duration = Duration::from_secs(180);

/// What keeps the web views running; any of these blocks closing them.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Activity {
    pub restarting: bool,
    pub all_parked: bool,
    pub login_pending: bool,
    pub held: bool,
    pub in_flight: bool,
    pub dialogs_open: bool,
    pub subscribed: bool,
}

/// Names a protocol call without its content: the method, or for a script the host and first path segments it fetches.
pub(crate) fn call_label(method: &str, params: &serde_json::Value) -> String {
    if method != "Runtime.evaluate" {
        return method.to_owned();
    }
    let expression = params
        .get("expression")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if let Some(start) = expression.find("\"url\":\"https://") {
        let rest = &expression[start + "\"url\":\"https://".len()..];
        let end = rest.find(['"', '?', '\\']).unwrap_or(rest.len());
        return rest[..end].split('/').take(3).collect::<Vec<_>>().join("/");
    }
    if expression.contains("return token;") {
        return "token lookup".to_owned();
    }
    "script".to_owned()
}

#[derive(Debug, Default)]
pub(crate) struct CallTally {
    counts: HashMap<String, u32>,
}

impl CallTally {
    pub(crate) fn note(&mut self, label: String) {
        *self.counts.entry(label).or_default() += 1;
    }

    pub(crate) fn summary(&mut self) -> String {
        let mut counts: Vec<(String, u32)> = self.counts.drain().collect();
        counts.sort_by(|first, second| second.1.cmp(&first.1).then_with(|| first.0.cmp(&second.0)));
        counts
            .iter()
            .take(6)
            .map(|(label, count)| format!("{label} x{count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(crate) fn blocker(activity: Activity) -> Option<&'static str> {
    [
        (activity.restarting, "restarting"),
        (!activity.all_parked, "an app is not parked"),
        (activity.login_pending, "sign-in pending"),
        (activity.held, "an app is held"),
        (activity.in_flight, "a protocol call is running"),
        (activity.dialogs_open, "a dialog is open"),
        (activity.subscribed, "a tab subscriber is attached"),
    ]
    .into_iter()
    .find_map(|(blocked, reason)| blocked.then_some(reason))
}

/// The web views close once both apps are parked and idle, and come back on the next protocol call, at the park page.
#[derive(Debug)]
pub(crate) struct Sleep<C> {
    asleep: bool,
    last_use: Instant,
    loading: HashSet<App>,
    queued: HashMap<App, Vec<C>>,
}

impl<C> Default for Sleep<C> {
    fn default() -> Self {
        Sleep {
            asleep: false,
            last_use: Instant::now(),
            loading: HashSet::new(),
            queued: HashMap::new(),
        }
    }
}

impl<C> Sleep<C> {
    pub(crate) fn touch(&mut self, now: Instant) {
        self.last_use = now;
    }

    pub(crate) fn asleep(&self) -> bool {
        self.asleep
    }

    pub(crate) fn should_sleep(&self, activity: Activity, now: Instant) -> bool {
        self.quiet(now) && blocker(activity).is_none()
    }

    /// Answers whether the views have been idle long enough to close.
    pub(crate) fn quiet(&self, now: Instant) -> bool {
        !self.asleep && self.loading.is_empty() && now.duration_since(self.last_use) >= SLEEP_AFTER
    }

    pub(crate) fn fall_asleep(&mut self) {
        self.asleep = true;
    }

    /// Answers whether the views have to be created now.
    pub(crate) fn wake(&mut self) -> bool {
        if !self.asleep {
            return false;
        }
        self.asleep = false;
        self.loading = App::ALL.into_iter().collect();
        true
    }

    /// A call for an app that is still loading its page waits; `Some` hands it back to run now.
    pub(crate) fn admit(&mut self, app: App, command: C) -> Option<C> {
        if self.loading.contains(&app) {
            self.queued.entry(app).or_default().push(command);
            None
        } else {
            Some(command)
        }
    }

    pub(crate) fn loaded(&mut self, app: App) -> Vec<C> {
        self.loading.remove(&app);
        self.queued.remove(&app).unwrap_or_default()
    }

    pub(crate) fn reset(&mut self) -> Vec<C> {
        self.asleep = false;
        self.loading.clear();
        self.queued
            .drain()
            .flat_map(|(_, commands)| commands)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> Activity {
        Activity {
            all_parked: true,
            ..Activity::default()
        }
    }

    #[test]
    fn parked_idle_views_sleep_after_the_quiet_period() {
        let sleep: Sleep<u32> = Sleep::default();
        let start = sleep.last_use;
        assert!(!sleep.should_sleep(idle(), start + SLEEP_AFTER - Duration::from_secs(1)));
        assert!(sleep.should_sleep(idle(), start + SLEEP_AFTER));
    }

    #[test]
    fn a_running_app_a_login_or_any_user_keeps_them_awake() {
        let sleep: Sleep<u32> = Sleep::default();
        let later = sleep.last_use + SLEEP_AFTER;
        assert!(!sleep.should_sleep(
            Activity {
                all_parked: false,
                ..idle()
            },
            later
        ));
        for blocked in [
            Activity {
                login_pending: true,
                ..idle()
            },
            Activity {
                held: true,
                ..idle()
            },
            Activity {
                in_flight: true,
                ..idle()
            },
            Activity {
                dialogs_open: true,
                ..idle()
            },
            Activity {
                subscribed: true,
                ..idle()
            },
            Activity {
                restarting: true,
                ..idle()
            },
        ] {
            assert!(!sleep.should_sleep(blocked, later), "{blocked:?}");
        }
    }

    #[test]
    fn calls_are_named_by_what_they_fetch() {
        let fetch = serde_json::json!({"expression": "(async () => {})({\"requests\":[{\"method\":\"GET\",\"url\":\"https://teams.cloud.microsoft/api/csa/emea/api/v1/x?y=1\"}]})"});
        assert_eq!(
            call_label("Runtime.evaluate", &fetch),
            "teams.cloud.microsoft/api/csa"
        );
        let token = serde_json::json!({"expression": "x; return token;"});
        assert_eq!(call_label("Runtime.evaluate", &token), "token lookup");
        assert_eq!(
            call_label("Page.reload", &serde_json::Value::Null),
            "Page.reload"
        );
        let mut tally = CallTally::default();
        tally.note("b".into());
        tally.note("a".into());
        tally.note("a".into());
        assert_eq!(tally.summary(), "a x2, b x1");
        assert_eq!(tally.summary(), "");
    }

    #[test]
    fn the_first_blocker_is_named() {
        assert_eq!(blocker(idle()), None);
        assert_eq!(
            blocker(Activity {
                held: true,
                dialogs_open: true,
                ..idle()
            }),
            Some("an app is held")
        );
    }

    #[test]
    fn calls_wait_for_the_page_after_waking_and_run_in_order() {
        let mut sleep = Sleep::default();
        sleep.fall_asleep();
        assert!(sleep.wake());
        assert!(!sleep.wake());
        assert_eq!(sleep.admit(App::Teams, 1), None);
        assert_eq!(sleep.admit(App::Teams, 2), None);
        assert!(!sleep.should_sleep(idle(), sleep.last_use + SLEEP_AFTER));
        assert_eq!(sleep.loaded(App::Teams), vec![1, 2]);
        assert_eq!(sleep.admit(App::Teams, 4), Some(4));
        sleep.fall_asleep();
        assert!(sleep.wake());
        assert_eq!(sleep.admit(App::Teams, 3), None);
        assert_eq!(sleep.reset(), vec![3]);
        assert_eq!(sleep.admit(App::Teams, 5), Some(5));
    }
}
