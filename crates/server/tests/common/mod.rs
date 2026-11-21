//! Test harness: a server on an ephemeral port with a temporary data
//! directory, an HTTP client with a cookie jar, and a minimal Yjs client
//! speaking the y-websocket protocol through yrs.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use loquaris_server::config::Config;
use loquaris_server::{AppState, serve};
use reqwest::cookie::{CookieStore, Jar};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message as WsMessage};
use yrs::encoding::read::Cursor;
use yrs::sync::{Awareness, Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, Map, MapRef, Out, ReadTxn, Text, TextRef, Transact, Update};

pub struct TestServer {
    pub addr: SocketAddr,
    pub state: AppState,
    shutdown: Option<oneshot::Sender<()>>,
    handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl TestServer {
    pub async fn start(dir: &Path) -> Self {
        Self::start_with(dir, |_| {}).await
    }

    pub async fn start_with(dir: &Path, tweak: impl FnOnce(&mut Config)) -> Self {
        let mut config = Config::new(dir.to_path_buf());
        config.room_idle = Duration::from_millis(300);
        config.room_tick = Duration::from_millis(50);
        tweak(&mut config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        config.public_url = format!("http://{addr}");
        let state = AppState::open(config).await.unwrap();
        let (tx, rx) = oneshot::channel();
        let handle = tokio::spawn(serve(listener, state.clone(), async {
            let _ = rx.await;
        }));
        Self {
            addr,
            state,
            shutdown: Some(tx),
            handle,
        }
    }

    pub async fn stop(mut self) {
        let _ = self.shutdown.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server did not stop")
            .unwrap()
            .unwrap();
    }

    pub fn client(&self) -> Client {
        Client::new(self.addr)
    }
}

pub struct Client {
    pub addr: SocketAddr,
    pub http: reqwest::Client,
    jar: Arc<Jar>,
}

impl Client {
    pub fn new(addr: SocketAddr) -> Self {
        let jar = Arc::new(Jar::default());
        let http = reqwest::Client::builder().cookie_provider(jar.clone()).build().unwrap();
        Self { addr, http, jar }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// The `Cookie` header the jar would send.
    pub fn cookie(&self) -> Option<String> {
        let url = self.url("/").parse().unwrap();
        self.jar.cookies(&url).map(|value| value.to_str().unwrap().to_owned())
    }

    pub fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, self.url(path)).header("X-Loquaris", "1")
    }

    pub async fn send(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = self.request(method, path);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(Method::POST, path, Some(body)).await
    }

    /// Registers and signs in; returns the user.
    pub async fn register(&self, email: &str, name: &str) -> Value {
        let (status, user) = self
            .post(
                "/api/auth/register",
                json!({"email": email, "displayName": name, "password": "correct horse"}),
            )
            .await;
        assert!(status.is_success(), "register {email}: {status} {user}");
        user
    }

    pub async fn create_project(&self, name: &str) -> String {
        let (status, project) = self.post("/api/projects", json!({"name": name, "l1Name": "English"})).await;
        assert!(status.is_success(), "{status} {project}");
        project["id"].as_str().unwrap().to_owned()
    }

    pub async fn add_member(&self, project: &str, email: &str, role: &str) {
        let (status, body) = self
            .post(&format!("/api/projects/{project}/members"), json!({"email": email, "role": role}))
            .await;
        assert!(status.is_success(), "{status} {body}");
    }

    pub async fn connect(&self, project: &str) -> YClient {
        YClient::connect(self.addr, project, self.cookie()).await
    }
}

/// A Yjs replica connected to a project's room.
pub struct YClient {
    pub doc: Doc,
    pub awareness_seen: Arc<Mutex<Vec<String>>>,
    outgoing: mpsc::UnboundedSender<WsMessage>,
    /// The close frame received, if any.
    pub closed: Arc<Mutex<Option<(u16, String)>>>,
    pub synced: Arc<Mutex<bool>>,
}

impl YClient {
    pub async fn connect(addr: SocketAddr, project: &str, cookie: Option<String>) -> Self {
        Self::connect_with(addr, project, cookie, None).await.unwrap()
    }

