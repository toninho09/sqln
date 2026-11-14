//! Private, versioned local communication with an open SQLN process.

use super::{Error, MAX_FRAME, Operation, Reply, VERSION};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instance {
    pub instance_id: String,
    pub pid: u32,
    pub working_directory: PathBuf,
    pub project: Option<PathBuf>,
    pub version: u32,
    pub socket: PathBuf,
}

pub struct Envelope {
    pub session: String,
    pub client: String,
    pub alive: CancellationToken,
    pub operation: Operation,
    pub reply: oneshot::Sender<Reply>,
}

#[derive(Serialize, Deserialize)]
struct Hello {
    instance_id: String,
    version: u32,
    client: String,
}
type Wire = Framed<UnixStream, LengthDelimitedCodec>;
fn framed(stream: UnixStream) -> Wire {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME)
        .new_framed(stream)
}
fn socket_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!(
        "s-{}.sock",
        id.replace('-', "").chars().take(20).collect::<String>()
    ))
}
fn invalid() -> io::Error {
    io::Error::other("Invalid SQLN local protocol or instance.")
}
fn encode<T: Serialize>(value: &T) -> io::Result<bytes::Bytes> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() > MAX_FRAME {
        return Err(invalid());
    }
    Ok(bytes.into())
}
async fn receive<T: for<'a> Deserialize<'a>>(wire: &mut Wire) -> io::Result<T> {
    let data = wire.next().await.ok_or_else(invalid)??;
    serde_json::from_slice(&data).map_err(|_| invalid())
}

pub fn runtime_dir() -> io::Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    let path = base.join(format!("sqln-{uid}"));
    private_directory(&path)?;
    Ok(path)
}

pub fn private_directory(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "The SQLN runtime directory is not private. Check its owner and permissions.",
        ));
    }
    Ok(())
}

struct Cleanup {
    socket: Option<PathBuf>,
    descriptor: Option<PathBuf>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.socket {
            let _ = fs::remove_file(path);
        }
        if let Some(path) = &self.descriptor {
            let _ = fs::remove_file(path);
        }
    }
}

pub struct Server {
    pub instance: Instance,
    pub requests: Receiver<Envelope>,
    stop: CancellationToken,
    worker: Option<JoinHandle<()>>,
    descriptor: PathBuf,
    _lock: File,
}

impl Server {
    pub fn start(project: Option<PathBuf>) -> io::Result<Self> {
        Self::at(runtime_dir()?, project)
    }

    pub fn at(dir: PathBuf, project: Option<PathBuf>) -> io::Result<Self> {
        private_directory(&dir)?;
        let id = uuid::Uuid::new_v4().to_string();
        let socket = socket_path(&dir, &id);
        let instance = Instance {
            instance_id: id.clone(),
            pid: std::process::id(),
            working_directory: std::env::current_dir()?,
            project,
            version: VERSION,
            socket,
        };
        let listener = std::os::unix::net::UnixListener::bind(&instance.socket)?;
        let mut cleanup = Cleanup {
            socket: Some(instance.socket.clone()),
            descriptor: None,
        };
        fs::set_permissions(&instance.socket, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let descriptor = dir.join(format!("{id}.json"));
        let lock = OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .mode(0o600)
            .open(&descriptor)?;
        cleanup.descriptor = Some(descriptor.clone());
        lock.try_lock().map_err(io::Error::other)?;
        let (ready, started) = mpsc::sync_channel(1);
        let (sender, requests) = mpsc::sync_channel(64);
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let name = id;
        let worker = std::thread::Builder::new().name("sqln-agent-ipc".into()).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() { Ok(runtime) => runtime, Err(error) => { let _ = ready.send(Err(error)); return; } };
            runtime.block_on(async move {
                let listener = match UnixListener::from_std(listener) { Ok(listener) => listener, Err(error) => { let _ = ready.send(Err(error)); return; } };
                let _ = ready.send(Ok(()));
                let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(64));
                loop {
                    tokio::select! {
                        () = stopping.cancelled() => break,
                        result = listener.accept() => {
                            let Ok((stream, _)) = result else { break };
                            let Ok(slot) = slots.clone().try_acquire_owned() else { continue };
                            let sender = sender.clone(); let name = name.clone(); let stop = stopping.child_token();
                            tokio::spawn(async move { let _slot = slot; let _ = serve(stream, name, sender, stop).await; });
                        }
                    }
                }
            });
        })?;
        let mut server = Self {
            instance,
            requests,
            stop,
            worker: Some(worker),
            descriptor,
            _lock: lock,
        };
        cleanup.socket = None;
        cleanup.descriptor = None;
        started
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| io::Error::other("The local assistant worker did not start."))??;
        serde_json::to_writer(&mut server._lock, &server.instance).map_err(io::Error::other)?;
        server._lock.flush()?;
        Ok(server)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.descriptor);
        let _ = fs::remove_file(&self.instance.socket);
    }
}

