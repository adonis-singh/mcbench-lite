//! A fake provider for the ported tests: an HTTP/1.1 server on 127.0.0.1
//! that answers each `METHOD target` from a queue of scripted answers and
//! keeps every request it got. Its addresses are [`Endpoints::under`] its
//! origin, so `https://api.openai.com/v1/models` is `/api.openai.com/v1/models`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::api::Endpoints;

/// A piece of an answer's body.
#[derive(Clone, Debug)]
pub enum Chunk {
    /// Bytes written at once.
    Text(String),
    /// A pause.
    Wait(u64),
    /// The text written again and again until the client goes away.
    Forever(String),
}

/// A scripted answer.
#[derive(Clone, Debug)]
pub struct Answer {
    /// The status.
    pub status: u16,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub chunks: Vec<Chunk>,
}

impl Answer {
    /// An event stream of `body`, in one write.
    pub fn sse(body: &str) -> Answer {
        Answer {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            chunks: vec![Chunk::Text(body.into())],
        }
    }

    /// JSON with `status`.
    pub fn json(status: u16, body: &serde_json::Value) -> Answer {
        Answer {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            chunks: vec![Chunk::Text(mcbench_json::stringify(body))],
        }
    }

    /// A body of `text` with `status`.
    pub fn text(status: u16, text: &str) -> Answer {
        Answer {
            status,
            headers: Vec::new(),
            chunks: vec![Chunk::Text(text.into())],
        }
    }
}

/// A request the fake got.
#[derive(Clone, Debug)]
pub struct Seen {
    /// `GET`, `POST`.
    pub method: String,
    /// The path and query.
    pub target: String,
    /// The headers, names in lower case.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: String,
}

impl Seen {
    /// A header's value.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The body as JSON.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("a JSON body")
    }
}

/// The fake.
#[derive(Clone)]
pub struct Fake {
    /// `http://127.0.0.1:PORT`.
    pub origin: String,
    routes: Arc<Mutex<HashMap<String, VecDeque<Answer>>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
    /// How many answers ended because the client went away.
    pub dropped: Arc<Mutex<u32>>,
}

impl Fake {
    /// Listens on a free port of 127.0.0.1.
    pub async fn start() -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let fake = Fake {
            origin: format!("http://{}", listener.local_addr().unwrap()),
            routes: Arc::default(),
            seen: Arc::default(),
            dropped: Arc::default(),
        };
        let server = fake.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let server = server.clone();
                tokio::spawn(async move { server.serve(stream).await });
            }
        });
        fake
    }

    /// The addresses under the fake.
    pub fn endpoints(&self) -> Endpoints {
        Endpoints::under(&self.origin)
    }

    /// Queues `answer` for `route` (`"POST /api.openai.com/v1/chat/completions"`).
    pub fn on(&self, route: &str, answer: Answer) {
        self.routes
            .lock()
            .unwrap()
            .entry(route.into())
            .or_default()
            .push_back(answer);
    }

    /// The requests so far.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    async fn serve(&self, mut stream: TcpStream) {
        let mut buffer = Vec::new();
        let mut piece = [0u8; 8192];
        let head_end = loop {
            if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                break at;
            }
            match stream.read(&mut piece).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buffer.extend_from_slice(&piece[..n]),
            }
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
        let mut lines = head.split("\r\n");
        let mut request_line = lines.next().unwrap().split(' ');
        let method = request_line.next().unwrap().to_owned();
        let target = request_line.next().unwrap().to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
            .collect();
        let length: usize = headers
            .iter()
            .find(|(n, _)| n == "content-length")
            .map_or(0, |(_, v)| v.parse().unwrap());
        let mut body = buffer[head_end + 4..].to_vec();
        while body.len() < length {
            match stream.read(&mut piece).await {
                Ok(0) | Err(_) => return,
                Ok(n) => body.extend_from_slice(&piece[..n]),
            }
        }
        let route = format!("{method} {target}");
        self.seen.lock().unwrap().push(Seen {
            method,
            target,
            headers,
            body: String::from_utf8_lossy(&body).to_string(),
        });
        let answer = self
            .routes
            .lock()
            .unwrap()
            .get_mut(&route)
            .and_then(VecDeque::pop_front);
        let answer =
            answer.unwrap_or_else(|| Answer::text(599, &format!("no scripted answer for {route}")));
        let mut head = format!("HTTP/1.1 {} Scripted\r\n", answer.status);
        for (name, value) in &answer.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("connection: close\r\n\r\n");
        if stream.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        for chunk in answer.chunks {
            let written = match chunk {
                Chunk::Text(text) => stream.write_all(text.as_bytes()).await,
                Chunk::Wait(ms) => {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    Ok(())
                }
                Chunk::Forever(text) => loop {
                    if let Err(error) = stream.write_all(text.as_bytes()).await {
                        break Err(error);
                    }
                    let _ = stream.flush().await;
                },
            };
            if written.is_err() {
                *self.dropped.lock().unwrap() += 1;
                return;
            }
            let _ = stream.flush().await;
        }
        let _ = stream.shutdown().await;
    }
}
