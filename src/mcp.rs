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

/// Tools that wait for the end of their task, or for the user's decision.
const WAITING_TOOLS: [&str; 7] = [
    "request_access",
    "query",
    "run_console",
    "edit_console",
    "describe",
    "compare",
    "job",
];
/// States that can still change without a new request.
const WAITING_STATES: [&str; 8] = [
    "pending",
    "building",
    "validating",
    "ready",
    "waiting_credentials",
    "waiting_confirmation",
    "running",
    "building_artifact",
];
/// Most clients stop a tool call after 60 seconds.
const TASK_WAIT: u64 = 50;
/// The user needs time to read an access request.
const ACCESS_WAIT: u64 = 120;
const MAX_WAIT: u64 = 600;

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    let schema = serde_json::to_value(schemars::schema_for!(Operation))
        .expect("the operation schema is JSON");
    let mut tools = vec![Tool::new(
        "list_instances",
        "List open SQLN instances. Other tools select the only open instance when instance_id is absent.",
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
        let properties = input["properties"]
            .as_object_mut()
            .expect("operation properties");
        properties.remove("method");
        properties.insert("instance_id".into(), json!({"type":"string", "description":"The instance_id from list_instances. Optional when only one SQLN instance is open."}));
        if WAITING_TOOLS.contains(&name.as_str()) {
            properties.insert("wait_seconds".into(), json!({"type":"integer", "minimum":0, "maximum":MAX_WAIT,
                "description":format!("Seconds to wait for the end. The default is {TASK_WAIT}, or {ACCESS_WAIT} for request_access. After this time, the reply holds the current state: call job to continue.")}));
        }
        input["required"]
            .as_array_mut()
            .expect("operation required fields")
            .retain(|field| field != "method");
        input["$defs"] = schema["$defs"].clone();
        let mut tool = Tool::new(
            name,
            description,
            Arc::new(input.as_object().expect("operation schema object").clone()),
        );
        // Executions also change visible results. These annotations are not access controls.
        let inspection = matches!(
            tool.name.as_ref(),
            "get_context" | "read_console" | "read_result"
        );
        tool.annotations = Some(ToolAnnotations::new().read_only(inspection));
        tools.push(tool);
    }
    tools
});

