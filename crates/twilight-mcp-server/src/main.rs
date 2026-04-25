use anyhow::Result;
use axum::{
    extract::{Path, Request, State},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use clap::Parser;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    schemars, tool, tool_router, ServiceExt as RmcpServiceExt,
    transport::{
        stdio,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
    },
};
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tower::util::ServiceExt;
use twilight_mcp_server::DaemonClient;

#[derive(Parser)]
#[command(about = "Twilight Bark MCP Server — connects an LLM to the agent fabric via daemon")]
struct Args {
    #[arg(long, env = "TWILIGHT_AGENT_NAME", default_value = "mcp-agent")]
    name: String,
    #[arg(long, env = "TWILIGHT_PORT")]
    port: Option<u16>,
    /// Unix socket path of the running twilight-daemon.
    #[arg(long, env = "TWILIGHT_DAEMON_SOCKET")]
    socket: Option<PathBuf>,
}

fn resolve_socket(arg: Option<PathBuf>) -> PathBuf {
    arg.unwrap_or_else(twilight_core::default_socket_path)
}

// ── MCP handler ───────────────────────────────────────────────────��──────────

#[derive(Clone)]
struct FabricHandler {
    client: Arc<DaemonClient>,
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReplyTaskParams {
    #[schemars(description = "Task ID from the incoming task_request event")]
    task_id: String,
    #[schemars(description = "JSON-encoded output payload for the reply")]
    output_json: String,
    #[schemars(description = "Whether the task succeeded")]
    success: bool,
}

#[tool_router(server_handler)]
impl FabricHandler {
    #[tool(description = "List all agents currently registered in the Twilight Bark fabric")]
    async fn get_registry(&self) -> String {
        match self.client.get_registry().await {
            Ok(v) => v.to_string(),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    #[tool(description = "Broadcast a task to all agents in the fabric. Returns the task_id.")]
    async fn publish_task(
        &self,
        Parameters(PublishTaskParams { operation, input_json }): Parameters<PublishTaskParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.client.publish_task(&operation, &input_json).await {
            Ok(id) => Ok(CallToolResult::success(vec![Content::text(id)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!("error: {e}"))])),
        }
    }

    #[tool(description = "Send a task directly to a specific agent by UUID. Returns the task_id.")]
    async fn ask_agent(
        &self,
        Parameters(AskAgentParams { agent_uuid, operation, input_json }): Parameters<AskAgentParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.client.ask_agent(&agent_uuid, &operation, &input_json).await {
            Ok(id) => Ok(CallToolResult::success(vec![Content::text(id)])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!("error: {e}"))])),
        }
    }

    #[tool(description = "Drain and return all pending incoming task events from the fabric (task_request and task_result). Returns a JSON array. Call this to check if any agents have sent you tasks or replies.")]
    async fn get_pending_tasks(&self) -> String {
        let tasks = self.client.get_pending_tasks().await;
        serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string())
    }

    #[tool(description = "Reply to an incoming task request. Use the task_id from a task_request event returned by get_pending_tasks.")]
    async fn reply_task(
        &self,
        Parameters(ReplyTaskParams { task_id, output_json, success }): Parameters<ReplyTaskParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.client.reply_task(&task_id, &output_json, success).await {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text("reply sent")])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!("error: {e}"))])),
        }
    }
}

// ── HTTP multiplexer gateway (Antigravity / multi-agent HTTP mode) ────────────

#[derive(Clone)]
struct GatewayState {
    socket: PathBuf,
    routers: Arc<RwLock<HashMap<String, Router>>>,
    ct: tokio_util::sync::CancellationToken,
}

async fn gateway_handler(
    Path((agent_name, rest)): Path<(String, String)>,
    State(state): State<GatewayState>,
    mut req: Request,
) -> Response {
    let router = {
        let mut map = state.routers.write().await;
        if let Some(r) = map.get(&agent_name) {
            r.clone()
        } else {
            match DaemonClient::connect(&state.socket, &agent_name, "mcp-agent").await {
                Ok(client) => {
                    let client = Arc::new(client);
                    let svc = StreamableHttpService::new(
                        move || Ok(FabricHandler { client: Arc::clone(&client) }),
                        LocalSessionManager::default().into(),
                        StreamableHttpServerConfig::default()
                            .with_cancellation_token(state.ct.child_token()),
                    );
                    let r = Router::new().nest_service("/mcp", svc);
                    map.insert(agent_name.clone(), r.clone());
                    r
                }
                Err(e) => {
                    log::error!("Cannot connect to daemon for agent '{agent_name}': {e}");
                    return (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        format!("daemon unavailable: {e}"),
                    )
                        .into_response();
                }
            }
        }
    };

    let new_pq = if let Some(q) = req.uri().query() {
        format!("/{rest}?{q}")
    } else {
        format!("/{rest}")
    };
    let mut parts = req.uri().clone().into_parts();
    parts.path_and_query = Some(new_pq.parse().unwrap());
    *req.uri_mut() = axum::http::Uri::from_parts(parts).unwrap();

    router.oneshot(req).await.unwrap_or_else(|_| unreachable!())
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stderr)
        .init();

    let socket = resolve_socket(args.socket);

    match args.port {
        Some(port) => {
            let ct = tokio_util::sync::CancellationToken::new();
            let state = GatewayState {
                socket,
                routers: Arc::new(RwLock::new(HashMap::new())),
                ct: ct.clone(),
            };
            let app = Router::new()
                .route("/:agent_name/*rest", any(gateway_handler))
                .with_state(state);
            let bind = format!("127.0.0.1:{port}");
            log::info!("HTTP MCP gateway on http://{bind}/  (connects to daemon per agent name)");
            eprintln!("[twilight-mcp] HTTP gateway mode — http://{bind}/<agent_name>/mcp");
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    tokio::signal::ctrl_c().await.unwrap();
                    ct.cancel();
                })
                .await?;
        }
        None => {
            let client = Arc::new(
                DaemonClient::connect(&socket, &args.name, "mcp-agent").await?,
            );
            let handler = FabricHandler { client };
            let svc = handler.serve(stdio()).await.inspect_err(|e| {
                log::error!("MCP server error: {:?}", e);
            })?;
            svc.waiting().await?;
        }
    }
    Ok(())
}
