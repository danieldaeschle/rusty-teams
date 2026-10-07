use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use session::Scope;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
    pub(crate) scope: Scope,
}

#[derive(Deserialize)]
struct RawPage<T> {
    #[serde(default = "Vec::new")]
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

impl<T: DeserializeOwned> Page<T> {
    pub(crate) fn parse(body: Value, scope: Scope) -> Result<Self> {
        let raw: RawPage<T> = serde_json::from_value(body)?;
        Ok(Page {
            items: raw.value,
            next_link: raw.next_link,
            delta_link: raw.delta_link,
            scope,
        })
    }
}
