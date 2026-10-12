use std::fmt;
use std::path::Path;

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

use super::entry::{SERVER_NAME, ServerEntry};
use super::error::InstallError;
use super::file;

struct OrderedObject(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for OrderedObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = OrderedObject;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, Box<RawValue>>()? {
                    entries.push(entry);
                }
                Ok(OrderedObject(entries))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

fn indentation(depth: usize) -> String {
    "  ".repeat(depth)
}

impl OrderedObject {
    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        let trimmed = text.trim_start_matches('\u{feff}').trim();
        if trimmed.is_empty() {
            return Ok(OrderedObject(Vec::new()));
        }
        serde_json::from_str(trimmed)
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.0.iter().position(|(existing, _)| existing == key)
    }

    fn get(&self, key: &str) -> Option<&RawValue> {
        self.position(key).map(|index| self.0[index].1.as_ref())
    }

    fn set(&mut self, key: &str, value: Box<RawValue>) {
        match self.position(key) {
            Some(index) => self.0[index].1 = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    fn remove(&mut self, key: &str) -> bool {
        match self.position(key) {
            Some(index) => {
                self.0.remove(index);
                true
            }
            None => false,
        }
    }

    fn render(&self, depth: usize) -> String {
        if self.0.is_empty() {
            return "{}".to_string();
        }
        let entries: Vec<String> = self
            .0
            .iter()
            .map(|(key, value)| {
                let key = serde_json::to_string(key).expect("string serializes");
                format!("{}{key}: {}", indentation(depth + 1), value.get())
            })
            .collect();
        format!("{{\n{}\n{}}}", entries.join(",\n"), indentation(depth))
    }
}

fn raw_at_depth(value: &impl Serialize, depth: usize) -> Box<RawValue> {
    let pretty = serde_json::to_string_pretty(value).expect("value serializes");
    let indented = pretty.replace('\n', &format!("\n{}", indentation(depth)));
    RawValue::from_string(indented).expect("pretty json is valid")
}

fn load(path: &Path) -> Result<Option<OrderedObject>, InstallError> {
    match file::read_if_exists(path)? {
        Some(text) => OrderedObject::parse(&text)
            .map(Some)
            .map_err(|error| InstallError::malformed(path, error)),
        None => Ok(None),
    }
}

fn container_of(
    root: &OrderedObject,
    container: &str,
    path: &Path,
) -> Result<OrderedObject, InstallError> {
    match root.get(container) {
        Some(raw) => serde_json::from_str(raw.get())
            .map_err(|_| InstallError::malformed(path, format!("`{container}` is not an object"))),
        None => Ok(OrderedObject(Vec::new())),
    }
}

pub(super) fn read_entry(
    path: &Path,
    container: &str,
) -> Result<Option<ServerEntry>, InstallError> {
    let Some(root) = load(path)? else {
        return Ok(None);
    };
    let servers = container_of(&root, container, path)?;
    Ok(servers
        .get(SERVER_NAME)
        .and_then(|raw| serde_json::from_str::<Value>(raw.get()).ok())
        .and_then(|value| ServerEntry::from_json(&value)))
}

pub(super) fn upsert(path: &Path, container: &str, entry: &Value) -> Result<(), InstallError> {
    let mut root = load(path)?.unwrap_or(OrderedObject(Vec::new()));
    let mut servers = container_of(&root, container, path)?;
    servers.set(SERVER_NAME, raw_at_depth(entry, 2));
    let rendered = servers.render(1);
    root.set(
        container,
        RawValue::from_string(rendered).expect("rendered object is valid json"),
    );
    file::write_atomic(path, &format!("{}\n", root.render(0)))
}

pub(super) fn remove(path: &Path, container: &str) -> Result<(), InstallError> {
    let Some(mut root) = load(path)? else {
        return Ok(());
    };
    let mut servers = container_of(&root, container, path)?;
    if !servers.remove(SERVER_NAME) {
        return Ok(());
    }
    let rendered = servers.render(1);
    root.set(
        container,
        RawValue::from_string(rendered).expect("rendered object is valid json"),
    );
    file::write_atomic(path, &format!("{}\n", root.render(0)))
}