fn invalid_request(message: &str) -> Error {
    Error::new("invalid_request", message)
}

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

    /// Returns the requested instance, or the only open instance.
    async fn target(id: Option<Value>) -> Result<String, Error> {
        if let Some(id) = id {
            return id
                .as_str()
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                .map(str::to_owned)
                .ok_or_else(|| {
                    Error::new("invalid_target", "Use an instance_id from list_instances.")
                });
        }
        let mut instances = Self::instances().await?;
        match instances.len() {
            0 => Err(Error::new(
                "no_instance",
                "No SQLN instance is open. Ask the user to open SQLN, then call this tool again.",
            )),
            1 => Ok(instances.remove(0).instance_id),
            _ => Err(Error::new(
                "ambiguous_instance",
                "Several SQLN instances are open. Call list_instances and send instance_id. Ask the user if the target is not clear.",
            )),
        }
    }

    async fn connect(
        &self,
        id: &str,
        context: &RequestContext<RoleServer>,
    ) -> Result<Arc<Mutex<Client>>, Error> {
        let mut clients = self.clients.lock().await;
        if let Some(client) = clients.get(id) {
            return Ok(client.clone());
        }
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
        clients.insert(id.to_owned(), client.clone());
        Ok(client)
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
                return Err(invalid_request("list_instances takes no arguments."));
            }
            let instances: Vec<_> = Self::instances().await?.into_iter().map(|instance| json!({
                "instance_id": instance.instance_id, "pid": instance.pid, "working_directory": instance.working_directory, "project": instance.project,
            })).collect();
            return Ok(json!({"instances": instances}));
        }
        let wait = match args.remove("wait_seconds") {
            None => None,
            Some(value) if WAITING_TOOLS.contains(&name) => Some(
                value
                    .as_u64()
                    .ok_or_else(|| invalid_request("wait_seconds is a whole number of seconds."))?
                    .min(MAX_WAIT),
            ),
            Some(_) => return Err(invalid_request("This tool does not wait.")),
        };
        let id = Self::target(args.remove("instance_id")).await?;
        if args.contains_key("method") {
            return Err(invalid_request("The method field is not an argument."));
        }
        args.insert("method".into(), json!(name));
        let operation: Operation = serde_json::from_value(Value::Object(args)).map_err(|_| {
            invalid_request("Use the declared tool arguments and valid field types.")
        })?;
        if context.ct.is_cancelled() {
            return Err(Error::new("cancelled", "The request was cancelled."));
        }
        let connection = self.connect(&id, context).await?;
        let access = matches!(operation, Operation::RequestAccess {});
        let wait = std::time::Duration::from_secs(wait.unwrap_or(if access {
            ACCESS_WAIT
        } else {
            TASK_WAIT
        }));
        let result = Self::wait(&connection, operation, wait, context).await;
        if result
            .as_ref()
            .is_err_and(|error| error.code == "instance_gone")
        {
            self.clients.lock().await.remove(&id);
        }
        result
    }

    /// Sends the operation, then repeats a state read until the state is final or the time ends.
    /// The lock covers one request at a time, so parallel tool calls continue.
    async fn wait(
        connection: &Mutex<Client>,
        operation: Operation,
        wait: std::time::Duration,
        context: &RequestContext<RoleServer>,
    ) -> Reply {
        let access = matches!(operation, Operation::RequestAccess {});
        let deadline = tokio::time::Instant::now() + wait;
        let mut delay = std::time::Duration::from_millis(50);
        let mut value = connection.lock().await.call(&operation).await?;
        loop {
            let pending = value["status"]
                .as_str()
                .is_some_and(|status| WAITING_STATES.contains(&status));
            let job = value["job_id"].as_str().map(str::to_owned);
            let task = job.clone().filter(|_| value["kind"] != "proposal");
            if context.ct.is_cancelled() {
                if let Some(job) = task.filter(|_| pending) {
                    let _ = connection
                        .lock()
                        .await
                        .call(&Operation::Job {
                            job_id: job,
                            cancel: true,
                            artifact_offset: 0,
                            rows: None,
                        })
                        .await;
                }
                return Err(Error::new("cancelled", "The request was cancelled."));
            }
            if !pending || (!access && job.is_none()) {
                return Ok(value);
            }
            if tokio::time::Instant::now() >= deadline {
                value["next_action"] = json!(if access {
                    "The user did not decide yet. Call request_access again to keep waiting."
                } else if task.is_some() {
                    "The task continues. Call job with this job_id to keep waiting, or with cancel to stop it."
                } else {
                    "The proposal waits for the user's review. Call job with this job_id to keep waiting."
                });
                return Ok(value);
            }
            tokio::select! {
                () = context.ct.cancelled() => continue,
                () = tokio::time::sleep(delay.min(deadline - tokio::time::Instant::now())) => {}
            }
            delay = (delay * 2).min(std::time::Duration::from_millis(500));
            let next = match job {
                Some(job_id) if !access => Operation::Job {
                    job_id,
                    cancel: false,
                    artifact_offset: 0,
                    rows: None,
                },
                _ => Operation::RequestAccess {},
            };
            value = connection.lock().await.call(&next).await?;
        }
    }
}

impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info.name = "sqln".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.instructions = Some("Call request_access first: it waits until the user decides in SQLN. instance_id is optional when one SQLN instance is open; with several, call list_instances and ask the user if the target is not clear. Then call get_context for connections and tabs. query runs SQL in your own console and returns its rows. Tools wait for the end of their task; call job when a reply says that the task continues. Without full access, only reads run: send other SQL with edit_console for the user to review and run. Use exact IDs and revisions.".into());
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
