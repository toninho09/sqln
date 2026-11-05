//! Real stdio initialization and private instance discovery with the MCP SDK.

use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::json;
use sqln::agent::{self, Operation};
use std::time::Duration;

#[tokio::test]
async fn stdio_lists_typed_tools_and_routes_only_to_the_explicit_instance() {
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
    assert_eq!(tools.len(), 25);
    let edit = tools
        .iter()
        .find(|tool| tool.name == "edit_console")
        .unwrap();
    assert!(edit.input_schema["properties"]["instance_id"].is_object());
    assert!(
        !edit.input_schema["properties"]
            .as_object()
            .unwrap()
            .contains_key("method")
    );
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
    let args = json!({"instance_id":second.instance.instance_id});
    let mut alive = None;
    let answer = {
        let request = client.call_tool(
            CallToolRequestParams::new("request_access")
                .with_arguments(args.as_object().unwrap().clone()),
        );
        tokio::pin!(request);
        loop {
            tokio::select! {
                result=&mut request=>break result.unwrap(),
                ()=tokio::time::sleep(Duration::from_millis(1))=>{
                    assert!(first.requests.try_recv().is_err());
                    if let Ok(request)=second.requests.try_recv(){assert!(matches!(request.operation,Operation::RequestAccess{}));alive=Some(request.alive.clone());request.reply.send(Ok(json!({"status":"pending"}))).unwrap();}
                }
            }
        }
    };
    assert_eq!(answer.structured_content.unwrap()["status"], "pending");
    let invalid = client
        .call_tool(
            CallToolRequestParams::new("get_context").with_arguments(
                json!({"instance_id":uuid::Uuid::new_v4().to_string()})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(invalid.structured_content.unwrap()["code"], "instance_gone");
    client.cancel().await.unwrap();
    let alive = alive.unwrap();
    tokio::time::timeout(Duration::from_secs(5), alive.cancelled())
        .await
        .unwrap();
    drop(first);
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
