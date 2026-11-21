//! Audio upload and download, exports and `.loqr` import.
mod common;

use common::*;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use serde_json::json;
use yrs::{Map, ReadTxn, Transact};

#[tokio::test(flavor = "multi_thread")]
async fn audio_and_exports() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path()).await;
    let ann = server.client();
    ann.register("ann@example.org", "Ann").await;
    let id = ann.create_project("Latīna Prīma").await;
    let cy = server.client();
    cy.register("cy@example.org", "Cy").await;
    ann.add_member(&id, "cy@example.org", "viewer").await;
    let eve = server.client();
    eve.register("eve@example.org", "Eve").await;

    // Upload: editors only, valid WAV only.
    let bytes = wav(48_000, 1, 4_800);
    let upload = |client: &Client, body: Vec<u8>| {
        client
            .request(reqwest::Method::POST, &format!("/api/projects/{id}/audio"))
            .header(CONTENT_TYPE, "audio/wav")
            .body(body)
            .send()
    };
    assert_eq!(upload(&cy, bytes.clone()).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(upload(&ann, b"not a wav".to_vec()).await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = upload(&ann, bytes.clone()).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let audio: serde_json::Value = response.json().await.unwrap();
    assert_eq!(audio["sampleRate"], 48_000);
    assert_eq!(audio["frames"], 4_800);
    assert_eq!(audio["channels"], 1);
    let blob = audio["blob"].as_str().unwrap().to_owned();
    assert_eq!(blob, loquaris_server::util::sha256_hex(&bytes));

    // Download: members of a project holding the blob; ranges work.
    let get = |client: &Client| client.http.get(client.url(&format!("/api/audio/{blob}")));
    let response = get(&cy).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()["cache-control"].to_str().unwrap().contains("immutable"));
    assert_eq!(response.bytes().await.unwrap().as_ref(), bytes.as_slice());
    let response = get(&cy).header(RANGE, "bytes=0-3").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[CONTENT_RANGE], format!("bytes 0-3/{}", bytes.len()));
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"RIFF");
    assert_eq!(get(&eve).send().await.unwrap().status(), StatusCode::NOT_FOUND);

    // Put the clip in the document, then export.
    let a = ann.connect(&id).await;
    a.wait_synced().await;
    a.add_entry("entry00000000001", "Salvē, discipulī!");
    {
        let entries = a.doc.transact().get_map("entries").unwrap();
        let txn = a.doc.transact();
        let Some(yrs::Out::YMap(entry)) = entries.get(&txn, "entry00000000001") else { panic!() };
        drop(txn);
        let value: yrs::Any = serde_json::from_value(audio.clone()).unwrap();
        entry.insert(&mut a.doc.transact_mut(), "audio", value);
    }
    let probe = ann.connect(&id).await;
    eventually("server has the entry", || probe.text("entry00000000001").is_some()).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let response = cy.http.get(cy.url(&format!("/api/projects/{id}/export/pdf"))).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/pdf");
    assert!(response.headers()[CONTENT_DISPOSITION].to_str().unwrap().contains("filename*=UTF-8''Lat%C4%ABna"));
    assert!(response.bytes().await.unwrap().starts_with(b"%PDF"));

    let response = cy
        .http
        .get(cy.url(&format!("/api/projects/{id}/export/wav?teacher=true&student=true")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{:?}", response.text().await);
    let response = cy.http.get(cy.url(&format!("/api/projects/{id}/export/wav"))).send().await.unwrap();
    assert_eq!(response.headers()[CONTENT_TYPE], "audio/wav");
    assert!(response.bytes().await.unwrap().starts_with(b"RIFF"));
    let response = cy.http.get(cy.url(&format!("/api/projects/{id}/export/pdf?lessons=nope"))).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // .loqr round trip: export, then import as a new project.
    let response = cy.http.get(cy.url(&format!("/api/projects/{id}/export/loqr"))).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let archive = response.bytes().await.unwrap().to_vec();
    let response = eve
        .request(reqwest::Method::POST, "/api/projects/import")
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(archive)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let imported: serde_json::Value = response.json().await.unwrap();
    assert_eq!(imported["name"], "Latīna Prīma");
    assert_eq!(imported["role"], "owner");
    // The importer can now read the clip.
    assert_eq!(get(&eve).send().await.unwrap().status(), StatusCode::OK);
    let e = eve.connect(imported["id"].as_str().unwrap()).await;
    e.wait_synced().await;
    assert_eq!(e.entry_ids().len(), 1);
    let _ = json!(null);
    server.stop().await;
}
