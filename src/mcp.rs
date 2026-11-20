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

/// The connection of one instance. It connects once, also with parallel first calls, and
/// a failed connection leaves it empty for the next call.
type Slot = Arc<tokio::sync::OnceCell<Arc<Mutex<Client>>>>;
type Clients = Mutex<HashMap<String, Slot>>;
#[derive(Default)]
pub struct Bridge {
    clients: Clients,
    /// The instance of the last connected call. A call without `instance_id` stays on it,
    /// also after it closes, so the bridge never moves to another instance by itself.
    current: Mutex<Option<String>>,
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
/// How the agent works with SQLN. Tool descriptions hold the details of each tool.
const INSTRUCTIONS: &str = "Call request_access first: it waits until the user decides in SQLN. \
Without instance_id, a call uses the instance of the previous call, or else the only open instance; with several, call list_instances and ask the user if the target is not clear. \
Then call get_context for connections and tabs. Each connection has its dialect, production (a run that can change data asks the user first), and read_only (the database refuses writes). \
Write SQL in the dialect of its connection. Before you write SQL, call describe for the tables, columns, topics, and topic fields; do not guess names. \
describe with schema and no object reads the catalog of that schema; a relation with columns_loaded false needs describe with its object. \
A Kafka connection does not accept standard SQL: follow syntax.Kafka, which the first get_context of the session holds; get_context with include_syntax sends it again. \
Bind each Kafka read with LIMIT, SCAN, or START AT; SQLN adds a SCAN to a read without one. When describe gives a topic no fields, read a few messages with LIMIT 5 to learn the paths. \
query runs SQL in your own console and returns its rows. Tools wait for the end of their task; call job when a reply says that the task continues. \
Read each statement of a reply: limit means that the grid kept only part of the rows, and notices hold the database messages. A Kafka result has read, with scanned, matched, and the end of the read; end scan with no match does not mean an empty topic. \
Without full access, only reads run: send other SQL with edit_console for the user to review and run. Use exact IDs and revisions.";

/// Most clients stop a tool call after 60 seconds.
const TASK_WAIT: u64 = 50;
/// The user needs time to read an access request.
const ACCESS_WAIT: u64 = 120;
const MAX_WAIT: u64 = 600;

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(tools);

/// Builds the tools from the schema of `Operation`. A test lists the tools, so a
/// change of the schema that breaks these checks fails the test suite.
#[expect(clippy::expect_used, reason = "the operation schema is constant")]
fn tools() -> Vec<Tool> {
    // Inline subschemas: many clients do not resolve `$ref`, and then send a nested
    // argument, such as a compare side, as text.
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<Operation>();
    let schema = serde_json::to_value(schema).expect("the operation schema is JSON");
    let mut tools = vec![Tool::new(
        "list_instances",
        "List open SQLN instances. An instance with compatible false runs another sqln version and refuses calls. Without instance_id, other tools use the instance of the previous call, or else the only open instance.",
        Arc::new(
            serde_json::from_value(
                json!({"type":"object", "properties":{}, "additionalProperties":false}),
            )
            .expect("constant schema"),
        ),
    )];
    tools[0].annotations = Some(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .open_world(false),
    );
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
        properties.insert("instance_id".into(), json!({"type":"string", "description":"The instance_id from list_instances. Without it, the call uses the instance of the previous call, or else the only open instance."}));
        if WAITING_TOOLS.contains(&name.as_str()) {
            let description = if name == "request_access" {
                format!(
                    "Seconds to wait for the user's decision. The default is {ACCESS_WAIT}. After this time, the reply holds the current state: call request_access again to keep waiting."
                )
            } else {
                format!(
                    "Seconds to wait for the end. The default is {TASK_WAIT}. After this time, the reply holds the current state: call job with its job_id to keep waiting."
                )
            };
            properties.insert("wait_seconds".into(), json!({"type":"integer", "minimum":0, "maximum":MAX_WAIT, "description":description}));
        }
        input["required"]
            .as_array_mut()
            .expect("operation required fields")
            .retain(|field| field != "method");
        let mut tool = Tool::new(
            name,
            description,
            Arc::new(input.as_object().expect("operation schema object").clone()),
        );
        // These annotations are hints for the client, not access controls. Reads, catalog
        // reads, and comparisons change no data: they only open tabs. All tools act on a
        // local SQLN instance.
        let read_only = matches!(
            tool.name.as_ref(),
            "get_context" | "read_console" | "read_result" | "describe" | "compare"
        );
        let destructive = matches!(tool.name.as_ref(), "query" | "run_console" | "edit_console");
        // A second access request only reads the state of the first one.
        let idempotent = tool.name == "request_access";
        tool.annotations = Some(
            ToolAnnotations::new()
                .read_only(read_only)
                .destructive(destructive)
                .idempotent(idempotent)
                .open_world(false),
        );
        tools.push(tool);
    }
    tools
}

/// The longest part of a decoding error in a reply.
const ARGUMENT_ERROR_CHARS: usize = 300;

/// Names the argument that the bridge cannot decode, so the agent can correct its call.
/// The reason goes only to the agent that sent the arguments, never to a log.
fn argument_error(error: &serde_json::Error) -> String {
    let reason: String = error
        .to_string()
        .chars()
        .take(ARGUMENT_ERROR_CHARS)
        .collect();
    format!("Use the declared tool arguments and valid field types: {reason}")
}

fn version_mismatch() -> Error {
    Error::new(
        "version_mismatch",
        "This SQLN instance uses another version of the local protocol. Ask the user to run the MCP client with the same sqln executable as the open SQLN, or to restart SQLN.",
    )
}

fn invalid_request(message: &str) -> Error {
    Error::new("invalid_request", message)
}

