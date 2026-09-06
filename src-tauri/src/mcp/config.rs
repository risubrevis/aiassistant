use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Transport spec for one MCP server (docs/06): either stdio (`command`+`args`+`env`)
/// or HTTP (`url`+`headers`). Claude Desktop-compatible shape, stored in the DB.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}
