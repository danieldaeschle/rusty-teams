use std::collections::HashMap;

use session::{App, TabEvent};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Default)]
pub(crate) struct Subscribers {
    sinks: HashMap<App, Vec<UnboundedSender<TabEvent>>>,
}

impl Subscribers {
    pub(crate) fn add(&mut self, app: App, sink: UnboundedSender<TabEvent>) {
        self.sinks.entry(app).or_default().push(sink);
    }

    pub(crate) fn publish(&mut self, app: App, event: &TabEvent) {
        if let Some(sinks) = self.sinks.get_mut(&app) {
            sinks.retain(|sink| sink.send(event.clone()).is_ok());
        }
    }

    /// Ends every event stream, so subscribers attach again to the new webviews.
    pub(crate) fn clear(&mut self) {
        self.sinks.clear();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;

    fn event() -> TabEvent {
        TabEvent {
            method: "Page.frameNavigated".into(),
            params: json!({}),
        }
    }

    #[test]
    fn publishes_to_the_app_only_and_drops_closed_sinks() {
        let mut subscribers = Subscribers::default();
        let (teams, mut teams_events) = unbounded_channel();
        let (closed, closed_events) = unbounded_channel();
        let (outlook, mut outlook_events) = unbounded_channel();
        subscribers.add(App::Teams, teams);
        subscribers.add(App::Teams, closed);
        subscribers.add(App::Outlook, outlook);
        drop(closed_events);
        subscribers.publish(App::Teams, &event());
        assert_eq!(teams_events.try_recv().unwrap(), event());
        assert!(outlook_events.try_recv().is_err());
        assert_eq!(subscribers.sinks[&App::Teams].len(), 1);
    }

    #[test]
    fn clear_ends_the_streams() {
        let mut subscribers = Subscribers::default();
        let (teams, mut teams_events) = unbounded_channel();
        subscribers.add(App::Teams, teams);
        subscribers.clear();
        assert!(teams_events.try_recv().is_err());
        assert!(teams_events.is_closed());
    }
}
