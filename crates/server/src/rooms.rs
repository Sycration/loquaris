//! Collaboration rooms. Each open project is a room: the server's replica of
//! the Yjs document, its awareness (presence) states and the connected
//! peers. Rooms load on first use from storage and unload a while after the
//! last peer leaves.
//!
//! Every change is applied to the room's document, appended to the update
//! log, and only then broadcast, all under the room's lock, so the log order
//! is the order peers see and nothing a peer has received can be lost.
//!
//! Lock order: the room map before a room's state, never the reverse.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Context;
use sqlx::SqlitePool;
use tokio::sync::{Mutex, mpsc};
use yrs::encoding::read::Cursor;
use yrs::sync::{Awareness, Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{ClientID, Doc, ReadTxn, Transact, TransactionMut, Update};

use crate::config::Config;
use crate::db::Role;
use crate::util::now_ms;
use crate::{doc, storage, versions};

/// WebSocket close codes the browser can act on.
pub mod close {
    /// The server is restarting; reconnect (the provider does so itself).
    pub const RESTART: u16 = 1012;
    /// Something failed on the server, such as saving an update.
    pub const INTERNAL: u16 = 1011;
    /// The connection fell too far behind; reconnect.
    pub const TOO_SLOW: u16 = 1013;
    /// The user's role changed; refetch the project (role) and reconnect.
    pub const ROLE_CHANGED: u16 = 4001;
    /// A malformed message was received.
    pub const MALFORMED: u16 = 4400;
    /// Not signed in, or the session ended (signed out, revoked, disabled).
    pub const UNAUTHENTICATED: u16 = 4401;
    /// Not (or no longer) a member of the project.
    pub const FORBIDDEN: u16 = 4403;
    /// The project does not exist or was deleted.
    pub const NOT_FOUND: u16 = 4404;
    /// A viewer kept sending document updates.
    pub const READ_ONLY: u16 = 4405;
}

/// Updates a viewer may send (and have dropped) before being disconnected.
const VIEWER_STRIKES: u32 = 10;
/// Messages queued for a peer before it is considered too slow.
const PEER_QUEUE: usize = 1024;

/// Sent to a connection's writer task.
#[derive(Debug)]
pub enum Outbound {
    Binary(Vec<u8>),
    Close(u16, String),
}

/// What [`Rooms::disconnect`] can select peers by.
pub struct PeerInfo<'a> {
    pub user_id: &'a str,
    pub session_id: &'a str,
}

struct Peer {
    user_id: String,
    session_id: String,
    role: Role,
    tx: mpsc::Sender<Outbound>,
    /// Awareness client IDs this connection has announced.
    clients: HashSet<ClientID>,
    strikes: u32,
}

pub struct Room {
    pub project_id: String,
    state: Mutex<RoomState>,
}

struct RoomState {
    awareness: Awareness,
    peers: HashMap<u64, Peer>,
    /// Sequence number of the last persisted update.
    seq: i64,
    since_snapshot: u32,
    /// Set once the room is unloaded; a late joiner then loads a new one.
    closed: bool,
    /// Bumped whenever the room becomes idle, so stale unload timers are
    /// ignored.
    idle_generation: u64,
    /// `projects.name` as last written.
    listed_name: String,
    /// Edits not yet reflected in `projects.updated_at`.
    touched: bool,
    /// Edits since the last automatic version.
    unversioned: bool,
    last_version_ms: i64,
}

impl RoomState {
    fn doc(&self) -> &Doc {
        self.awareness.doc()
    }

    fn send(&mut self, conn: u64, message: Vec<u8>) {
        if let Some(peer) = self.peers.get(&conn)
            && peer.tx.try_send(Outbound::Binary(message)).is_err()
        {
            self.drop_peers(&[conn]);
        }
    }

    /// Sends `message` to every peer except `except`.
    fn broadcast(&mut self, message: &[u8], except: Option<u64>) {
        let slow: Vec<u64> = self
            .peers
            .iter()
            .filter(|(conn, _)| Some(**conn) != except)
            .filter(|(_, peer)| peer.tx.try_send(Outbound::Binary(message.to_vec())).is_err())
            .map(|(conn, _)| *conn)
            .collect();
        self.drop_peers(&slow);
    }

