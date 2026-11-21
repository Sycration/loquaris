//! Accounts, sessions, CSRF, registration modes and admin.
mod common;

use common::*;
use loquaris_server::config::Registration;
use reqwest::{Method, StatusCode};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn register_login_logout() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let ann = server.client();

    let (status, config) = ann.get("/api/config").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["registration"], "open");
    assert!(config["version"].is_string());

    assert_eq!(ann.get("/api/auth/me").await.0, StatusCode::UNAUTHORIZED);
    let user = ann.register("  Ann@Example.org ", "Ann").await;
    assert_eq!(user["email"], "ann@example.org");
    assert_eq!(user["isAdmin"], true, "the first account is an admin");
    assert_eq!(user["settings"], json!({}));
    let cookie = ann.cookie().unwrap();
    assert!(cookie.starts_with("loq_session="));

    let (status, me) = ann.get("/api/auth/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["displayName"], "Ann");

    // A second account is not an admin; emails are unique.
    let bob = server.client();
    assert_eq!(bob.register("bob@example.org", "Bob").await["isAdmin"], false);
    let (status, body) = server
        .client()
        .post(
            "/api/auth/register",
            json!({"email": "BOB@example.org", "displayName": "B", "password": "long enough"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].is_string());
    let (status, _) = server
        .client()
        .post("/api/auth/register", json!({"email": "c@d.e", "displayName": "C", "password": "short"}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Logout ends the session.
    let (status, _) = ann.post("/api/auth/logout", json!(null)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(ann.get("/api/auth/me").await.0, StatusCode::UNAUTHORIZED);

    // Wrong password, then the right one.
    let (status, body) = ann
        .post("/api/auth/login", json!({"email": "ann@example.org", "password": "wrong password"}))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "Incorrect email or password");
    let (status, _) = ann
        .post("/api/auth/login", json!({"email": "ANN@example.org", "password": "correct horse"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ann.get("/api/auth/me").await.0, StatusCode::OK);

    // Settings and display name.
    let (status, me) = ann
        .send(
            Method::PATCH,
            "/api/auth/me",
            Some(json!({"displayName": "Anna", "settings": {"keybindings": {"undo": "Mod+Z"}}})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["displayName"], "Anna");
    assert_eq!(me["settings"]["keybindings"]["undo"], "Mod+Z");
    assert_eq!(ann.get("/api/auth/me").await.1["settings"]["keybindings"]["undo"], "Mod+Z");

    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn csrf_header_is_required() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let client = server.client();
    let response = client
        .http
        .post(client.url("/api/auth/register"))
        .json(&json!({"email": "a@b.c", "displayName": "A", "password": "long enough"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("X-Loquaris"));
    // GET needs no header; unknown API paths are JSON 404s.
    let response = client.http.get(client.url("/api/nope")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_and_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let laptop = server.client();
    laptop.register("ann@example.org", "Ann").await;
    let phone = server.client();
    let (status, _) = phone
        .post("/api/auth/login", json!({"email": "ann@example.org", "password": "correct horse"}))
        .await;
    assert_eq!(status, StatusCode::OK);

    let (_, sessions) = laptop.get("/api/auth/sessions").await;
    let sessions = sessions.as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions.iter().filter(|s| s["current"] == true).count(), 1);

    // Sign out everywhere else.
    assert_eq!(laptop.send(Method::DELETE, "/api/auth/sessions", None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(phone.get("/api/auth/me").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(laptop.get("/api/auth/sessions").await.1.as_array().unwrap().len(), 1);

    // Change the password: the old one stops working.
    let (status, _) = laptop
        .post("/api/auth/password", json!({"current": "wrong one!", "new": "new password"}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = laptop
        .post("/api/auth/password", json!({"current": "correct horse", "new": "new password"}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(laptop.get("/api/auth/me").await.0, StatusCode::OK);
    let (status, _) = phone
        .post("/api/auth/login", json!({"email": "ann@example.org", "password": "correct horse"}))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = phone
        .post("/api/auth/login", json!({"email": "ann@example.org", "password": "new password"}))
        .await;
    assert_eq!(status, StatusCode::OK);

    // Revoke one session by ID.
    let (_, sessions) = laptop.get("/api/auth/sessions").await;
    let other = sessions.as_array().unwrap().iter().find(|s| s["current"] == false).unwrap();
    let path = format!("/api/auth/sessions/{}", other["id"].as_str().unwrap());
    assert_eq!(laptop.send(Method::DELETE, &path, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(phone.get("/api/auth/me").await.0, StatusCode::UNAUTHORIZED);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn registration_modes() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start_with(dir.path(), |config| config.registration = Registration::Invite).await;
    let owner = server.client();
    // With no invite, even the first account cannot register in invite mode.
    let (status, _) = owner
        .post(
            "/api/auth/register",
            json!({"email": "ann@example.org", "displayName": "Ann", "password": "long enough"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    server.stop().await;

    // Make an account and project while open, then switch to invite mode.
    let server = TestServer::start(dir.path()).await;
    let owner = server.client();
    owner.register("ann@example.org", "Ann").await;
    let project = owner.create_project("Latin I").await;
    let (_, invite) = owner
        .post(&format!("/api/projects/{project}/invites"), json!({"role": "viewer", "maxUses": 1}))
        .await;
    let token = invite["url"].as_str().unwrap().rsplit('/').next().unwrap().to_owned();
    server.stop().await;

    let server = TestServer::start_with(dir.path(), |config| config.registration = Registration::Invite).await;
    assert_eq!(server.client().get("/api/config").await.1["registration"], "invite");
    let bob = server.client();
    let (status, user) = bob
        .post(
            "/api/auth/register",
            json!({"email": "bob@example.org", "displayName": "Bob", "password": "long enough", "invite": token}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    let (_, projects) = bob.get("/api/projects").await;
    assert_eq!(projects[0]["id"], project.as_str());
    assert_eq!(projects[0]["role"], "viewer");
    // The invite is used up.
    let (status, _) = server
        .client()
        .post(
            "/api/auth/register",
            json!({"email": "cy@example.org", "displayName": "Cy", "password": "long enough", "invite": token}),
        )
        .await;
    assert_eq!(status, StatusCode::GONE);
    server.stop().await;

    let server = TestServer::start_with(dir.path(), |config| config.registration = Registration::Closed).await;
    let (status, _) = server
        .client()
        .post(
            "/api/auth/register",
            json!({"email": "dee@example.org", "displayName": "Dee", "password": "long enough"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limits_sign_in() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let client = server.client();
    client.register("ann@example.org", "Ann").await;
    let mut statuses = Vec::new();
    for _ in 0..12 {
        let (status, _) = client
            .post("/api/auth/login", json!({"email": "ann@example.org", "password": "wrong password"}))
            .await;
        statuses.push(status);
    }
    assert_eq!(statuses.last(), Some(&StatusCode::TOO_MANY_REQUESTS));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_manages_users() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let admin = server.client();
    admin.register("root@example.org", "Root").await;
    let bob = server.client();
    bob.register("bob@example.org", "Bob").await;
    assert_eq!(bob.get("/api/admin/users").await.0, StatusCode::FORBIDDEN);

    let (status, users) = admin.get("/api/admin/users").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(users.as_array().unwrap().len(), 2);
    assert_eq!(users[0]["projectCount"], 0);

    // A new account gets a set-password link.
    let (status, link) = admin
        .post("/api/admin/users", json!({"email": "cy@example.org", "displayName": "Cy"}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let url = link["url"].as_str().unwrap();
    assert!(url.starts_with(&format!("http://{}/reset/", server.addr)));
    assert!(link["expiresAt"].is_string());
    let token = url.rsplit('/').next().unwrap();
    let cy = server.client();
    let (status, user) = cy.post("/api/auth/reset", json!({"token": token, "password": "cy's password"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["email"], "cy@example.org");
    assert_eq!(cy.get("/api/auth/me").await.0, StatusCode::OK);
    // Links are one-time.
    let (status, _) = cy.post("/api/auth/reset", json!({"token": token, "password": "another one"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Disable Bob: his session ends and he cannot sign in.
    let users = admin.get("/api/admin/users").await.1;
    let bob_id = users.as_array().unwrap().iter().find(|u| u["email"] == "bob@example.org").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, updated) = admin
        .send(Method::PATCH, &format!("/api/admin/users/{bob_id}"), Some(json!({"disabled": true})))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["disabled"], true);
    assert_eq!(bob.get("/api/auth/me").await.0, StatusCode::UNAUTHORIZED);
    let (status, _) = bob
        .post("/api/auth/login", json!({"email": "bob@example.org", "password": "correct horse"}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The last admin stays an admin.
    let root_id = admin.get("/api/auth/me").await.1["id"].as_str().unwrap().to_owned();
    let (status, _) = admin
        .send(Method::PATCH, &format!("/api/admin/users/{root_id}"), Some(json!({"isAdmin": false})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Reset link for an existing user.
    let (status, link) = admin.post(&format!("/api/admin/users/{bob_id}/reset-link"), json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(link["url"].as_str().unwrap().contains("/reset/"));
    server.stop().await;
}
