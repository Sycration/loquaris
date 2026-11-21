//! Projects, members, roles, invites.
mod common;

use common::*;
use reqwest::{Method, StatusCode};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn project_lifecycle_and_roles() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let ann = server.client();
    ann.register("ann@example.org", "Ann").await;
    let bob = server.client();
    let bob_user = bob.register("bob@example.org", "Bob").await;
    let cy = server.client();
    let cy_user = cy.register("cy@example.org", "Cy").await;
    let eve = server.client();
    eve.register("eve@example.org", "Eve").await;

    let (status, created) = ann.post("/api/projects", json!({"name": " Latin I ", "l1Name": "English"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["name"], "Latin I");
    assert_eq!(created["role"], "owner");
    assert_eq!(created["memberCount"], 1);
    assert_eq!(created["owner"]["displayName"], "Ann");
    let id = created["id"].as_str().unwrap().to_owned();

    ann.add_member(&id, "BOB@example.org", "editor").await;
    ann.add_member(&id, "cy@example.org", "viewer").await;
    let (status, _) = ann
        .post(&format!("/api/projects/{id}/members"), json!({"email": "bob@example.org", "role": "viewer"}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = ann
        .post(&format!("/api/projects/{id}/members"), json!({"email": "nobody@example.org", "role": "viewer"}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Listings and details.
    let (_, list) = bob.get("/api/projects").await;
    assert_eq!(list[0]["role"], "editor");
    assert_eq!(list[0]["memberCount"], 3);
    let (status, detail) = ann.get(&format!("/api/projects/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["members"].as_array().unwrap().len(), 3);
    assert_eq!(detail["members"][0]["role"], "owner");
    assert!(detail["invites"].is_array(), "owners see invites");
    let (_, detail) = bob.get(&format!("/api/projects/{id}")).await;
    assert!(detail.get("invites").is_none(), "others do not");

    // Outsiders see nothing; viewers and editors cannot do owner things.
    assert_eq!(eve.get(&format!("/api/projects/{id}")).await.0, StatusCode::NOT_FOUND);
    let rename = Some(json!({"name": "Renamed"}));
    assert_eq!(cy.send(Method::PATCH, &format!("/api/projects/{id}"), rename.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(bob.send(Method::PATCH, &format!("/api/projects/{id}"), rename.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(cy.post(&format!("/api/projects/{id}/versions"), json!({"label": "x"})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(bob.post(&format!("/api/projects/{id}/versions"), json!({"label": "x"})).await.0, StatusCode::CREATED);
    assert_eq!(bob.send(Method::DELETE, &format!("/api/projects/{id}"), None).await.0, StatusCode::FORBIDDEN);

    // Rename (owner).
    let (status, detail) = ann.send(Method::PATCH, &format!("/api/projects/{id}"), rename).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["name"], "Renamed");

    // Change roles; the owner cannot demote themselves or leave.
    let bob_id = bob_user["id"].as_str().unwrap();
    let cy_id = cy_user["id"].as_str().unwrap();
    let ann_id = ann.get("/api/auth/me").await.1["id"].as_str().unwrap().to_owned();
    let path = |user: &str| format!("/api/projects/{id}/members/{user}");
    let (status, member) = ann.send(Method::PUT, &path(cy_id), Some(json!({"role": "editor"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member["role"], "editor");
    assert_eq!(ann.send(Method::PUT, &path(&ann_id), Some(json!({"role": "editor"}))).await.0, StatusCode::CONFLICT);
    assert_eq!(ann.send(Method::DELETE, &path(&ann_id), None).await.0, StatusCode::CONFLICT);

    // Transfer ownership to Bob: Ann becomes an editor.
    let (status, member) = ann.send(Method::PUT, &path(bob_id), Some(json!({"role": "owner"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member["role"], "owner");
    let (_, detail) = ann.get(&format!("/api/projects/{id}")).await;
    assert_eq!(detail["role"], "editor");
    assert_eq!(detail["owner"]["id"], bob_id);

    // Cy leaves; Bob removes Ann.
    assert_eq!(cy.send(Method::DELETE, &path(cy_id), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(cy.get(&format!("/api/projects/{id}")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(bob.send(Method::DELETE, &path(&ann_id), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(ann.get("/api/projects").await.1.as_array().unwrap().len(), 0);

    // Duplicate (any member) and delete (owner).
    let (status, copy) = bob.post(&format!("/api/projects/{id}/duplicate"), json!(null)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(copy["name"], "Renamed (copy)");
    assert_eq!(copy["role"], "owner");
    assert_eq!(bob.send(Method::DELETE, &format!("/api/projects/{id}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(bob.get(&format!("/api/projects/{id}")).await.0, StatusCode::NOT_FOUND);
    let (_, list) = bob.get("/api/projects").await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn invites() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let ann = server.client();
    ann.register("ann@example.org", "Ann").await;
    let id = ann.create_project("Latin I").await;
    let bob = server.client();
    bob.register("bob@example.org", "Bob").await;
    let cy = server.client();
    cy.register("cy@example.org", "Cy").await;

    let (status, created) = ann
        .post(&format!("/api/projects/{id}/invites"), json!({"role": "editor", "expiresInDays": 7, "maxUses": 1}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let url = created["url"].as_str().unwrap();
    assert!(url.starts_with(&format!("http://{}/invite/", server.addr)));
    assert_eq!(created["invite"]["role"], "editor");
    assert_eq!(created["invite"]["maxUses"], 1);
    assert_eq!(created["invite"]["createdBy"]["displayName"], "Ann");
    let token = url.rsplit('/').next().unwrap();

    // Public preview.
    let (status, preview) = server.client().get(&format!("/api/invites/{token}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["projectName"], "Latin I");
    assert_eq!(preview["valid"], true);
    assert_eq!(server.client().get("/api/invites/nonsense").await.0, StatusCode::NOT_FOUND);

    // Accepting needs an account.
    let (status, _) = server.client().post(&format!("/api/invites/{token}/accept"), json!(null)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, accepted) = bob.post(&format!("/api/invites/{token}/accept"), json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accepted["projectId"], id.as_str());
    assert_eq!(bob.get("/api/projects").await.1[0]["role"], "editor");
    // Accepting again as a member changes nothing; others find it used up.
    assert_eq!(bob.post(&format!("/api/invites/{token}/accept"), json!(null)).await.0, StatusCode::OK);
    assert_eq!(cy.post(&format!("/api/invites/{token}/accept"), json!(null)).await.0, StatusCode::GONE);
    assert_eq!(server.client().get(&format!("/api/invites/{token}")).await.1["valid"], false);

    // Revoking.
    let (_, created) = ann.post(&format!("/api/projects/{id}/invites"), json!({"role": "viewer"})).await;
    let token = created["url"].as_str().unwrap().rsplit('/').next().unwrap().to_owned();
    let invite_id = created["invite"]["id"].as_str().unwrap();
    let (_, detail) = ann.get(&format!("/api/projects/{id}")).await;
    assert_eq!(detail["invites"].as_array().unwrap().len(), 2);
    let path = format!("/api/projects/{id}/invites/{invite_id}");
    assert_eq!(bob.send(Method::DELETE, &path, None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(ann.send(Method::DELETE, &path, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(cy.post(&format!("/api/invites/{token}/accept"), json!(null)).await.0, StatusCode::GONE);
    let (_, detail) = ann.get(&format!("/api/projects/{id}")).await;
    assert_eq!(detail["invites"].as_array().unwrap().len(), 1);
    server.stop().await;
}
