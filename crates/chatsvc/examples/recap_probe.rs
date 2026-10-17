use chatsvc::{Attendance, Recap, TranscriptError, TranscriptFormat};
use chrono::Utc;
use session::{DEFAULT_ENDPOINT, Session};

fn usage() -> ! {
    eprintln!("usage: recap_probe <thread-id-file> [--delete]");
    std::process::exit(2)
}

fn host_and_path_shape(url: &str) -> String {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let mut parts = without_scheme.split('?');
    let path = parts.next().unwrap_or_default();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let shape: Vec<&str> = segments
        .iter()
        .map(|segment| match *segment {
            "_api" | "v2.1" | "drives" | "items" | "personal" | ":v:" | "g" => segment,
            _ => "*",
        })
        .collect();
    let host_kind = without_scheme.split('/').next().map_or("?", |host| {
        if host.ends_with("-my.sharepoint.com") {
            "<tenant>-my.sharepoint.com"
        } else {
            "other"
        }
    });
    format!("{host_kind}/{}", shape.join("/"))
}

fn error_shape(error: &TranscriptError) -> String {
    match error {
        TranscriptError::Request(session::Error::Api { status, body, .. }) => format!(
            "HTTP {status} code {}",
            body.pointer("/error/code")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("-")
        ),
        TranscriptError::Request(other) => format!(
            "request {}",
            other.to_string().chars().take(60).collect::<String>()
        ),
        other => format!("{other:?}"),
    }
}

fn magic(bytes: &[u8]) -> String {
    let start = bytes
        .strip_prefix(b"\xEF\xBB\xBF".as_slice())
        .unwrap_or(bytes);
    if start.starts_with(b"WEBVTT") {
        format!("WEBVTT (bom {})", start.len() != bytes.len())
    } else if bytes.starts_with(b"PK\x03\x04") {
        "zip PK".to_owned()
    } else {
        format!("unknown {:02x?}", &bytes[..bytes.len().min(4)])
    }
}

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(thread_file) = arguments.next() else {
        usage()
    };
    let delete = arguments.any(|argument| argument == "--delete");
    let thread_id = std::fs::read_to_string(&thread_file)
        .expect("thread file")
        .trim()
        .to_owned();
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let recap = Recap::new(&session);

    let occurrences = match recap.occurrences(&thread_id).await {
        Ok(occurrences) => occurrences,
        Err(error) => {
            println!("# occurrences: error {}", error_shape(&error));
            return;
        }
    };
    println!("# occurrences: {}", occurrences.len());
    for occurrence in &occurrences {
        println!(
            "#   started {}, name {}, duration {:?}, organizer {}, url {}",
            occurrence.started_at.is_some(),
            occurrence.recording_name.is_some(),
            occurrence.duration_seconds,
            occurrence.organizer_id.is_some(),
            host_and_path_shape(&occurrence.recording_url)
        );
    }
    let Some(occurrence) = occurrences.first() else {
        return;
    };

    let details = match recap.details(occurrence).await {
        Ok(details) => details,
        Err(error) => {
            println!("# details: error {}", error_shape(&error));
            return;
        }
    };
    println!(
        "# details: audio only {}, duration {:?}, expires in days {:?}, transcript {}, item {}",
        details.audio_only,
        details.duration_seconds,
        details
            .expires_at
            .map(|expires_at| chatsvc::recap::days_until_expiry(expires_at, Utc::now())),
        details.transcript_id.is_some(),
        host_and_path_shape(&details.item_url)
    );

    match recap.transcript(&details).await {
        Ok(turns) => println!(
            "# transcript: {} turns, {} with speaker, {} words, spans {:?}",
            turns.len(),
            turns.iter().filter(|turn| !turn.speaker.is_empty()).count(),
            turns
                .iter()
                .map(|turn| turn.text.split_whitespace().count())
                .sum::<usize>(),
            turns
                .iter()
                .map(|turn| (turn.start_seconds.round(), turn.end_seconds.round()))
                .collect::<Vec<_>>()
        ),
        Err(error) => println!("# transcript: error {}", error_shape(&error)),
    }

    let directory = std::env::temp_dir().join(format!("recap-probe-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp dir");
    for format in [TranscriptFormat::Vtt, TranscriptFormat::Docx] {
        match recap.transcript_file(&details, format).await {
            Ok(bytes) => {
                let path = directory.join(format!("transcript.{}", format.query()));
                std::fs::write(&path, &bytes).expect("write transcript");
                let text = String::from_utf8_lossy(&bytes);
                println!(
                    "# transcript file {}: {} bytes, magic {}, cues {}, voice tags {}, word document part {}",
                    format.query(),
                    bytes.len(),
                    magic(&bytes),
                    text.matches("-->").count(),
                    text.matches("<v ").count(),
                    text.contains("word/document.xml")
                );
            }
            Err(error) => println!(
                "# transcript file {}: error {}",
                format.query(),
                error_shape(&error)
            ),
        }
    }
    let _ = std::fs::remove_dir_all(&directory);

    match recap.attendance(&thread_id).await {
        Ok(Attendance::TurnedOff) => println!("# attendance: turned off"),
        Ok(Attendance::Report(records)) => println!("# attendance: {} records", records.len()),
        Err(error) => println!("# attendance: error {}", error_shape(&error)),
    }

    if !delete {
        return;
    }
    match recap.delete_recording(&details).await {
        Ok(()) => println!("# delete: ok"),
        Err(error) => println!("# delete: error {}", error_shape(&error)),
    }
    match recap.occurrences(&thread_id).await {
        Ok(after) => println!("# occurrences after delete: {}", after.len()),
        Err(error) => println!("# occurrences after delete: error {}", error_shape(&error)),
    }
    match recap.details(occurrence).await {
        Ok(_) => println!("# details after delete: still found"),
        Err(error) => println!("# details after delete: {}", error_shape(&error)),
    }
}
