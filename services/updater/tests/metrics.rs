use std::time::Duration;

use tokio::net::TcpListener;

#[tokio::test]
async fn metrics_endpoint_reports_uptime() {
    let _ = updater::init_for_tests();

    let app = updater::build_router().await.expect("build router");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let server = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve");
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = reqwest::get(format!("http://{}/metrics", addr))
        .await
        .expect("metrics response");
    assert!(resp.status().is_success());
    let body = resp.text().await.expect("body");
    assert!(body.contains("process_uptime_seconds"));

    server.abort();
}
