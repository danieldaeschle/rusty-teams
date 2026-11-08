//! Creates a "Native client test" Meet now meeting and prints its join link (call end-to-end test).
use session::{DEFAULT_ENDPOINT, Session};

const SUBJECT: &str = "Native client test";

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    match chatsvc::Calendar::new(&session).create_meet_now(SUBJECT).await {
        Ok(link) => println!("{link}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