    pub async fn connect_with(
        addr: SocketAddr,
        project: &str,
        cookie: Option<String>,
        origin: Option<&str>,
    ) -> Result<Self, tungstenite::Error> {
        let mut request = format!("ws://{addr}/api/projects/{project}/sync").into_client_request().unwrap();
        if let Some(cookie) = cookie {
            request.headers_mut().insert("cookie", cookie.parse().unwrap());
        }
        if let Some(origin) = origin {
            request.headers_mut().insert("origin", origin.parse().unwrap());
        }
        let (socket, _) = tokio_tungstenite::connect_async(request).await?;
        let (mut sink, mut stream) = socket.split();
        let doc = Doc::new();
        let (outgoing, mut queue) = mpsc::unbounded_channel::<WsMessage>();
        tokio::spawn(async move {
            while let Some(message) = queue.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
        });

        // Local changes go to the server; remote ones are applied with the
        // "remote" origin and not echoed.
        let tx = outgoing.clone();
        doc.observe_update_v1("send", move |txn, event| {
            if txn.origin().is_some_and(|origin| origin.as_ref() == b"remote") {
                return;
            }
            let message = Message::Sync(SyncMessage::Update(event.update.clone())).encode_v1();
            let _ = tx.send(WsMessage::Binary(message.into()));
        })
        .unwrap();

        let closed = Arc::new(Mutex::new(None));
        let synced = Arc::new(Mutex::new(false));
        let awareness_seen = Arc::new(Mutex::new(Vec::new()));
        let step1 = Message::Sync(SyncMessage::SyncStep1(doc.transact().state_vector())).encode_v1();
        outgoing.send(WsMessage::Binary(step1.into())).unwrap();

        let (doc2, tx, closed2, synced2, seen2) =
            (doc.clone(), outgoing.clone(), closed.clone(), synced.clone(), awareness_seen.clone());
        tokio::spawn(async move {
            while let Some(message) = stream.next().await {
                match message {
                    Ok(WsMessage::Binary(data)) => {
                        let mut decoder = DecoderV1::new(Cursor::new(&data));
                        for message in MessageReader::new(&mut decoder) {
                            match message.unwrap() {
                                Message::Sync(SyncMessage::SyncStep1(sv)) => {
                                    let diff = doc2.transact().encode_state_as_update_v1(&sv);
                                    let reply = Message::Sync(SyncMessage::SyncStep2(diff)).encode_v1();
                                    let _ = tx.send(WsMessage::Binary(reply.into()));
                                }
                                Message::Sync(SyncMessage::SyncStep2(update)) => {
                                    apply_remote(&doc2, &update);
                                    *synced2.lock().unwrap() = true;
                                }
                                Message::Sync(SyncMessage::Update(update)) => apply_remote(&doc2, &update),
                                Message::Awareness(update) => {
                                    let mut seen = seen2.lock().unwrap();
                                    seen.extend(update.clients.values().map(|entry| entry.json.to_string()));
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(WsMessage::Close(frame)) => {
                        *closed2.lock().unwrap() =
                            Some(frame.map_or((1005, String::new()), |f| (f.code.into(), f.reason.to_string())));
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            doc,
            awareness_seen,
            outgoing,
            closed,
            synced,
        })
    }

    pub async fn wait_synced(&self) {
        eventually("initial sync", || *self.synced.lock().unwrap()).await;
    }

    pub async fn wait_closed(&self) -> (u16, String) {
        eventually("connection closed", || self.closed.lock().unwrap().is_some()).await;
        self.closed.lock().unwrap().clone().unwrap()
    }

    /// Announces a presence state.
    pub fn set_awareness(&self, state: &str) {
        let mut awareness = Awareness::new(self.doc.clone());
        awareness.set_local_state_raw(state);
        let message = Message::Awareness(awareness.update().unwrap()).encode_v1();
        self.outgoing.send(WsMessage::Binary(message.into())).unwrap();
    }

    pub fn send_raw(&self, bytes: Vec<u8>) {
        let _ = self.outgoing.send(WsMessage::Binary(bytes.into()));
    }

    fn entries(&self) -> Option<MapRef> {
        self.doc.transact().get_map("entries")
    }

    pub fn entry_ids(&self) -> Vec<String> {
        let txn = self.doc.transact();
        txn.get_map("entries").map_or_else(Vec::new, |map| map.keys(&txn).map(str::to_owned).collect())
    }

    pub fn lesson_ids(&self) -> Vec<String> {
        let txn = self.doc.transact();
        txn.get_map("lessons").map_or_else(Vec::new, |map| map.keys(&txn).map(str::to_owned).collect())
    }

    fn entry_text(&self, entry: &str) -> Option<TextRef> {
        let txn = self.doc.transact();
        let entries = txn.get_map("entries")?;
        let Some(Out::YMap(entry)) = entries.get(&txn, entry) else { return None };
        match entry.get(&txn, "text") {
            Some(Out::YText(text)) => Some(text),
            _ => None,
        }
    }

    pub fn text(&self, entry: &str) -> Option<String> {
        let text = self.entry_text(entry)?;
        Some(text.get_string(&self.doc.transact()))
    }

    pub fn meta_name(&self) -> String {
        let txn = self.doc.transact();
        let meta = txn.get_map("meta").unwrap();
        match meta.get(&txn, "name") {
            Some(Out::Any(yrs::Any::String(name))) => name.to_string(),
            _ => String::new(),
        }
    }

    pub fn set_meta_name(&self, name: &str) {
        let meta = self.doc.transact().get_map("meta").unwrap();
        meta.insert(&mut self.doc.transact_mut(), "name", name);
    }

    /// Adds an entry (as the browser does) to the first lesson; returns its ID.
    pub fn add_entry(&self, id: &str, text: &str) {
        let lesson = self.lesson_ids()[0].clone();
        let entries = self.entries().unwrap_or_else(|| self.doc.get_or_insert_map("entries"));
        let mut txn = self.doc.transact_mut();
        let entry = entries.insert(&mut txn, id, yrs::MapPrelim::default());
        entry.insert(&mut txn, "lesson", lesson.as_str());
        entry.insert(&mut txn, "order", "a0");
        entry.insert(&mut txn, "participant", "teacher");
        entry.insert(&mut txn, "language", "latin");
        entry.insert(&mut txn, "text", yrs::TextPrelim::new(text));
        entry.insert(&mut txn, "audio", yrs::Any::Null);
    }

    pub fn insert_text(&self, entry: &str, index: u32, chunk: &str) {
        let text = self.entry_text(entry).expect("entry exists");
        text.insert(&mut self.doc.transact_mut(), index, chunk);
    }
}

fn apply_remote(doc: &Doc, update: &[u8]) {
    let update = Update::decode_v1(update).unwrap();
    doc.transact_mut_with("remote").apply_update(update).unwrap();
}

/// Polls `condition` for up to five seconds.
pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..250 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

/// A 16-bit PCM WAV file.
pub fn wav(sample_rate: u32, channels: u16, frames: u32) -> Vec<u8> {
    let data_len = frames * u32::from(channels) * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames * u32::from(channels) {
        bytes.extend_from_slice(&(((i * 37) % 2000) as i16 - 1000).to_le_bytes());
    }
    bytes
}
