use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, SeqAccess, Visitor};

const TEXT_KEYS: [&str; 4] = ["title", "text", "subtitle", "value"];

enum Node {
    Object(Vec<(String, Node)>),
    List(Vec<Node>),
    Text(String),
    Other,
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeVisitor;

        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_str<E>(self, value: &str) -> Result<Node, E> {
                Ok(Node::Text(value.to_owned()))
            }

            fn visit_bool<E>(self, _: bool) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_i64<E>(self, _: i64) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_u64<E>(self, _: u64) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_f64<E>(self, _: f64) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_unit<E>(self) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Node, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = sequence.next_element()? {
                    items.push(item);
                }
                Ok(Node::List(items))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Node::Object(entries))
            }
        }

        deserializer.deserialize_any(NodeVisitor)
    }
}

/// Adaptive Card text: own text keys first, then nested values in document order.
fn collect(node: &Node, lines: &mut Vec<String>) {
    match node {
        Node::List(items) => items.iter().for_each(|item| collect(item, lines)),
        Node::Object(entries) => {
            for key in TEXT_KEYS {
                let found = entries.iter().find(|(name, _)| name == key);
                if let Some((_, Node::Text(text))) = found
                    && !text.trim().is_empty()
                {
                    lines.push(text.clone());
                }
            }
            for (_, value) in entries {
                if matches!(value, Node::Object(_) | Node::List(_)) {
                    collect(value, lines);
                }
            }
        }
        Node::Text(_) | Node::Other => {}
    }
}

pub fn card_content_text(content: &str) -> Option<String> {
    let parsed: Node = serde_json::from_str(content).ok()?;
    let mut lines = Vec::new();
    collect(&parsed, &mut lines);
    (!lines.is_empty()).then(|| lines.join("\n"))
}