    /// Removes peers (closing their queues ends their connections), clears
    /// their presence and tells the others.
    fn drop_peers(&mut self, conns: &[u64]) {
        let mut gone = Vec::new();
        for conn in conns {
            if let Some(peer) = self.peers.remove(conn) {
                gone.extend(peer.clients);
            }
        }
        if gone.is_empty() {
            return;
        }
        for client in &gone {
            self.awareness.remove_state(*client);
        }
        if let Ok(update) = self.awareness.update_with_clients(gone) {
            let message = Message::Awareness(update).encode_v1();
            self.broadcast(&message, None);
        }
    }

    fn close_peers(&mut self, conns: &[u64], code: u16, reason: &str) {
        for conn in conns {
            if let Some(peer) = self.peers.get(conn) {
                let _ = peer.tx.try_send(Outbound::Close(code, reason.to_owned()));
            }
        }
        self.drop_peers(conns);
    }
}

/// Why a connection must end.
#[derive(Debug)]
pub struct CloseReason(pub u16, pub String);

impl CloseReason {
    fn new(code: u16, reason: impl Into<String>) -> Self {
        Self(code, reason.into())
    }
}

/// Every loaded room.
pub struct Rooms {
    db: SqlitePool,
    config: Arc<Config>,
    rooms: Mutex<HashMap<String, Arc<Room>>>,
    next_conn: AtomicU64,
    shutting_down: AtomicBool,
}

/// A peer's handle on a room after joining.
pub struct Joined {
    pub room: Arc<Room>,
    pub conn: u64,
    pub rx: mpsc::Receiver<Outbound>,
}