async fn serve(
    stream: UnixStream,
    instance: String,
    sender: SyncSender<Envelope>,
    alive: CancellationToken,
) -> io::Result<()> {
    let cancel = alive.clone().drop_guard();
    let mut wire = framed(stream);
    let hello: Hello = tokio::time::timeout(Duration::from_secs(5), receive(&mut wire))
        .await
        .map_err(|_| invalid())??;
    if hello.version != VERSION || hello.instance_id != instance || hello.client.len() > 256 {
        return Err(invalid());
    }
    wire.send(encode(&VERSION)?).await?;
    let session = uuid::Uuid::new_v4().to_string();
    loop {
        let operation = tokio::select! {
            () = alive.cancelled() => break,
            result = receive::<Operation>(&mut wire) => result?,
        };
        let (reply, received) = oneshot::channel();
        let envelope = Envelope {
            session: session.clone(),
            client: hello.client.clone(),
            alive: alive.clone(),
            operation,
            reply,
        };
        if let Err(error) = sender.try_send(envelope) {
            let reply: Reply = Err(Error::new(
                "ui_busy",
                "SQLN has too many pending requests. Try again.",
            ));
            wire.send(encode(&reply)?).await?;
            if matches!(error, mpsc::TrySendError::Disconnected(_)) {
                break;
            }
            continue;
        }
        let result = tokio::select! {
            () = alive.cancelled() => break,
            // The bridge sends one request at a time. EOF also cancels an unanswered request.
            _ = wire.next() => break,
            result = tokio::time::timeout(Duration::from_secs(10), received) => match result {
                Ok(Ok(result)) => result,
                _ => Err(Error::new("ui_busy", "SQLN did not process the request. Try again.")),
            }
        };
        wire.send(encode(&result)?).await?;
    }
    drop(cancel);
    Ok(())
}

pub struct Client {
    wire: Wire,
}
impl Client {
    pub async fn connect(instance: &Instance, client: &str) -> io::Result<Self> {
        let meta = fs::symlink_metadata(&instance.socket)?;
        if !meta.file_type().is_socket()
            || meta.uid() != rustix::process::getuid().as_raw()
            || meta.mode() & 0o077 != 0
        {
            return Err(invalid());
        }
        let mut wire = framed(UnixStream::connect(&instance.socket).await?);
        wire.send(encode(&Hello {
            instance_id: instance.instance_id.clone(),
            version: VERSION,
            client: client.into(),
        })?)
        .await?;
        let version: u32 = receive(&mut wire).await?;
        if version != VERSION {
            return Err(invalid());
        }
        Ok(Self { wire })
    }
    pub async fn call(&mut self, operation: &Operation) -> Reply {
        if self
            .wire
            .send(encode(operation).map_err(|_| {
                Error::new(
                    "invalid_request",
                    "The request exceeds the local protocol limit.",
                )
            })?)
            .await
            .is_err()
        {
            return Err(Error::new(
                "instance_gone",
                "SQLN disconnected. List instances and request access again.",
            ));
        }
        receive(&mut self.wire).await.map_err(|_| {
            Error::new(
                "instance_gone",
                "SQLN disconnected. List instances and request access again.",
            )
        })?
    }
}

pub async fn discover(dir: &Path) -> io::Result<Vec<Instance>> {
    private_directory(dir)?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(dir)?.take(256) {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file()
            || meta.uid() != rustix::process::getuid().as_raw()
            || meta.mode() & 0o077 != 0
            || meta.len() > 65536
        {
            continue;
        }
        let Ok(mut file) = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&path)
        else {
            continue;
        };
        let mut data = Vec::new();
        Read::by_ref(&mut file).take(65536).read_to_end(&mut data)?;
        let Ok(instance) = serde_json::from_slice::<Instance>(&data) else {
            continue;
        };
        if instance.version != VERSION
            || instance.socket.parent() != Some(dir)
            || path.file_stem().and_then(|v| v.to_str()) != Some(&instance.instance_id)
        {
            continue;
        }
        if file.try_lock().is_ok() {
            let _ = fs::remove_file(&path);
            // Only remove the socket with this descriptor's UUID, never an arbitrary path.
            if instance.socket == socket_path(dir, &instance.instance_id) {
                let _ = fs::remove_file(&instance.socket);
            }
            continue;
        }
        candidates.push(instance);
    }
    let mut found = Vec::new();
    for (instance, live) in
        futures_util::future::join_all(candidates.into_iter().map(|instance| async move {
            let live = matches!(
                tokio::time::timeout(
                    Duration::from_millis(500),
                    Client::connect(&instance, "discovery")
                )
                .await,
                Ok(Ok(_))
            );
            (instance, live)
        }))
        .await
    {
        if live {
            found.push(instance);
        }
    }
    found.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    Ok(found)
}
