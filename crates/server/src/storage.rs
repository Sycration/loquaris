//! Document persistence: an append-only log of Yjs updates (`doc_updates`)
//! compacted into full-state snapshots (`doc_snapshots`). A document is the
//! latest snapshot plus every later update.
use anyhow::Context;
use sqlx::{SqliteConnection, SqlitePool};
use yrs::Doc;

use crate::doc;
use crate::util::now_ms;

/// A document read from storage.
pub struct Stored {
    pub doc: Doc,
    /// The sequence number of the last update included.
    pub seq: i64,
    /// How many updates follow the snapshot.
    pub since_snapshot: u32,
}

pub async fn load(pool: &SqlitePool, project_id: &str) -> anyhow::Result<Stored> {
    let snapshot: Option<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT seq, data FROM doc_snapshots WHERE project_id = ? ORDER BY seq DESC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    let (snapshot_seq, snapshot) = snapshot.unwrap_or_default();
    let updates: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT seq, data FROM doc_updates WHERE project_id = ? AND seq > ? ORDER BY seq",
    )
    .bind(project_id)
    .bind(snapshot_seq)
    .fetch_all(pool)
    .await?;
    let seq = updates.last().map_or(snapshot_seq, |(seq, _)| *seq);
    let since_snapshot = updates.len() as u32;
    let parts = (!snapshot.is_empty())
        .then_some(snapshot.as_slice())
        .into_iter()
        .chain(updates.iter().map(|(_, data)| data.as_slice()));
    let doc = doc::decode(parts).with_context(|| format!("cannot load project {project_id}"))?;
    Ok(Stored {
        doc,
        seq,
        since_snapshot,
    })
}

/// Appends one update to the log.
pub async fn append(
    pool: &SqlitePool,
    project_id: &str,
    seq: i64,
    data: &[u8],
    user_id: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO doc_updates (project_id, seq, data, user_id, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(project_id)
    .bind(seq)
    .bind(data)
    .bind(user_id)
    .bind(now_ms())
    .execute(pool)
    .await?;
    Ok(())
}

/// Replaces the log up to `seq` with a snapshot of the full state.
pub async fn compact(pool: &SqlitePool, project_id: &str, seq: i64, state: &[u8]) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    write_snapshot(&mut tx, project_id, seq, state).await?;
    sqlx::query("DELETE FROM doc_updates WHERE project_id = ? AND seq <= ?")
        .bind(project_id)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM doc_snapshots WHERE project_id = ? AND seq < ?")
        .bind(project_id)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

/// Stores the first state of a new project.
pub async fn write_snapshot(
    conn: &mut SqliteConnection,
    project_id: &str,
    seq: i64,
    state: &[u8],
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO doc_snapshots (project_id, seq, data, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(project_id)
    .bind(seq)
    .bind(state)
    .bind(now_ms())
    .execute(conn)
    .await?;
    Ok(())
}
