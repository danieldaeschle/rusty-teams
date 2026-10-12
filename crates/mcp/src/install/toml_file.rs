use std::path::Path;

use toml_edit::{DocumentMut, InlineTable, Item, Table, value};

use super::entry::{SERVER_NAME, ServerEntry};
use super::error::InstallError;
use super::file;

const SERVERS_KEY: &str = "mcp_servers";
const TOOL_TIMEOUT_SECONDS: i64 = 300;

fn load(path: &Path) -> Result<Option<DocumentMut>, InstallError> {
    match file::read_if_exists(path)? {
        Some(text) => text
            .parse::<DocumentMut>()
            .map(Some)
            .map_err(|error| InstallError::malformed(path, error)),
        None => Ok(None),
    }
}

fn entry_of(document: &DocumentMut) -> Option<ServerEntry> {
    let server = document.get(SERVERS_KEY)?.get(SERVER_NAME)?;
    Some(ServerEntry {
        url: server.get("url")?.as_str()?.to_string(),
        authorization: server
            .get("http_headers")?
            .get("Authorization")?
            .as_str()?
            .to_string(),
    })
}

pub(super) fn read_entry(path: &Path) -> Result<Option<ServerEntry>, InstallError> {
    Ok(load(path)?.and_then(|document| entry_of(&document)))
}

pub(super) fn upsert(path: &Path, entry: &ServerEntry) -> Result<(), InstallError> {
    let mut document = load(path)?.unwrap_or_default();
    if entry_of(&document).as_ref() != Some(entry) {
        let server = server_table(&mut document, path)?;
        server["url"] = value(entry.url.as_str());
        let mut headers = InlineTable::new();
        headers.insert("Authorization", entry.authorization.as_str().into());
        server["http_headers"] = value(headers);
    }
    let server = server_table(&mut document, path)?;
    if !server.contains_key("tool_timeout_sec") {
        server["tool_timeout_sec"] = value(TOOL_TIMEOUT_SECONDS);
    }
    file::write_atomic(path, &document.to_string())
}

fn server_table<'document>(
    document: &'document mut DocumentMut,
    path: &Path,
) -> Result<&'document mut Table, InstallError> {
    let servers = document
        .entry(SERVERS_KEY)
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_mut()
        .ok_or_else(|| InstallError::malformed(path, format!("`{SERVERS_KEY}` is not a table")))?;
    servers
        .entry(SERVER_NAME)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| InstallError::malformed(path, format!("`{SERVER_NAME}` is not a table")))
}

pub(super) fn remove(path: &Path) -> Result<(), InstallError> {
    let Some(mut document) = load(path)? else {
        return Ok(());
    };
    let removed = document
        .get_mut(SERVERS_KEY)
        .and_then(Item::as_table_like_mut)
        .and_then(|servers| servers.remove(SERVER_NAME))
        .is_some();
    if removed {
        file::write_atomic(path, &document.to_string())?;
    }
    Ok(())
}
