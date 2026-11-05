//! MCP stdio adapter. Every target operation goes through the application's local gate.

use crate::agent::ipc::{Client, Instance};
use crate::agent::{self, Error, Operation, Reply};
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;

type Clients = Mutex<HashMap<String, Arc<Mutex<Client>>>>;
#[derive(Default)]
pub struct Bridge {
    clients: Clients,
}

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    let schema = serde_json::to_value(schemars::schema_for!(Operation))
        .expect("the operation schema is JSON");
    let mut tools = vec![Tool::new(
        "list_instances",
        "List open SQLN instances. Never guess a target when several instances match.",
        Arc::new(
            serde_json::from_value(
                json!({"type":"object", "properties":{}, "additionalProperties":false}),
            )
            .expect("constant schema"),
        ),
    )];
    tools[0].annotations = Some(ToolAnnotations::new().read_only(true));
    for variant in schema["oneOf"]
        .as_array()
        .expect("the operation schema is a tagged enum")
    {
        let mut input = variant.clone();
        let name = input["properties"]["method"]["const"]
            .as_str()
            .expect("a tagged operation has a method")
            .to_owned();
        let description = input["description"]
            .as_str()
            .unwrap_or("Operate on an authorized SQLN instance.")
            .to_owned();
        input["properties"]
            .as_object_mut()
            .expect("operation properties")
            .remove("method");
        input["properties"]["instance_id"] = json!({"type":"string", "description":"The exact instance_id returned by list_instances."});
        let required = input["required"]
            .as_array_mut()
            .expect("operation required fields");
        required.retain(|field| field != "method");
        required.push(json!("instance_id"));
        input["$defs"] = schema["$defs"].clone();
        let mut tool = Tool::new(
            name,
            description,
            Arc::new(input.as_object().expect("operation schema object").clone()),
        );
        // Executions also change visible results. These annotations are not access controls.
        let inspection = matches!(
            tool.name.as_ref(),
            "get_access"
                | "get_context"
                | "list_connections"
                | "list_tabs"
                | "read_console"
                | "read_messages"
                | "read_result"
                | "read_result_cell"
                | "get_job"
                | "get_proposal"
                | "read_artifact"
        );
        tool.annotations = Some(ToolAnnotations::new().read_only(inspection));
        tools.push(tool);
    }
    tools
});

impl Bridge {
    async fn instances() -> Result<Vec<Instance>, Error> {
        let dir = agent::ipc::runtime_dir().map_err(|_| {
            Error::new(
                "discovery_failed",
                "Cannot open the private SQLN runtime directory. Check its permissions.",
            )
        })?;
        agent::ipc::discover(&dir).await.map_err(|_| {
            Error::new(
                "discovery_failed",
                "Cannot discover SQLN instances. Check the local runtime directory.",
            )
        })
    }
    async fn invoke(
        &self,
        request: CallToolRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> Reply {
        let name = request.name.as_ref();
        let mut args = request.arguments.unwrap_or_default();
        if name == "list_instances" {
            if !args.is_empty() {
                return Err(Error::new(
                    "invalid_request",
                    "list_instances takes no arguments.",
                ));
            }
            let instances: Vec<_> = Self::instances().await?.into_iter().map(|instance| json!({
                "instance_id": instance.instance_id, "pid": instance.pid, "working_directory": instance.working_directory, "project": instance.project,
            })).collect();
            return Ok(json!({"instances": instances}));
        }
        let id = args
            .remove("instance_id")
            .and_then(|id| id.as_str().map(str::to_owned))
            .filter(|id| uuid::Uuid::parse_str(id).is_ok())
            .ok_or_else(|| {
                Error::new("invalid_target", "Use an instance_id from list_instances.")
            })?;
        if args.contains_key("method") {
            return Err(Error::new(
                "invalid_request",
                "The method field is not an argument.",
            ));
        }
        args.insert("method".into(), json!(name));
        let operation: Operation = serde_json::from_value(Value::Object(args)).map_err(|_| {
            Error::new(
                "invalid_request",
                "Use the declared tool arguments and valid field types.",
            )
        })?;
        if context.ct.is_cancelled() {
            return Err(Error::new("cancelled", "The request was cancelled."));
        }
        let connection = {
            let mut clients = self.clients.lock().await;
            if let Some(client) = clients.get(&id) {
                client.clone()
            } else {
                let instance = Self::instances().await?.into_iter().find(|item| item.instance_id == id)
                    .ok_or_else(|| Error::new("instance_gone", "The selected instance is not open. List instances again; do not switch targets silently."))?;
                let label = context.client_info().map_or_else(
                    || "MCP client".into(),
                    |info| info.name.chars().take(64).collect::<String>(),
                );
                let client = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    Client::connect(&instance, &label),
                )
                .await
                .map_err(|_| Error::new("instance_gone", "The selected instance did not answer."))?
                .map_err(|_| Error::new("instance_gone", "The selected instance disconnected."))?;
                let client = Arc::new(Mutex::new(client));
                clients.insert(id.clone(), client.clone());
                client
            }
        };
        let result = {
            let mut client = connection.lock().await;
            let result = client.call(&operation).await;
            if context.ct.is_cancelled()
                && let Ok(value) = &result
                && let Some(job_id) = value["job_id"].as_str()
            {
                let _ = client
                    .call(&Operation::CancelJob {
                        job_id: job_id.into(),
                    })
                    .await;
            }
            result
        };
        if result
            .as_ref()
            .is_err_and(|error| error.code == "instance_gone")
        {
            self.clients.lock().await.remove(&id);
        }
        result
    }
}

impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info.name = "sqln".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.instructions = Some("First list instances. Ask the user if the target is ambiguous. Request access in that exact instance and wait for user approval in SQLN. Use explicit IDs and revisions. Database operations are read-only. The user accepts edit proposals in SQLN. Never execute generated write SQL.".into());
        info
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        TOOLS.iter().find(|tool| tool.name == name).cloned()
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult {
            tools: TOOLS.clone(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        let result = match self.invoke(request, &context).await {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => CallToolResult::structured_error(json!(error)),
        };
        let result = if serde_json::to_vec(&result)
            .map_or(true, |encoded| encoded.len() > agent::PAGE_BYTES - 4096)
        {
            CallToolResult::structured_error(json!(Error::new(
                "response_too_large",
                "Use a smaller page or a paged artifact. The MCP response would exceed 256 KiB."
            )))
        } else {
            result
        };
        Ok(result.into())
    }
}

/// Runs only the protocol. This path never starts the terminal or reads a password.
pub fn run() -> std::io::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let bridge = Bridge::default();
            let transport = rmcp::transport::async_rw::AsyncRwTransport::new_server(
                tokio::io::stdin(),
                tokio::io::stdout(),
            )
            .with_max_line_length(agent::MAX_FRAME);
            let service = bridge
                .serve(transport)
                .await
                .map_err(|_| std::io::Error::other("MCP initialization failed."))?;
            service
                .waiting()
                .await
                .map_err(|_| std::io::Error::other("MCP transport failed."))?;
            Ok(())
        })
}
