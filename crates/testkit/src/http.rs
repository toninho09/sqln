//! A fake HTTP server for the tests of HTTP clients. It answers one request on each
//! connection, and it keeps the full text of each request.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// The answer of the server: the status and the body.
pub type Answer = (u16, Vec<u8>);

pub struct HttpServer {
    /// The address of the server, such as `http://127.0.0.1:4000`, without a slash.
    pub url: String,
    pub port: u16,
    /// The full text of each request, in order: request line, headers, and body.
    pub requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HttpServer {
    /// Starts a server. `answer` receives the text of each request. The server waits
    /// `delay` before each answer.
    pub fn start(answer: impl Fn(&str) -> Answer + Send + 'static, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let (mut socket, _) = match listener.accept() {
                    Ok(accepted) => accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let request = read_request(&mut socket);
                if request.is_empty() {
                    continue;
                }
                let request = String::from_utf8(request).unwrap();
                let (status, body) = answer(&request);
                captured.lock().unwrap().push(request);
                std::thread::sleep(delay);
                let head = format!(
                    "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes());
                let _ = socket.write_all(&body);
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            port,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}

/// Reads the head and the body of the `Content-Length`.
fn read_request(socket: &mut impl Read) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = match socket.read(&mut buffer) {
            Ok(count) if count > 0 => count,
            _ => break,
        };
        request.extend_from_slice(&buffer[..count]);
        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&request[..end]);
            let size = head
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|size| size.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if request.len() >= end + 4 + size {
                break;
            }
        }
    }
    request
}

/// Returns the method and the path of a request text.
pub fn request_line(request: &str) -> (&str, &str) {
    let mut words = request.split(' ');
    (
        words.next().unwrap_or_default(),
        words.next().unwrap_or_default(),
    )
}

/// Returns the body of a request text.
pub fn body(request: &str) -> &str {
    request.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
