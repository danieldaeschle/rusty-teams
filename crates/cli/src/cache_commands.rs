use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use graph::Graph;
use store::{Sidebar, Store};
use teams_core::{SyncConfig, SyncEngine};

use crate::BoxedResult;

fn open_store(database: Option<PathBuf>) -> BoxedResult<Arc<Store>> {
    let store = match database {
        Some(path) => Store::open(&path)?,
        None => Store::open_default()?,
    };
    Ok(Arc::new(store))
}

fn counts(sidebar: &Sidebar) -> (usize, usize, usize) {
    let channels = sidebar.teams.iter().map(|entry| entry.channels.len()).sum();
    (sidebar.chats.len(), sidebar.teams.len(), channels)
}

pub async fn sync(graph: Graph, database: Option<PathBuf>, chat_limit: usize) -> BoxedResult<()> {
    let config = SyncConfig {
        chat_limit,
        ..SyncConfig::default()
    };
    let engine = SyncEngine::with_config(graph, open_store(database)?, config);

    let started = Instant::now();
    let summary = engine.refresh_sidebar().await?;
    println!(
        "network refresh: {} chats in {} pages (full: {}), teams refreshed: {}, {} teams, {} channels ({} teams failed) in {} ms",
        summary.chats,
        summary.chat_pages,
        summary.full_chat_refresh,
        summary.teams_refreshed,
        summary.teams,
        summary.channels,
        summary.failed_teams,
        started.elapsed().as_millis()
    );

    let started = Instant::now();
    let sidebar = engine.sidebar()?;
    let elapsed = started.elapsed();
    let (chats, teams, channels) = counts(&sidebar);
    println!(
        "cache read:      {chats} chats, {teams} teams, {channels} channels in {:.2} ms",
        elapsed.as_secs_f64() * 1000.0
    );
    let unread = sidebar.chats.iter().filter(|chat| chat.unread).count();
    println!("unread chats: {unread}");
    Ok(())
}

struct Target {
    id: String,
    title: String,
}

fn find_target(sidebar: &Sidebar, needle: &str) -> Vec<Target> {
    let needle = needle.to_lowercase();
    let chats = sidebar.chats.iter().map(|chat| Target {
        id: chat.id.clone(),
        title: chat.title.clone(),
    });
    let channels = sidebar.teams.iter().flat_map(|entry| {
        entry.channels.iter().map(|channel| Target {
            id: channel.id.clone(),
            title: format!("{} / {}", entry.team.name, channel.name),
        })
    });
    chats
        .chain(channels)
        .filter(|target| target.title.to_lowercase().contains(&needle))
        .collect()
}

pub async fn open(graph: Graph, database: Option<PathBuf>, title: &str) -> BoxedResult<()> {
    let engine = SyncEngine::new(graph, open_store(database)?);
    if engine.sidebar()?.chats.is_empty() {
        engine.refresh_sidebar().await?;
    }
    let sidebar = engine.sidebar()?;
    let matches = find_target(&sidebar, title);
    let Some(target) = matches.first() else {
        return Err(format!("no chat or channel title contains {title:?}").into());
    };
    if matches.len() > 1 {
        eprintln!(
            "{} matches, using the first: {}",
            matches.len(),
            target.title
        );
    }

    let started = Instant::now();
    let cached = engine.open_conversation(&target.id)?;
    println!(
        "{}: {} cached messages in {:.2} ms",
        target.title,
        cached.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );

    let started = Instant::now();
    let delta = engine.fetch_newer(&target.id).await?;
    println!(
        "network delta: {} new, {} changed in {} ms",
        delta.added.len(),
        delta.updated.len(),
        started.elapsed().as_millis()
    );
    println!("cached now: {}", engine.store().message_count(&target.id)?);
    Ok(())
}
