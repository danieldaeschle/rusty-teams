use std::borrow::Cow;

use crate::app::App;

pub const GRAPH: &str = "https://graph.microsoft.com";
pub const IC3: &str = "https://ic3.teams.office.com";
pub const PRESENCE: &str = "https://presence.teams.microsoft.com";
pub const OUTLOOK: &str = "https://outlook.office.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub resource: Cow<'static, str>,
    pub name: Cow<'static, str>,
}

impl Scope {
    pub fn new(resource: impl Into<Cow<'static, str>>, name: impl Into<Cow<'static, str>>) -> Self {
        Scope {
            resource: resource.into(),
            name: name.into(),
        }
    }

    pub fn graph(name: impl Into<Cow<'static, str>>) -> Self {
        Scope::new(GRAPH, name)
    }

    /// Tabs to try, best first. The other tab is the fallback, as in the Python client.
    pub fn tab_order(&self) -> [App; 2] {
        if self.resource == GRAPH && self.name.starts_with("Chat.") {
            [App::Outlook, App::Teams]
        } else {
            [App::Teams, App::Outlook]
        }
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
    fn chat_scopes_prefer_outlook() {
        assert_eq!(Scope::graph("Chat.Read").tab_order(), [App::Outlook, App::Teams]);
        assert_eq!(Scope::graph("Chat.ReadWrite").tab_order(), [App::Outlook, App::Teams]);
    }

    #[test]
    fn other_scopes_prefer_teams() {
        assert_eq!(Scope::graph("ChannelMessage.Read.All").tab_order(), [App::Teams, App::Outlook]);
        assert_eq!(Scope::new(IC3, "Teams.AccessAsUser.All").tab_order(), [App::Teams, App::Outlook]);
        assert_eq!(Scope::new(PRESENCE, "Chat.Read").tab_order(), [App::Teams, App::Outlook]);
    }

    #[test]
    fn displays_resource_and_name() {
        assert_eq!(Scope::graph("Chat.Read").to_string(), "https://graph.microsoft.com/Chat.Read");
    }
}
