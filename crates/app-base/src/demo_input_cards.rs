use serde_json::{Value, json};
use teams_core::InputChoice;

const DATASET_SERVICES: &str = "demo.services";

const SERVICES: [(&str, &str); 10] = [
    ("Billing API", "billing-api"),
    ("Build Farm", "build-farm"),
    ("Chat Gateway", "chat-gateway"),
    ("Identity Provider", "identity"),
    ("Notification Hub", "notification-hub"),
    ("Search Index", "search-index"),
    ("Storage Broker", "storage-broker"),
    ("Task Scheduler", "task-scheduler"),
    ("Telemetry Collector", "telemetry"),
    ("Wiki Renderer", "wiki-renderer"),
];

pub fn search(dataset: &str, query_text: &str) -> Vec<InputChoice> {
    if dataset != DATASET_SERVICES {
        return Vec::new();
    }
    let needle = query_text.to_lowercase();
    SERVICES
        .iter()
        .filter(|(title, _)| title.to_lowercase().contains(&needle))
        .map(|(title, value)| InputChoice {
            title: (*title).to_owned(),
            value: (*value).to_owned(),
        })
        .collect()
}

pub fn all_inputs_card() -> Value {
    json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "size": "medium", "weight": "bolder", "text": "Maintenance window"},
            {"type": "Input.Date", "id": "window_day", "label": "Day", "isRequired": true, "errorMessage": "Pick a day in the planning range", "min": "2026-10-01", "max": "2026-12-31", "value": "2026-11-02"},
            {"type": "Input.Time", "id": "window_start", "label": "Start (24h)", "isRequired": true, "errorMessage": "Start between 06:00 and 20:00", "min": "06:00", "max": "20:00", "value": "09:30"},
            {"type": "Input.Text", "id": "ticket", "label": "Ticket", "placeholder": "ABC-1234", "maxLength": 8, "regex": "^[A-Z]{3}-\\d{1,4}$", "isRequired": true, "errorMessage": "Use the form ABC-1234",
                "inlineAction": {"type": "Action.Submit", "title": "Look up", "data": {"action": "lookup"}}},
            {"type": "Input.Number", "id": "servers", "label": "Servers", "placeholder": "1-20", "min": 1, "max": 20, "isRequired": true, "errorMessage": "Between 1 and 20 servers", "value": 4},
            {"type": "Input.ChoiceSet", "id": "service", "label": "Service", "placeholder": "Type to search services", "isRequired": true, "errorMessage": "Pick a service",
                "choices.data": {"type": "Data.Query", "dataset": DATASET_SERVICES, "count": 5}},
            {"type": "Input.Rating", "id": "risk", "label": "Risk", "max": 5, "value": 2.5, "allowHalf": true, "color": "marigold", "size": "large", "isRequired": true, "errorMessage": "Rate the risk"},
            {"type": "Input.Toggle", "id": "notify", "title": "Notify everyone who uses the service and wait for the confirmation before the window starts", "wrap": true, "value": "true"},
            {"type": "Rating", "value": 4.5, "count": 128, "max": 5},
            {"type": "Rating", "value": 3.7, "count": 1204, "style": "compact"}
        ],
        "actions": [
            {"type": "Action.Submit", "title": "Schedule", "data": {"action": "schedule"}}
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_filters_the_local_list_case_insensitively() {
        let values: Vec<String> = search(DATASET_SERVICES, "CH")
            .into_iter()
            .map(|choice| choice.value)
            .collect();
        assert_eq!(values, ["chat-gateway", "search-index", "task-scheduler"]);
        assert!(search("other", "ch").is_empty());
    }
}
