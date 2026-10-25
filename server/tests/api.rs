use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app(dir: &tempfile::TempDir) -> Router {
    spentday_server::router(
        spentday_server::open_db(&dir.path().join("s.db").to_string_lossy()).unwrap(),
    )
}

const NONCE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn push_body(device: &str, ciphertext: &str) -> String {
    format!(r#"{{"device_id":"{device}","nonce":"{NONCE}","ciphertext":"{ciphertext}"}}"#)
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: String,
) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn first_writer_claims_the_space() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(&dir);
    assert_eq!(
        call(&app, "GET", "/health", None, String::new()).await,
        (StatusCode::OK, "ok".into())
    );

    let url = "/v1/spaces/abc123/batches";
    assert_eq!(
        call(&app, "GET", url, Some("t1"), String::new()).await,
        (StatusCode::OK, r#"{"batches":[],"more":false,"head":0}"#.into())
    );
    assert_eq!(
        call(&app, "POST", url, Some("t1"), push_body("d1", "aGVsbG8=")).await,
        (StatusCode::OK, r#"{"cursor":1}"#.into())
    );
    assert_eq!(
        call(&app, "POST", url, Some("t1"), push_body("d2", "d29ybGQ=")).await,
        (StatusCode::OK, r#"{"cursor":2}"#.into())
    );
    assert_eq!(
        call(&app, "POST", url, Some("t2"), push_body("d3", "eA==")).await,
        (
            StatusCode::UNAUTHORIZED,
            "wrong token for this space".into()
        )
    );
    assert_eq!(
        call(&app, "GET", url, Some("t2"), String::new()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "POST", url, None, push_body("d3", "eA=="))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    // A second space is independent of the first.
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/spaces/other/batches",
            Some("t2"),
            push_body("d9", "eA==")
        )
        .await
        .0,
        StatusCode::OK
    );

    let (status, body) = call(
        &app,
        "GET",
        &format!("{url}?after=0&limit=1"),
        Some("t1"),
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        format!(
            r#"{{"batches":[{{"cursor":1,"device_id":"d1","nonce":"{NONCE}","ciphertext":"aGVsbG8="}}],"more":true,"head":2}}"#
        )
    );
    let (_, body) = call(
        &app,
        "GET",
        &format!("{url}?after=1"),
        Some("t1"),
        String::new(),
    )
    .await;
    assert_eq!(
        body,
        format!(
            r#"{{"batches":[{{"cursor":2,"device_id":"d2","nonce":"{NONCE}","ciphertext":"d29ybGQ="}}],"more":false,"head":2}}"#
        )
    );
}

#[tokio::test]
async fn rejects_malformed_and_oversized_batches() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(&dir);
    let url = "/v1/spaces/s/batches";
    assert_eq!(
        call(&app, "POST", url, Some("t"), "{}".into()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, "POST", url, Some("t"), push_body("d1", "not base64!"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/spaces/bad%20id/batches",
            Some("t"),
            push_body("d1", "eA==")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let under = "A".repeat(8 * 1024 * 1024 - 200);
    assert_eq!(
        call(&app, "POST", url, Some("t"), push_body("d1", &under))
            .await
            .0,
        StatusCode::OK
    );
    let over = "A".repeat(8 * 1024 * 1024);
    assert_eq!(
        call(&app, "POST", url, Some("t"), push_body("d1", &over))
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

async fn page_cursors(app: &Router, after: i64) -> (Vec<i64>, bool) {
    let (status, body) = call(
        app,
        "GET",
        &format!("/v1/spaces/s/batches?after={after}&limit=1000"),
        Some("t"),
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let page: serde_json::Value = serde_json::from_str(&body).unwrap();
    let cursors = page["batches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["cursor"].as_i64().unwrap())
        .collect();
    (cursors, page["more"].as_bool().unwrap())
}

#[tokio::test]
async fn pages_stop_near_4_mib_but_always_hold_one_batch() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(&dir);
    let url = "/v1/spaces/s/batches";
    for size in [3 << 20, 3 << 20, 256 << 10, 256 << 10, 7 << 20, 12] {
        let body = push_body("d1", &"A".repeat(size));
        assert_eq!(
            call(&app, "POST", url, Some("t"), body).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(page_cursors(&app, 0).await, (vec![1], true));
    assert_eq!(page_cursors(&app, 1).await, (vec![2, 3, 4], true));
    assert_eq!(page_cursors(&app, 4).await, (vec![5], true));
    assert_eq!(page_cursors(&app, 5).await, (vec![6], false));
}
