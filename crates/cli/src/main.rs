mod cache_commands;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use graph::Graph;
use session::{DEFAULT_ENDPOINT, Session};

type BoxedResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(
    name = "teams-probe",
    about = "Read-only end-to-end check of the Teams session, Graph and cache layers"
)]
struct Arguments {
    #[arg(long, default_value = DEFAULT_ENDPOINT)]
    endpoint: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Me,
    Chats {
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Refresh chats, teams and channels into the cache, then read them back from it.
    Sync {
        #[arg(long)]
        database: Option<PathBuf>,
        #[arg(long, default_value_t = 200)]
        chat_limit: usize,
    },
    /// Open a chat or channel by title substring and print counts and timings only.
    Open {
        title: String,
        #[arg(long)]
        database: Option<PathBuf>,
    },
}

async fn run(arguments: Arguments) -> BoxedResult<()> {
    let started = Instant::now();
    let graph = Graph::new(Session::connect(&arguments.endpoint).await?);
    match arguments.command {
        Command::Me => {
            let me = graph.me().await?;
            println!("{} ({})", me.display_name.as_deref().unwrap_or("?"), me.id);
        }
        Command::Chats { limit } => {
            let me = graph.me().await?;
            for chat in graph.list_chats(limit).await? {
                let last = chat
                    .last_message_time()
                    .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_else(|| "-".to_owned());
                println!("{last}  {}", chat.title(&me.id));
            }
        }
        Command::Sync {
            database,
            chat_limit,
        } => cache_commands::sync(graph, database, chat_limit).await?,
        Command::Open { title, database } => cache_commands::open(graph, database, &title).await?,
    }
    eprintln!("done in {} ms", started.elapsed().as_millis());
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Arguments::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
