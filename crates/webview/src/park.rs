use std::collections::HashMap;

use session::App;

const MAX_COMMAND_WAITS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParkDecision {
    Park,
    Wait,
}

/// A navigation of the app tab destroys the page context of every running protocol call
/// and the Trouter sockets of a live call, so parking waits until the tab is idle.
#[derive(Debug, Default)]
pub(crate) struct ParkGate {
    holds: HashMap<App, usize>,
    in_flight: HashMap<App, usize>,
    command_waits: HashMap<App, usize>,
}

impl ParkGate {
    pub(crate) fn hold(&mut self, app: App, held: bool) {
        adjust(&mut self.holds, app, held);
    }

    pub(crate) fn held(&self, app: App) -> bool {
        self.holds.get(&app).copied().unwrap_or(0) > 0
    }

    pub(crate) fn command_started(&mut self, app: App) {
        adjust(&mut self.in_flight, app, true);
    }

    pub(crate) fn command_finished(&mut self, app: App) {
        adjust(&mut self.in_flight, app, false);
    }

    pub(crate) fn forget_commands(&mut self) {
        self.in_flight.clear();
    }

    pub(crate) fn in_flight(&self, app: App) -> usize {
        self.in_flight.get(&app).copied().unwrap_or(0)
    }

    /// A protocol call whose page promise never settles stays in flight; after a few waits it no longer blocks the park.
    pub(crate) fn decide(&mut self, app: App) -> ParkDecision {
        if self.held(app) {
            return ParkDecision::Wait;
        }
        let waits = self.command_waits.entry(app).or_default();
        if self.in_flight.get(&app).copied().unwrap_or(0) > 0 && *waits < MAX_COMMAND_WAITS {
            *waits += 1;
            return ParkDecision::Wait;
        }
        *waits = 0;
        ParkDecision::Park
    }
}

fn adjust(counts: &mut HashMap<App, usize>, app: App, up: bool) {
    let count = counts.entry(app).or_default();
    *count = if up {
        *count + 1
    } else {
        count.saturating_sub(1)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_tab_parks() {
        assert_eq!(ParkGate::default().decide(App::Teams), ParkDecision::Park);
    }

    #[test]
    fn a_running_command_delays_the_park_until_it_answers() {
        let mut gate = ParkGate::default();
        gate.command_started(App::Teams);
        assert_eq!(gate.decide(App::Teams), ParkDecision::Wait);
        gate.command_finished(App::Teams);
        assert_eq!(gate.decide(App::Teams), ParkDecision::Park);
    }

    #[test]
    fn a_command_that_never_answers_stops_blocking_after_a_few_waits() {
        let mut gate = ParkGate::default();
        gate.command_started(App::Teams);
        for _ in 0..MAX_COMMAND_WAITS {
            assert_eq!(gate.decide(App::Teams), ParkDecision::Wait);
        }
        assert_eq!(gate.decide(App::Teams), ParkDecision::Park);
    }

    #[test]
    fn a_held_tab_waits_for_every_hold_to_end() {
        let mut gate = ParkGate::default();
        gate.hold(App::Teams, true);
        gate.hold(App::Teams, true);
        gate.hold(App::Teams, false);
        assert_eq!(gate.decide(App::Teams), ParkDecision::Wait);
        gate.hold(App::Teams, false);
        assert_eq!(gate.decide(App::Teams), ParkDecision::Park);
    }

    #[test]
    fn extra_releases_do_not_underflow() {
        let mut gate = ParkGate::default();
        gate.hold(App::Teams, false);
        gate.command_finished(App::Teams);
        gate.hold(App::Teams, true);
        assert_eq!(gate.decide(App::Teams), ParkDecision::Wait);
    }

    #[test]
    fn a_restart_forgets_unanswered_commands_but_keeps_holds() {
        let mut gate = ParkGate::default();
        gate.command_started(App::Teams);
        gate.forget_commands();
        assert_eq!(gate.decide(App::Teams), ParkDecision::Park);
        gate.hold(App::Teams, true);
        gate.forget_commands();
        assert_eq!(gate.decide(App::Teams), ParkDecision::Wait);
    }
}
