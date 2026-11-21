//! Real-time collaboration over the y-websocket protocol: convergence,
//! persistence, roles on live connections, versions.
mod common;

use std::time::Duration;

use common::*;
use reqwest::{Method, StatusCode};
use serde_json::json;
use yrs::sync::{Message, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{Doc, Map, ReadTxn, Text, Transact, WriteTxn};

/// Ann owns a project; Bob edits and Cy views it.
async fn setup(server: &TestServer) -> (Client, Client, Client, String) {
    let ann = server.client();
    ann.register("ann@example.org", "Ann").await;
    let bob = server.client();
    bob.register("bob@example.org", "Bob").await;
    let cy = server.client();
    cy.register("cy@example.org", "Cy").await;
    let id = ann.create_project("Latin I").await;
    ann.add_member(&id, "bob@example.org", "editor").await;
    ann.add_member(&id, "cy@example.org", "viewer").await;
    (ann, bob, cy, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_edits_converge() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let (ann, bob, _, id) = setup(&server).await;

    let a = ann.connect(&id).await;
    a.wait_synced().await;
    assert_eq!(a.meta_name(), "Latin I");
    assert_eq!(a.lesson_ids().len(), 1, "a new project has one lesson");
    a.add_entry("entry00000000001", "Salvē!");

    let b = bob.connect(&id).await;
    b.wait_synced().await;
    eventually("Bob sees the entry", || b.text("entry00000000001").as_deref() == Some("Salvē!")).await;

    // Both type into the same text at once.
    for i in 0..20 {
        a.insert_text("entry00000000001", 0, &format!("a{i} "));
        b.insert_text("entry00000000001", b.text("entry00000000001").unwrap().len() as u32, &format!(" b{i}"));
    }
    eventually("convergence", || {
        let (ta, tb) = (a.text("entry00000000001").unwrap(), b.text("entry00000000001").unwrap());
        ta == tb && ta.matches("a").count() >= 20 && ta.matches(" b").count() == 20
    })
    .await;

    // Presence reaches the other peer.
    a.set_awareness(r#"{"user":{"name":"Ann"}}"#);
    eventually("awareness", || b.awareness_seen.lock().unwrap().iter().any(|s| s.contains("Ann"))).await;

    // A late joiner gets the merged state from the server.
    let c = bob.connect(&id).await;
    c.wait_synced().await;
    assert_eq!(c.text("entry00000000001"), a.text("entry00000000001"));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn persists_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    // Compact every few updates, so both snapshots and the log are used.
    let server = TestServer::start_with(dir.path(), |config| config.compact_every = 7).await;
    let (ann, _, _, id) = setup(&server).await;
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    a.add_entry("entry00000000001", "");
    for i in 0..30 {
        a.insert_text("entry00000000001", i, "x");
    }
    // Wait until the server has it all (a second replica sees it).
    let probe = ann.connect(&id).await;
    eventually("server has the edits", || probe.text("entry00000000001").is_some_and(|t| t.len() == 30)).await;
    server.stop().await;
    assert_eq!(a.wait_closed().await.0, 1012, "clients are told the server restarts");

    let server = TestServer::start(dir.path()).await;
    let ann = server.client();
    let (status, _) = ann
        .post("/api/auth/login", json!({"email": "ann@example.org", "password": "correct horse"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    assert_eq!(a.text("entry00000000001").unwrap(), "x".repeat(30));
    server.stop().await;

    // Even without a graceful shutdown nothing is lost: every update is in
    // the log before it is broadcast.
    let server = TestServer::start(dir.path()).await;
    let ann = server.client();
    ann.post("/api/auth/login", json!({"email": "ann@example.org", "password": "correct horse"})).await;
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    a.insert_text("entry00000000001", 0, "crash-safe ");
    let probe = ann.connect(&id).await;
    eventually("server has it", || probe.text("entry00000000001").is_some_and(|t| t.starts_with("crash"))).await;
    let stored = loquaris_server::storage::load(&server.state.db, &id).await.unwrap();
    let text = {
        let txn = stored.doc.transact();
        let project = loquaris_server::doc::read_project(&txn);
        project.lessons[0].entries[0].text.clone()
    };
    assert!(text.starts_with("crash-safe x"));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn viewers_cannot_edit() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let (ann, _, cy, id) = setup(&server).await;
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    a.add_entry("entry00000000001", "Salvē");

    let v = cy.connect(&id).await;
    v.wait_synced().await;
    eventually("viewer receives edits", || v.text("entry00000000001").as_deref() == Some("Salvē")).await;

    // The viewer's local edit is dropped by the server.
    v.insert_text("entry00000000001", 0, "HACK ");
    a.insert_text("entry00000000001", "Salvē".len() as u32, "!");
    eventually("owner's edit arrives", || v.text("entry00000000001").is_some_and(|t| t.ends_with('!'))).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(a.text("entry00000000001").unwrap(), "Salvē!");

    // A viewer that keeps sending updates is disconnected.
    for i in 0..15 {
        v.insert_text("entry00000000001", 0, &i.to_string());
    }
    assert_eq!(v.wait_closed().await.0, 4405);
    assert_eq!(a.text("entry00000000001").unwrap(), "Salvē!");
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn membership_changes_reach_live_connections() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let (ann, bob, cy, id) = setup(&server).await;
    let bob_id = bob.get("/api/auth/me").await.1["id"].as_str().unwrap().to_owned();
    let cy_id = cy.get("/api/auth/me").await.1["id"].as_str().unwrap().to_owned();
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    let b = bob.connect(&id).await;
    b.wait_synced().await;
    let c = cy.connect(&id).await;
    c.wait_synced().await;

    // Cy becomes an editor: asked to reconnect, then can edit.
    let (status, _) = ann
        .send(Method::PUT, &format!("/api/projects/{id}/members/{cy_id}"), Some(json!({"role": "editor"})))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c.wait_closed().await.0, 4001);
    let c = cy.connect(&id).await;
    c.wait_synced().await;
    c.add_entry("entry00000000001", "Cy was here");
    eventually("Cy's edit arrives", || a.text("entry00000000001").is_some()).await;

    // Bob is removed: disconnected, and cannot reconnect.
    let (status, _) = ann.send(Method::DELETE, &format!("/api/projects/{id}/members/{bob_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (code, reason) = b.wait_closed().await;
    assert_eq!(code, 4403);
    assert_eq!(reason, "Removed from the project");
    let again = bob.connect(&id).await;
    assert_eq!(again.wait_closed().await.0, 4404);

    // Deleting the project disconnects everyone.
    assert_eq!(ann.send(Method::DELETE, &format!("/api/projects/{id}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(a.wait_closed().await.0, 4404);
    assert_eq!(c.wait_closed().await.0, 4404);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn websocket_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let (ann, _, _, id) = setup(&server).await;

    let anonymous = YClient::connect(server.addr, &id, None).await;
    assert_eq!(anonymous.wait_closed().await.0, 4401);

    let foreign = YClient::connect_with(server.addr, &id, ann.cookie(), Some("https://evil.example")).await;
    assert!(foreign.is_err(), "cross-origin upgrade is refused");
    let own = YClient::connect_with(server.addr, &id, ann.cookie(), Some(&format!("http://{}", server.addr)))
        .await
        .unwrap();
    own.wait_synced().await;

    // Signing out ends the live connection too.
    ann.post("/api/auth/logout", json!(null)).await;
    assert_eq!(own.wait_closed().await.0, 4401);

    // Garbage is a protocol error.
    ann.post("/api/auth/login", json!({"email": "ann@example.org", "password": "correct horse"})).await;
    let c = ann.connect(&id).await;
    c.wait_synced().await;
    c.send_raw(vec![0, 2, 200, 1, 2]);
    assert_eq!(c.wait_closed().await.0, 4400);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn names_sync_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let (ann, _, _, id) = setup(&server).await;
    let a = ann.connect(&id).await;
    a.wait_synced().await;

    // Renaming through the API changes the live document.
    ann.send(Method::PATCH, &format!("/api/projects/{id}"), Some(json!({"name": "Prīma"}))).await;
    eventually("rename reaches the doc", || a.meta_name() == "Prīma").await;

    // Renaming in the document updates the listing.
    a.set_meta_name("Secunda");
    for _ in 0..100 {
        if ann.get("/api/projects").await.1[0]["name"] == "Secunda" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(ann.get("/api/projects").await.1[0]["name"], "Secunda");
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn versions_restore_into_the_live_document() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let (ann, bob, cy, id) = setup(&server).await;
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    a.add_entry("entry00000000001", "Salvē, discipulī!");
    let b = bob.connect(&id).await;
    eventually("sync", || b.text("entry00000000001").is_some()).await;

    let (status, version) = bob.post(&format!("/api/projects/{id}/versions"), json!({"label": "First draft"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(version["label"], "First draft");
    assert_eq!(version["auto"], false);
    assert_eq!(version["createdBy"]["displayName"], "Bob");
    let version_id = version["id"].as_str().unwrap().to_owned();

    // Edit after the version: change the text, add another entry.
    a.insert_text("entry00000000001", 0, "Ō ");
    a.add_entry("entry00000000002", "Later");
    eventually("Bob sees the edits", || b.text("entry00000000002").is_some()).await;

    let restore = format!("/api/projects/{id}/versions/{version_id}/restore");
    assert_eq!(cy.post(&restore, json!(null)).await.0, StatusCode::FORBIDDEN);
    let (status, _) = bob.post(&restore, json!(null)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for client in [&a, &b] {
        eventually("restored", || {
            client.text("entry00000000001").as_deref() == Some("Salvē, discipulī!")
                && client.text("entry00000000002").is_none()
        })
        .await;
    }

    // The list (newest first) has the automatic "Before restore", the named
    // version, and possibly the first automatic version of the session.
    let (_, versions) = cy.get(&format!("/api/projects/{id}/versions")).await;
    let versions = versions.as_array().unwrap();
    assert_eq!(versions[0]["label"], "Before restore");
    assert_eq!(versions[0]["auto"], true);
    assert!(versions.iter().any(|v| v["label"] == "First draft" && v["auto"] == false));

    // The restore is persisted like any edit.
    server.stop().await;
    let server = TestServer::start(dir.path()).await;
    let stored = loquaris_server::storage::load(&server.state.db, &id).await.unwrap();
    let project = loquaris_server::doc::read_project(&stored.doc.transact());
    assert_eq!(project.lessons[0].entries.len(), 1);
    assert_eq!(project.lessons[0].entries[0].text, "Salvē, discipulī!");
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_versions_while_editing() {
    let dir = tempfile::tempdir().unwrap();
    let server =
        TestServer::start_with(dir.path(), |config| config.auto_version_every = Duration::from_millis(100)).await;
    let (ann, _, _, id) = setup(&server).await;
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    a.add_entry("entry00000000001", "Salvē");
    eventually("an automatic version", || {
        let versions = futures_executor(ann.get(&format!("/api/projects/{id}/versions")));
        versions.1.as_array().is_some_and(|v| v.iter().any(|v| v["auto"] == true && v["label"].is_null()))
    })
    .await;
    server.stop().await;
}

/// Runs a future to completion from synchronous polling code.
fn futures_executor<F: std::future::Future>(future: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

#[tokio::test(flavor = "multi_thread")]
async fn rooms_unload_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let (ann, _, _, id) = setup(&server).await;
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    // Send an update built outside the client's own doc (from scratch).
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        let meta = txn.get_or_insert_map("meta");
        meta.insert(&mut txn, "extra", "kept");
        let text = txn.get_or_insert_text("unrelated");
        text.insert(&mut txn, 0, "x");
    }
    let update = doc.transact().encode_state_as_update_v1(&Default::default());
    a.send_raw(Message::Sync(SyncMessage::Update(update)).encode_v1());
    drop(a);
    // After the idle delay the room is gone, and a fresh load has the change.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let b = ann.connect(&id).await;
    b.wait_synced().await;
    let txn = b.doc.transact();
    let meta = txn.get_map("meta").unwrap();
    assert!(matches!(meta.get(&txn, "extra"), Some(yrs::Out::Any(yrs::Any::String(s))) if &*s == "kept"));
    drop(txn);
    server.stop().await;
}
