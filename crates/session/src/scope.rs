use std::borrow::Cow;

pub const GRAPH: &str = "https://graph.microsoft.com";
pub const IC3: &str = "https://ic3.teams.office.com";
pub const PRESENCE: &str = "https://presence.teams.microsoft.com";
pub const SPACES: &str = "https://api.spaces.skype.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub resource: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub nested_app: Option<NestedApp>,
}

/// An app hosted inside the tab that gets its tokens through the tab's own sign-in (nested app auth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestedApp {
    pub client_id: &'static str,
    pub redirect_uri: &'static str,
    pub broker_redirect_uri: &'static str,
}

impl Scope {
    pub fn new(resource: impl Into<Cow<'static, str>>, name: impl Into<Cow<'static, str>>) -> Self {
        Scope {
            resource: resource.into(),
            name: name.into(),
            nested_app: None,
        }
    }

    pub fn for_nested_app(mut self, app: NestedApp) -> Self {
        self.nested_app = Some(app);
        self
    }

    pub(crate) fn nested_app_json(&self) -> serde_json::Value {
        match self.nested_app {
            Some(app) => serde_json::json!({
                "clientId": app.client_id,
                "redirectUri": app.redirect_uri,
                "brokerRedirectUri": app.broker_redirect_uri,
            }),
            None => serde_json::Value::Null,
        }
    }

    pub fn graph(name: impl Into<Cow<'static, str>>) -> Self {
        Scope::new(GRAPH, name)
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}/{}", self.resource, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_resource_and_name() {
        assert_eq!(
            Scope::graph("Chat.Read").to_string(),
            "https://graph.microsoft.com/Chat.Read"
        );
    }
}
