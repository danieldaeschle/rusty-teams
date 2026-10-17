use chatsvc::{Calendar, parse_calendar_events};
use chrono::{Duration, Utc};
use session::{DEFAULT_ENDPOINT, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let start = Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc();
    let answer = Calendar::new(&session)
        .view_response(start, start + Duration::days(7))
        .await
        .expect("calendar view");
    let events = parse_calendar_events(&answer.body);
    println!("# calendar view: HTTP {}", answer.status);
    println!("# events: {}", events.len());
    println!(
        "# with join url: {}",
        events
            .iter()
            .filter(|event| event.join_url.is_some())
            .count()
    );
    println!(
        "# online: {}",
        events.iter().filter(|event| event.online).count()
    );
    println!(
        "# with thread: {}",
        events
            .iter()
            .filter(|event| event.thread_id.is_some())
            .count()
    );
    println!(
        "# with organizer name: {}",
        events
            .iter()
            .filter(|event| event.organizer_name.is_some())
            .count()
    );
    println!(
        "# not responded: {}, cancelled: {}, all day: {}",
        events
            .iter()
            .filter(|event| event.response == chatsvc::ResponseType::NotResponded)
            .count(),
        events.iter().filter(|event| event.cancelled).count(),
        events.iter().filter(|event| event.all_day).count()
    );
}
