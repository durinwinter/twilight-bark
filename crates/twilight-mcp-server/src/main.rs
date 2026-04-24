use anyhow::Result;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    schemars, tool, tool_router, ServiceExt,
    transport::stdio,
};
use serde::Deserialize;
use std::sync::Arc;
use twilight_bus::TwilightBus;
use twilight_mcp_server::TwilightMcpServer;
use twilight_proto::twilight::{MessageTarget, TargetKind};
use twilight_traffic_controller::TrafficController;

#[derive(Clone)]
struct FabricHandler {
    inner: Arc<TwilightMcpServer>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PublishTaskParams {
    #[schemars(description = "Name of the operation to perform")]
    operation: String,
    #[schemars(description = "JSON-encoded input payload for the task")]
    input_json: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AskAgentParams {
    #[schemars(description = "UUID of the target agent")]
    agent_uuid: String,
    #[schemars(description = "Name of the operation to perform")]
    operation: String,
    #[schemars(description = "JSON-encoded input payload for the task")]
    input_json: String,
}

#[tool_router(server_handler)]
impl FabricHandler {
    #[tool(description = "List all agents currently registered in the Twilight Bark fabric")]
    fn get_registry(&self) -> String {
        let identities = self.inner.get_registry();
        serde_json::to_string(&identities).unwrap_or_else(|_| "[]".to_string())
    }

    #[tool(description = "Broadcast a task to all agents in the fabric. Returns the task_id.")]
    async fn publish_task(
        &self,
        Parameters(PublishTaskParams { operation, input_json }): Parameters<PublishTaskParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut target = MessageTarget::default();
        target.target_kind = TargetKind::Broadcast as i32;
        match self.inner.publish_task(&operation, &input_json, target).await {
            Ok(task_id) => Ok(CallToolResult::success(vec![Content::text(task_id)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!("error: {e}"))])),
        }
    }

    #[tool(description = "Send a task directly to a specific agent by UUID. Returns the task_id.")]
    async fn ask_agent(
        &self,
        Parameters(AskAgentParams { agent_uuid, operation, input_json }): Parameters<AskAgentParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.inner.ask_agent(&agent_uuid, &operation, &input_json).await {
            Ok(task_id) => Ok(CallToolResult::success(vec![Content::text(task_id)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!("error: {e}"))])),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // MCP protocol uses stdout for JSON-RPC; all logs must go to stderr.
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stderr)
        .init();

    let bus = Arc::new(TwilightBus::new("default", "local").await?);
    let controller = Arc::new(TrafficController::new());
    let inner = Arc::new(TwilightMcpServer::new(bus, controller));
    let handler = FabricHandler { inner };

    let service = handler.serve(stdio()).await.inspect_err(|e| {
        log::error!("MCP server error: {:?}", e);
    })?;

    service.waiting().await?;
    Ok(())
}