impl Rooms {
    pub fn new(db: SqlitePool, config: Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            rooms: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
            shutting_down: AtomicBool::new(false),
        })
    }

    /// The loaded room of a project, loading it if needed.
    async fn room(self: &Arc<Self>, project_id: &str) -> anyhow::Result<Arc<Room>> {
        let mut rooms = self.rooms.lock().await;
        if let Some(room) = rooms.get(project_id) {
            return Ok(room.clone());
        }
        let stored = storage::load(&self.db, project_id).await?;
        let (listed_name, last_version): (String, Option<i64>) = sqlx::query_as(
            "SELECT name, (SELECT MAX(created_at) FROM versions WHERE project_id = projects.id)
             FROM projects WHERE id = ?",
        )
        .bind(project_id)
        .fetch_one(&self.db)
        .await
        .context("project row missing")?;
        let room = Arc::new(Room {
            project_id: project_id.to_owned(),
            state: Mutex::new(RoomState {
                awareness: Awareness::new(stored.doc),
                peers: HashMap::new(),
                seq: stored.seq,
                since_snapshot: stored.since_snapshot,
                closed: false,
                idle_generation: 0,
                listed_name,
                touched: false,
                unversioned: false,
                last_version_ms: last_version.unwrap_or(0),
            }),
        });
        rooms.insert(project_id.to_owned(), room.clone());
        tracing::debug!(project = project_id, seq = stored.seq, "room loaded");
        tokio::spawn(self.clone().tick(room.clone()));
        Ok(room)
    }

    async fn loaded(&self, project_id: &str) -> Option<Arc<Room>> {
        self.rooms.lock().await.get(project_id).cloned()
    }

    /// Adds a connection to a project's room and queues the opening messages
    /// (sync step 1 and the current presence states).
    pub async fn join(
        self: &Arc<Self>,
        project_id: &str,
        user_id: &str,
        session_id: &str,
        role: Role,
    ) -> anyhow::Result<Joined> {
        loop {
            anyhow::ensure!(!self.shutting_down.load(Ordering::SeqCst), "server shutting down");
            let room = self.room(project_id).await?;
            let mut state = room.state.lock().await;
            if state.closed {
                continue;
            }
            let conn = self.next_conn.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = mpsc::channel(PEER_QUEUE);
            let step1 = Message::Sync(SyncMessage::SyncStep1(state.doc().transact().state_vector())).encode_v1();
            let _ = tx.try_send(Outbound::Binary(step1));
            if let Ok(update) = state.awareness.update()
                && !update.clients.is_empty()
            {
                let _ = tx.try_send(Outbound::Binary(Message::Awareness(update).encode_v1()));
            }
            state.peers.insert(
                conn,
                Peer {
                    user_id: user_id.to_owned(),
                    session_id: session_id.to_owned(),
                    role,
                    tx,
                    clients: HashSet::new(),
                    strikes: 0,
                },
            );
            state.idle_generation += 1;
            drop(state);
            return Ok(Joined { room, conn, rx });
        }
    }

    /// Removes a connection; unloads the room later if it was the last.
    pub async fn leave(self: &Arc<Self>, room: &Arc<Room>, conn: u64) {
        let mut state = room.state.lock().await;
        state.drop_peers(&[conn]);
        if state.peers.is_empty() && !state.closed {
            self.schedule_unload(room, &mut state);
        }
    }

    fn schedule_unload(self: &Arc<Self>, room: &Arc<Room>, state: &mut RoomState) {
        state.idle_generation += 1;
        let generation = state.idle_generation;
        let (rooms, project_id) = (self.clone(), room.project_id.clone());
        tokio::spawn(async move {
            tokio::time::sleep(rooms.config.room_idle).await;
            rooms.unload(&project_id, Some(generation)).await;
        });
    }

    /// Flushes and forgets a room: if `generation` is given, only if it is
    /// still idle since then.
    async fn unload(&self, project_id: &str, generation: Option<u64>) {
        let mut rooms = self.rooms.lock().await;
        let Some(room) = rooms.get(project_id).cloned() else { return };
        let mut state = room.state.lock().await;
        if generation.is_some_and(|g| g != state.idle_generation || !state.peers.is_empty()) {
            return;
        }
        let conns: Vec<u64> = state.peers.keys().copied().collect();
        state.close_peers(&conns, close::NOT_FOUND, "Project closed");
        if let Err(error) = self.flush(&room.project_id, &mut state, true).await {
            tracing::error!(project = project_id, "flushing room failed: {error:#}");
        }
        state.closed = true;
        rooms.remove(project_id);
        tracing::debug!(project = project_id, "room unloaded");
    }

    /// Periodic bookkeeping of a loaded room until it unloads.
    async fn tick(self: Arc<Self>, room: Arc<Room>) {
        loop {
            tokio::time::sleep(self.config.room_tick).await;
            let mut state = room.state.lock().await;
            if state.closed {
                return;
            }
            if let Err(error) = self.flush(&room.project_id, &mut state, false).await {
                tracing::error!(project = room.project_id, "room bookkeeping failed: {error:#}");
            }
        }
    }

    /// Mirrors `meta.name` and the edit time into `projects`, takes an
    /// automatic version when due, and (when `compact`) snapshots the log.
    async fn flush(&self, project_id: &str, state: &mut RoomState, compact: bool) -> anyhow::Result<()> {
        let name = doc::read_name(&state.doc().transact());
        if name != state.listed_name {
            sqlx::query("UPDATE projects SET name = ? WHERE id = ?")
                .bind(&name)
                .bind(project_id)
                .execute(&self.db)
                .await?;
            state.listed_name = name;
        }
        if state.touched {
            sqlx::query("UPDATE projects SET updated_at = ? WHERE id = ?")
                .bind(now_ms())
                .bind(project_id)
                .execute(&self.db)
                .await?;
            state.touched = false;
        }
        let interval = self.config.auto_version_every.as_millis() as i64;
        if state.unversioned && now_ms() - state.last_version_ms >= interval {
            let data = doc::encode_state(state.doc());
            versions::insert(&self.db, project_id, None, true, &data, None).await?;
            state.last_version_ms = now_ms();
            state.unversioned = false;
        }
        if compact && state.since_snapshot > 0 {
            self.compact(project_id, state).await?;
        }
        Ok(())
    }

    async fn compact(&self, project_id: &str, state: &mut RoomState) -> sqlx::Result<()> {
        let data = doc::encode_state(state.doc());
        storage::compact(&self.db, project_id, state.seq, &data).await?;
        state.since_snapshot = 0;
        Ok(())
    }

    /// Persists an update already applied to the room's document, then sends
    /// it to every peer but its author.
    async fn commit(
        &self,
        project_id: &str,
        state: &mut RoomState,
        update: Vec<u8>,
        user_id: Option<&str>,
        author: Option<u64>,
    ) -> anyhow::Result<()> {
        storage::append(&self.db, project_id, state.seq + 1, &update, user_id).await?;
        state.seq += 1;
        state.since_snapshot += 1;
        state.touched = true;
        state.unversioned = true;
        let message = Message::Sync(SyncMessage::Update(update)).encode_v1();
        state.broadcast(&message, author);
        if state.since_snapshot >= self.config.compact_every {
            self.compact(project_id, state).await?;
        }
        Ok(())
    }

    /// Handles one binary WebSocket message from a peer.
    pub async fn receive(&self, room: &Room, conn: u64, data: &[u8]) -> Result<(), CloseReason> {
        let mut state = room.state.lock().await;
        let Some(peer) = state.peers.get(&conn) else {
            // Dropped meanwhile (removed, too slow, room closed).
            return Err(CloseReason::new(close::FORBIDDEN, "Disconnected"));
        };
        let (user_id, can_edit) = (peer.user_id.clone(), peer.role >= Role::Editor);
        let mut decoder = DecoderV1::new(Cursor::new(data));
        let messages: Vec<Message> = MessageReader::new(&mut decoder)
            .collect::<Result<_, _>>()
            .map_err(|_| CloseReason::new(close::MALFORMED, "Malformed message"))?;
        // The reader stops quietly at a truncated message; the decoded
        // messages re-encode to the same length only if nothing was left.
        if messages.iter().map(|message| message.encode_v1().len()).sum::<usize>() != data.len() {
            return Err(CloseReason::new(close::MALFORMED, "Malformed message"));
        }
        for message in messages {
            match message {
                Message::Sync(SyncMessage::SyncStep1(sv)) => {
                    let diff = state.doc().transact().encode_state_as_update_v1(&sv);
                    state.send(conn, Message::Sync(SyncMessage::SyncStep2(diff)).encode_v1());
                }
                Message::Sync(SyncMessage::SyncStep2(update) | SyncMessage::Update(update)) if !can_edit => {
                    // A viewer's replica may hold stale local changes; drop
                    // them, but end a connection that keeps editing.
                    if !is_noop(&update)
                        && let Some(peer) = state.peers.get_mut(&conn)
                    {
                        peer.strikes += 1;
                        if peer.strikes > VIEWER_STRIKES {
                            return Err(CloseReason::new(close::READ_ONLY, "Viewers cannot edit"));
                        }
                    }
                }
                Message::Sync(SyncMessage::SyncStep2(update) | SyncMessage::Update(update)) => {
                    let applied = apply(state.doc(), &update, conn)
                        .map_err(|_| CloseReason::new(close::MALFORMED, "Malformed update"))?;
                    if let Some(applied) = applied {
                        self.commit(&room.project_id, &mut state, applied, Some(&user_id), Some(conn))
                            .await
                            .map_err(|error| {
                                tracing::error!(project = room.project_id, "saving update failed: {error:#}");
                                CloseReason::new(close::INTERNAL, "Saving failed")
                            })?;
                    }
                }
                Message::Awareness(update) => {
                    let message = Message::Awareness(update.clone()).encode_v1();
                    if let Some(peer) = state.peers.get_mut(&conn) {
                        for (client, entry) in &update.clients {
                            if &*entry.json == "null" {
                                peer.clients.remove(client);
                            } else {
                                peer.clients.insert(*client);
                            }
                        }
                    }
                    state
                        .awareness
                        .apply_update(update)
                        .map_err(|_| CloseReason::new(close::MALFORMED, "Malformed awareness update"))?;
                    state.broadcast(&message, Some(conn));
                }
                Message::AwarenessQuery => {
                    if let Ok(update) = state.awareness.update() {
                        state.send(conn, Message::Awareness(update).encode_v1());
                    }
                }
                Message::Auth(_) | Message::Custom(..) => {}
            }
        }
        Ok(())
    }

    /// Runs `change` on the project's document as a server transaction and
    /// persists and broadcasts the result (version restore, rename).
    pub async fn change<R>(
        self: &Arc<Self>,
        project_id: &str,
        user_id: Option<&str>,
        change: impl FnOnce(&mut TransactionMut) -> R,
    ) -> anyhow::Result<R> {
        let mut change = Some(change);
        loop {
            let room = self.room(project_id).await?;
            let mut state = room.state.lock().await;
            if state.closed {
                continue;
            }
            let change = change.take().expect("runs once");
            let (result, update) = {
                let mut txn = state.doc().transact_mut_with("server");
                let result = change(&mut txn);
                txn.commit();
                (result, txn.has_changes().then(|| txn.encode_update_v1()))
            };
            if let Some(update) = update {
                self.commit(project_id, &mut state, update, user_id, None).await?;
            }
            if state.peers.is_empty() {
                self.schedule_unload(&room, &mut state);
            }
            return Ok(result);
        }
    }

    /// Reads the project's current document: the room's if loaded, else
    /// straight from storage (without loading a room).
    pub async fn read<R>(&self, project_id: &str, read: impl FnOnce(&Doc) -> R) -> anyhow::Result<R> {
        if let Some(room) = self.loaded(project_id).await {
            let state = room.state.lock().await;
            if !state.closed {
                return Ok(read(state.doc()));
            }
        }
        let stored = storage::load(&self.db, project_id).await?;
        Ok(read(&stored.doc))
    }

    /// Applies a membership change to live connections: a removed member is
    /// disconnected; a member who gains or loses the right to edit is asked
    /// to reconnect (other role changes apply in place).
    pub async fn set_role(&self, project_id: &str, user_id: &str, role: Option<Role>) {
        let Some(room) = self.loaded(project_id).await else { return };
        let mut state = room.state.lock().await;
        let mut changed = Vec::new();
        for (conn, peer) in state.peers.iter_mut().filter(|(_, peer)| peer.user_id == user_id) {
            match role {
                Some(role) if (role >= Role::Editor) == (peer.role >= Role::Editor) => peer.role = role,
                _ => changed.push(*conn),
            }
        }
        match role {
            None => state.close_peers(&changed, close::FORBIDDEN, "Removed from the project"),
            Some(_) => state.close_peers(&changed, close::ROLE_CHANGED, "Role changed"),
        }
    }

    /// Closes one connection with the given code.
    pub async fn disconnect_conn(&self, room: &Room, conn: u64, code: u16, reason: &str) {
        room.state.lock().await.close_peers(&[conn], code, reason);
    }

    /// Disconnects matching peers in every room (sign-out, disabled user).
    pub async fn disconnect(&self, select: impl Fn(&PeerInfo) -> bool, code: u16, reason: &str) {
        let rooms: Vec<Arc<Room>> = self.rooms.lock().await.values().cloned().collect();
        for room in rooms {
            let mut state = room.state.lock().await;
            let conns: Vec<u64> = state
                .peers
                .iter()
                .filter(|(_, peer)| {
                    select(&PeerInfo {
                        user_id: &peer.user_id,
                        session_id: &peer.session_id,
                    })
                })
                .map(|(conn, _)| *conn)
                .collect();
            state.close_peers(&conns, code, reason);
        }
    }

    /// Disconnects everyone from a deleted project and unloads its room.
    pub async fn close_project(&self, project_id: &str) {
        self.unload(project_id, None).await;
    }

    /// Closes every connection (asking clients to reconnect later) and
    /// flushes every room. Called on graceful shutdown.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let mut rooms = self.rooms.lock().await;
        for (project_id, room) in rooms.drain() {
            let mut state = room.state.lock().await;
            let conns: Vec<u64> = state.peers.keys().copied().collect();
            state.close_peers(&conns, close::RESTART, "Server restarting");
            if let Err(error) = self.flush(&project_id, &mut state, true).await {
                tracing::error!(project = project_id, "flushing room failed: {error:#}");
            }
            state.closed = true;
        }
    }
}

/// Applies a peer's update; returns the update to persist and broadcast
/// (what actually changed), or `None` if it changed nothing.
fn apply(doc: &Doc, update: &[u8], conn: u64) -> anyhow::Result<Option<Vec<u8>>> {
    let update = Update::decode_v1(update)?;
    let mut txn = doc.transact_mut_with(conn);
    txn.apply_update(update)?;
    txn.commit();
    Ok(txn.has_changes().then(|| txn.encode_update_v1()))
}

/// True for an update that carries nothing (what a replica without local
/// changes answers sync step 1 with).
fn is_noop(update: &[u8]) -> bool {
    Update::decode_v1(update).is_ok_and(|update| update.is_empty())
}
