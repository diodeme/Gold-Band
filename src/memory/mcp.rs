use camino::Utf8PathBuf;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    model::*,
    service::{RequestContext, RoleServer},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{MemoryService, WriteCommand};

pub const SERVER_NAME: &str = "gold-band-memory";
const FLAG: &str = "--gold-band-memory-mcp";

#[derive(Serialize, Deserialize)]
struct Binding {
    repo_root: Utf8PathBuf,
    data_root: Utf8PathBuf,
    project_id: String,
    task_id: String,
    language: crate::config::DesktopLanguage,
}

pub fn server_config(
    paths: &crate::storage::GoldBandPaths,
    task_id: &str,
    language: crate::config::DesktopLanguage,
) -> anyhow::Result<Value> {
    let binding = Binding {
        repo_root: paths.repo_root.clone(),
        data_root: paths.user_gold_band_root.clone(),
        project_id: paths.project_id.clone(),
        task_id: task_id.into(),
        language,
    };
    Ok(
        json!({"name": SERVER_NAME, "command": std::env::current_exe()?, "args": [FLAG, serde_json::to_string(&binding)?], "env": []}),
    )
}

pub fn requested() -> bool {
    std::env::args().nth(1).as_deref() == Some(FLAG)
}

pub async fn run() -> anyhow::Result<()> {
    let binding: Binding = serde_json::from_str(
        &std::env::args()
            .nth(2)
            .ok_or_else(|| anyhow::anyhow!("memory.locator"))?,
    )?;
    let mut paths = crate::storage::GoldBandPaths::new(binding.repo_root);
    // This launch configuration is supplied by the application, never by tool arguments.
    paths.user_gold_band_root = binding.data_root;
    paths.project_id = binding.project_id.clone();
    paths.runtime_root = paths
        .user_gold_band_root
        .join("projects")
        .join(&binding.project_id);
    let service = MemoryService::new(paths, &binding.project_id, Some(binding.task_id))?;
    MemoryMcp(service, binding.language)
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

#[derive(Clone)]
struct MemoryMcp(MemoryService, crate::config::DesktopLanguage);

fn tools(language: crate::config::DesktopLanguage) -> Vec<Tool> {
    let descriptions: Value = serde_json::from_str(match language {
        crate::config::DesktopLanguage::ZhCn => {
            include_str!("../prompts/zh-CN/runtime/memory-tools.json")
        }
        crate::config::DesktopLanguage::En => {
            include_str!("../prompts/en/runtime/memory-tools.json")
        }
    })
    .expect("bundled memory tool descriptions");
    let entry = json!({"type":"object", "additionalProperties":false, "required":["key","value","desc"], "properties":{
        "key":{"type":"string","maxLength":super::MAX_KEY_CHARS}, "value":{"type":"string","maxLength":super::MAX_VALUE_CHARS}, "desc":{"type":"string","maxLength":super::MAX_DESC_CHARS}
    }});
    vec![
        serde_json::from_value(json!({"name":"memory_read", "description":descriptions["read"], "inputSchema":{"type":"object","additionalProperties":false}})).unwrap(),
        serde_json::from_value(json!({"name":"memory_write", "description":descriptions["write"], "inputSchema":{"type":"object","additionalProperties":false,"required":["scope","key","expectedRevision","entry"],"properties":{
            "scope":{"type":"string","enum":["workspace","task"]}, "key":{"type":"string"}, "expectedRevision":{"type":["string","null"]}, "entry":{"anyOf":[entry,{"type":"null"}]}
        }}})).unwrap(),
    ]
}

impl ServerHandler for MemoryMcp {
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools(self.1).into_iter().find(|tool| tool.name == name)
    }
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools(self.1),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let service = self.0.clone();
        let result = tokio::task::spawn_blocking(move || match request.name.as_ref() {
            "memory_read" => service.read(),
            "memory_write" => {
                let command: WriteCommand =
                    serde_json::from_value(Value::Object(request.arguments.unwrap_or_default()))
                        .map_err(|_| super::error("memory.field", json!({})))?;
                service.write(command)
            }
            _ => Err(super::error("memory.tool", json!({}))),
        })
        .await
        .map_err(|_| ErrorData::internal_error("memory.worker", None))?;
        let output = match result {
            Ok(snapshot) => CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string(&snapshot).unwrap(),
            )]),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(
                serde_json::to_string(&error).unwrap(),
            )]),
        };
        Ok(output.into())
    }
}
