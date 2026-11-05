//! Real stdio initialization and private instance discovery with the MCP SDK.

use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::json;
use sqln::agent::{self, Operation};
use std::time::Duration;

/// Sends one tool call. Until it ends, the `answer` function replies to the requests of `server`,
/// and the other server must receive nothing.
async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    args: serde_json::Value,
    server: &agent::ipc::Server,
    other: Option<&agent::ipc::Server>,
    mut answer: impl FnMut(&Operation) -> agent::Reply,
) -> (
    serde_json::Value,
    Vec<(Operation, tokio_util::sync::CancellationToken)>,
) {
    let request = client.call_tool(
        CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
    );
    tokio::pin!(request);
    let mut seen = Vec::new();
    loop {
        tokio::select! {
            result = &mut request => return (result.unwrap().structured_content.unwrap(), seen),
            () = tokio::time::sleep(Duration::from_millis(1)) => {
                if let Some(other) = other {
                    assert!(other.requests.try_recv().is_err());
                }
                while let Ok(request) = server.requests.try_recv() {
                    request.reply.send(answer(&request.operation)).unwrap();
                    seen.push((request.operation, request.alive));
                }
            }
        }
    }
}

#[tokio::test]
async fn stdio_lists_typed_tools_routes_to_one_instance_and_waits_for_tasks() {
    let root = std::env::temp_dir().join(format!(
        "sm-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    ));
    agent::ipc::private_directory(&root).unwrap();
    let dir = root.join(format!("sqln-{}", rustix::process::getuid().as_raw()));
    let first = agent::ipc::Server::at(dir.clone(), None).unwrap();
    let second = agent::ipc::Server::at(dir.clone(), None).unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_sqln"));
    command.arg("mcp").env("XDG_RUNTIME_DIR", &root);
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(
        names,
        [
            "list_instances",
            "request_access",
            "get_context",
            "read_console",
            "read_result",
            "query",
            "run_console",
            "edit_console",
            "describe",
            "compare",
            "job"
        ]
    );
    let tool = |name: &str| tools.iter().find(|tool| tool.name == name).unwrap();
    let edit = tool("edit_console");
    assert!(edit.input_schema["properties"]["instance_id"].is_object());
    assert!(edit.input_schema["properties"]["wait_seconds"].is_object());
    assert!(
        !edit.input_schema["required"]
            .as_array()
            .is_some_and(|required| required.contains(&json!("instance_id")))
    );
    assert!(
        !edit.input_schema["properties"]
            .as_object()
            .unwrap()
            .contains_key("method")
    );
    assert!(tool("get_context").input_schema["properties"]["wait_seconds"].is_null());
    // Each tool schema stands alone: clients that do not resolve `$ref` still see the
    // nested arguments, such as the sides of a comparison.
    for tool in &tools {
        let schema = serde_json::to_string(&tool.input_schema).unwrap();
        assert!(!schema.contains("$ref"), "{}: {schema}", tool.name);
    }
    let side = &tool("compare").input_schema["properties"]["source"];
    assert_eq!(side["type"], "object", "{side}");
    assert!(side["properties"]["connection_id"].is_object(), "{side}");
    let list = client
        .call_tool(CallToolRequestParams::new("list_instances"))
        .await
        .unwrap();
    let instances = &list.structured_content.unwrap()["instances"];
    assert_eq!(instances.as_array().unwrap().len(), 2);
    assert!(
        instances
            .as_array()
            .unwrap()
            .iter()
            .all(|instance| instance.get("socket").is_none()
                && instance.get("connections").is_none())
    );
    // Two instances: a call without instance_id never selects one.
    let (ambiguous, _) = call(
        &client,
        "get_context",
        json!({}),
        &second,
        Some(&first),
        |_| unreachable!(),
    )
    .await;
    assert_eq!(ambiguous["code"], "ambiguous_instance");
    // request_access waits until the user decides.
    let id = second.instance.instance_id.clone();
    let mut decided = 0;
    let (access, seen) = call(
        &client,
        "request_access",
        json!({"instance_id":id}),
        &second,
        Some(&first),
        |operation| {
            assert!(matches!(operation, Operation::RequestAccess {}));
            decided += 1;
            Ok(json!({"status": if decided < 3 { "pending" } else { "authorized" }}))
        },
    )
    .await;
    assert_eq!(access["status"], "authorized");
    assert_eq!(seen.len(), 3);
    // A query waits for its task and returns the final reply.
    let mut polls = 0;
    let (done, seen) = call(
        &client,
        "query",
        json!({"instance_id":id,"connection_id":"1","sql":"SELECT 1"}),
        &second,
        Some(&first),
        |operation| match operation {
            Operation::Query { .. } => Ok(json!({"job_id":"t","kind":"Query","status":"validating"})),
            Operation::Job { cancel: false, .. } => {
                polls += 1;
                Ok(json!({"job_id":"t","kind":"Query","status": if polls < 2 { "running" } else { "completed" },"results":[{"rows":[]}]}))
            }
            other => panic!("unexpected {other:?}"),
        },
    )
    .await;
    assert_eq!(done["status"], "completed");
    assert!(done["results"].is_array());
    assert_eq!(seen.len(), 3);
    // After wait_seconds, the reply holds the current state and the next action.
    let (running, _) = call(
        &client,
        "query",
        json!({"instance_id":id,"connection_id":"1","sql":"SELECT 1","wait_seconds":0}),
        &second,
        Some(&first),
        |_| Ok(json!({"job_id":"t","kind":"Query","status":"running"})),
    )
    .await;
    assert_eq!(running["status"], "running");
    assert!(running["next_action"].as_str().unwrap().contains("job"));
    let (refused, _) = call(
        &client,
        "get_context",
        json!({"instance_id":id,"wait_seconds":1}),
        &second,
        Some(&first),
        |_| unreachable!(),
    )
    .await;
    assert_eq!(refused["code"], "invalid_request");
    let (invalid, _) = call(
        &client,
        "get_context",
        json!({"instance_id":uuid::Uuid::new_v4().to_string()}),
        &second,
        Some(&first),
        |_| unreachable!(),
    )
    .await;
    assert_eq!(invalid["code"], "instance_gone");
    // A structure comparison reaches SQLN, with and without a table on each side.
    for table in [None, Some("player")] {
        let side = |connection: &str| {
            let mut side = json!({"connection_id":connection,"schema":"public"});
            if let Some(table) = table {
                side["table"] = json!(table);
            }
            side
        };
        let (compared, seen) = call(
            &client,
            "compare",
            json!({"instance_id":id,"kind":"structure","source":side("1"),"target":side("2")}),
            &second,
            Some(&first),
            |operation| match operation {
                Operation::Compare {
                    kind: agent::CompareKind::Structure,
                    source,
                    target,
                    removals: false,
                    rows: None,
                } => {
                    assert_eq!(
                        (source.connection_id.as_str(), target.connection_id.as_str()),
                        ("1", "2")
                    );
                    assert_eq!(source.table.as_deref(), table);
                    assert_eq!(target.table.as_deref(), table);
                    Ok(json!({"job_id":"c","kind":"Comparison","status":"completed"}))
                }
                other => panic!("unexpected {other:?}"),
            },
        )
        .await;
        assert_eq!(compared["status"], "completed", "{compared}");
        assert_eq!(seen.len(), 1);
    }
    // A wrong argument names its cause, and nothing reaches SQLN.
    let (wrong, _) = call(
        &client,
        "compare",
        json!({"instance_id":id,"kind":"structure","source":"{\"connection_id\":\"1\"}","target":{"connection_id":"2","schema":"public"}}),
        &second,
        Some(&first),
        |_| unreachable!(),
    )
    .await;
    assert_eq!(wrong["code"], "invalid_request");
    let message = wrong["message"].as_str().unwrap();
    assert!(
        message.starts_with("Use the declared tool arguments"),
        "{message}"
    );
    assert!(message.contains("invalid type"), "{message}");
    // One instance: a call without instance_id selects it.
    drop(first);
    let (context, mut seen) = call(&client, "get_context", json!({}), &second, None, |_| {
        Ok(json!({"access":"propose"}))
    })
    .await;
    assert_eq!(context["access"], "propose");
    assert_eq!(seen.len(), 1);
    // Closing the client ends its session in SQLN.
    let (_, alive) = seen.remove(0);
    client.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), alive.cancelled())
        .await
        .unwrap();
    drop(second);
    assert!(agent::ipc::discover(&dir).await.unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_registry_rejects_shared_directories_and_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = std::env::temp_dir().join(format!(
        "sr-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(agent::ipc::Server::at(dir.clone(), None).is_err());
    let link = dir.with_extension("link");
    symlink(&dir, &link).unwrap();
    assert!(agent::ipc::Server::at(link.clone(), None).is_err());
    std::fs::remove_file(link).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[tokio::test]
async fn abandoned_descriptors_do_not_select_a_different_process() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "sd-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    ));
    agent::ipc::private_directory(&dir).unwrap();
    let instance = agent::ipc::Instance {
        instance_id: uuid::Uuid::new_v4().to_string(),
        pid: std::process::id(),
        working_directory: dir.clone(),
        project: None,
        version: agent::VERSION,
        socket: dir.join("missing.sock"),
    };
    let path = dir.join(format!("{}.json", instance.instance_id));
    std::fs::write(&path, serde_json::to_vec(&instance).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(agent::ipc::discover(&dir).await.unwrap().is_empty());
    assert!(!path.exists());
    std::fs::remove_dir_all(dir).unwrap();
}