/// The longest MCP response, with space for the JSON-RPC envelope.
const RESPONSE_BYTES: usize = agent::PAGE_BYTES - 4096;

/// Builds the tool result within `RESPONSE_BYTES`. The result holds the reply twice, as
/// structured content and as JSON text, so a reply that SQLN accepts can be too large.
/// The reply then loses the parts that the agent can read again, and keeps its task
/// state: a task that ran never looks like a failure.
fn response(reply: Reply) -> CallToolResult {
    let mut value = match reply {
        Ok(value) => value,
        Err(error) => return CallToolResult::structured_error(json!(error)),
    };
    let fits = |result: &CallToolResult| {
        serde_json::to_vec(result).is_ok_and(|encoded| encoded.len() <= RESPONSE_BYTES)
    };
    let mut shortened = false;
    loop {
        if shortened {
            value["shortened"] = json!(SHORTENED);
        }
        let result = CallToolResult::structured(value.clone());
        if fits(&result) {
            return result;
        }
        if !shorten(&mut value) {
            break;
        }
        shortened = true;
    }
    let mut error = json!(Error::new(
        "response_too_large",
        "The reply exceeds the MCP response limit of 256 KiB. Read rows with read_result and a smaller limit, large values with row and column, and artifacts with job and artifact_offset.",
    ));
    for key in ["job_id", "kind", "status"] {
        if let Some(item) = value.get(key) {
            error[key] = item.clone();
        }
    }
    CallToolResult::structured_error(error)
}

const SHORTENED: &str = "The reply was shortened to fit the MCP response. Read the omitted rows with read_result from next_offset, and an omitted artifact with job and artifact_offset.";

/// Removes one part that the agent can read again: rows of the last result page first,
/// then the artifact page. Returns false when no such part remains.
fn shorten(value: &mut Value) -> bool {
    if let Some(results) = value.get_mut("results").and_then(Value::as_array_mut) {
        let paged = results
            .iter()
            .filter(|page| page.get("rows").is_some())
            .count();
        if let Some(page) = results
            .iter_mut()
            .rev()
            .find(|page| page.get("rows").is_some())
        {
            // The first result keeps part of its rows; later results become references.
            if paged > 1 || !halve(page) {
                *page = json!({
                    "tab_id": page["tab_id"],
                    "result": page["result"],
                    "generation": page["generation"],
                    "rows_omitted": true,
                });
            }
            return true;
        }
    }
    if halve(value) {
        return true;
    }
    if value
        .get("artifact")
        .is_some_and(|artifact| !artifact.is_null())
    {
        value["artifact"] = Value::Null;
        value["artifact_omitted"] = json!(true);
        return true;
    }
    false
}

/// Keeps the first half of a row page. `next_offset` names the first removed row.
fn halve(page: &mut Value) -> bool {
    let Some(rows) = page.get_mut("rows").and_then(Value::as_array_mut) else {
        return false;
    };
    let keep = rows.len() / 2;
    let Some(next) = rows
        .get(keep)
        .filter(|_| keep > 0)
        .and_then(|row| row.get("row"))
        .cloned()
    else {
        return false;
    };
    rows.truncate(keep);
    page["next_offset"] = next;
    true
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

    /// Returns the requested instance, else the instance of the previous call, else the
    /// only open instance.
    async fn target(&self, id: Option<Value>) -> Result<String, Error> {
        if let Some(id) = id {
            return id
                .as_str()
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                .map(str::to_owned)
                .ok_or_else(|| {
                    Error::new("invalid_target", "Use an instance_id from list_instances.")
                });
        }
        if let Some(current) = self.current.lock().await.clone() {
            return Ok(current);
        }
        let (mut instances, other): (Vec<_>, Vec<_>) = Self::instances()
            .await?
            .into_iter()
            .partition(Instance::compatible);
        if instances.is_empty() && !other.is_empty() {
            return Err(version_mismatch());
        }
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
        // The map lock covers only the lookup: a slow instance does not hold the others.
        let slot = self
            .clients
            .lock()
            .await
            .entry(id.to_owned())
            .or_default()
            .clone();
        let label = context.client_info().map_or_else(
            || "MCP client".into(),
            |info| info.name.chars().take(64).collect::<String>(),
        );
        slot.get_or_try_init(|| async {
            let instance = Self::instances().await?.into_iter().find(|item| item.instance_id == id)
                .ok_or_else(|| Error::new("instance_gone", "The SQLN instance of this session is not open. Call list_instances and send instance_id; ask the user if the target is not clear. Do not switch targets silently."))?;
            if !instance.compatible() {
                return Err(version_mismatch());
            }
            let client = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                Client::connect(&instance, &label),
            )
            .await
            .map_err(|_| Error::new("instance_gone", "The selected instance did not answer."))?
            .map_err(|_| Error::new("instance_gone", "The selected instance disconnected."))?;
            Ok(Arc::new(Mutex::new(client)))
        })
        .await
        .cloned()
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
                "instance_id": instance.instance_id, "pid": instance.pid, "working_directory": instance.working_directory, "project": instance.project, "compatible": instance.compatible(),
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
        let id = self.target(args.remove("instance_id")).await?;
        if args.contains_key("method") {
            return Err(invalid_request("The method field is not an argument."));
        }
        args.insert("method".into(), json!(name));
        let operation: Operation = serde_json::from_value(Value::Object(args))
            .map_err(|error| invalid_request(&argument_error(&error)))?;
        if context.ct.is_cancelled() {
            return Err(Error::new("cancelled", "The request was cancelled."));
        }
        let connection = self.connect(&id, context).await?;
        *self.current.lock().await = Some(id.clone());
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
        info.instructions = Some(INSTRUCTIONS.into());
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
        Ok(response(self.invoke(request, &context).await).into())
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
