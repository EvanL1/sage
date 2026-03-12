use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Json, routing::{get, post}, Router};
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

use crate::store::Store;
use sage_types::{BridgeBehaviorEvent, BridgeImportRequest};

pub const DEFAULT_PORT: u16 = 18522;

/// 构建 Bridge HTTP Router（可独立测试）
pub fn build_router(store: Arc<Store>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/status", get(status_handler))
        .route("/api/memories", post(import_memories_handler))
        .route("/api/behaviors", post(behavior_handler))
        .layer(cors)
        .with_state(store)
}

async fn status_handler(State(store): State<Arc<Store>>) -> Json<Value> {
    let memory_count = store.count_memories().unwrap_or(0);
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "memory_count": memory_count,
    }))
}

async fn import_memories_handler(
    State(store): State<Arc<Store>>,
    Json(req): Json<BridgeImportRequest>,
) -> Result<Json<Value>, StatusCode> {
    let source = format!("browser:{}", req.source);
    let mut imported = 0;
    for entry in &req.memories {
        match store.save_memory(&entry.category, &entry.content, &source, entry.confidence) {
            Ok(_) => imported += 1,
            Err(e) => error!("Bridge: failed to save memory: {e}"),
        }
    }
    info!("Bridge: imported {imported} memories from {}", req.source);
    Ok(Json(json!({"success": true, "imported": imported})))
}

async fn behavior_handler(
    State(store): State<Arc<Store>>,
    Json(event): Json<BridgeBehaviorEvent>,
) -> Result<Json<Value>, StatusCode> {
    let metadata = serde_json::to_string(&event.metadata).unwrap_or_default();
    match store.save_browser_behavior(&event.source, &event.event_type, &metadata) {
        Ok(_) => {
            info!("Bridge: behavior from {} — {}", event.source, event.event_type);
            Ok(Json(json!({"success": true})))
        }
        Err(e) => {
            error!("Bridge: failed to save behavior: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// 启动 Bridge HTTP 服务器（阻塞，应在 tokio::spawn 中调用）
pub async fn start_bridge_server(store: Arc<Store>, port: u16) -> anyhow::Result<()> {
    let app = build_router(store);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Bridge server listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app() -> (Router, Arc<Store>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = build_router(store.clone());
        (app, store)
    }

    #[tokio::test]
    async fn test_status_endpoint() {
        let (app, _) = test_app();
        let req = Request::builder()
            .uri("/api/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_import_memories() {
        let (app, store) = test_app();
        let body = json!({
            "source": "chatgpt",
            "memories": [
                {"category": "behavior", "content": "prefers concise answers", "confidence": 0.8},
                {"category": "identity", "content": "software engineer", "confidence": 0.9}
            ]
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/memories")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let count = store.count_memories().unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_import_empty_memories() {
        let (app, _) = test_app();
        let body = json!({ "source": "claude", "memories": [] });
        let req = Request::builder()
            .method("POST")
            .uri("/api/memories")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_behavior_event() {
        let (app, store) = test_app();
        let body = json!({
            "source": "gemini",
            "event_type": "conversation_start",
            "metadata": {"topic": "rust async"}
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/behaviors")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let behaviors = store.get_browser_behaviors(10).unwrap();
        assert_eq!(behaviors.len(), 1);
        assert_eq!(behaviors[0].source, "gemini");
    }

    #[tokio::test]
    async fn test_invalid_json_rejected() {
        let (app, _) = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/memories")
            .header("content-type", "application/json")
            .body(Body::from("not json"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.status().is_client_error());
    }

    #[tokio::test]
    async fn test_cors_preflight() {
        let (app, _) = test_app();
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/api/status")
            .header("origin", "chrome-extension://abc123")
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().contains_key("access-control-allow-origin"));
    }
}
