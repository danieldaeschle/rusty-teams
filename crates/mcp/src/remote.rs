use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Map, Value};

use crate::McpEndpoint;

#[derive(Clone, Debug, PartialEq)]
pub struct ListedTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub struct ToolSession {
    client: rmcp::service::RunningService<rmcp::RoleClient, ClientConfig>,
}

impl ToolSession {
    pub async fn connect(endpoint: &McpEndpoint) -> Result<Self, String> {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.url.clone())
                .auth_header(endpoint.token.clone()),
        );
        let client = ClientConfig::default()
            .serve(transport)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Self { client })
    }

    pub async fn tools(&self) -> Result<Vec<ListedTool>, String> {
        let tools = self
            .client
            .list_all_tools()
            .await
            .map_err(|error| error.to_string())?;
        Ok(tools
            .into_iter()
            .map(|tool| ListedTool {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                parameters: Value::Object(tool.input_schema.as_ref().clone()),
            })
            .collect())
    }

    pub async fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
        let parsed: Value = serde_json::from_str(arguments).unwrap_or(Value::Object(Map::new()));
        let arguments = parsed.as_object().cloned().unwrap_or_default();
        let params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        let result = self
            .client
            .call_tool(params)
            .await
            .map_err(|error| error.to_string())?;
        let text = result
            .content
            .iter()
            .filter_map(|content| content.as_text().map(|text| text.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        if result.is_error == Some(true) {
            return Err(if text.is_empty() {
                format!("{name} failed")
            } else {
                text
            });
        }
        Ok(text)
    }
}
