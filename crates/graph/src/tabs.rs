use serde_json::Value;
use session::Scope;

use crate::client::Graph;
use crate::error::Result;
use crate::urls;

const TABS_SCOPE: &str = "TeamsTab.ReadWrite.All";

fn web_url(tab: &Value) -> Option<String> {
    tab.get("webUrl")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://"))
        .map(str::to_owned)
}

impl Graph {
    pub async fn channel_tab_web_url(
        &self,
        team_id: &str,
        channel_id: &str,
        tab_id: &str,
    ) -> Result<Option<String>> {
        let tab = self
            .get(
                &urls::channel_tab(team_id, channel_id, tab_id),
                &Scope::graph(TABS_SCOPE),
            )
            .await?;
        Ok(web_url(&tab))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_tab_deep_link_is_its_web_url() {
        let tab = json!({"id": "t1", "displayName": "Plan", "webUrl": "https://teams.microsoft.com/l/channel/19%3Ac/tab%3A%3At1?groupId=g&tenantId=t"});
        assert_eq!(
            web_url(&tab).as_deref(),
            Some("https://teams.microsoft.com/l/channel/19%3Ac/tab%3A%3At1?groupId=g&tenantId=t")
        );
    }

    #[test]
    fn a_tab_without_a_usable_web_url_has_none() {
        assert_eq!(web_url(&json!({"id": "t1"})), None);
        assert_eq!(web_url(&json!({"webUrl": "javascript:alert(1)"})), None);
    }

    #[test]
    fn the_tab_url_names_team_channel_and_tab() {
        assert_eq!(
            urls::channel_tab("team", "19:c@thread.tacv2", "tab-1"),
            "https://graph.microsoft.com/v1.0/teams/team/channels/19%3Ac%40thread.tacv2/tabs/tab-1"
        );
    }
}
