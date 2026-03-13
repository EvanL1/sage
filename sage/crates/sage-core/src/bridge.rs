use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use axum::{extract::{Query, State}, http::StatusCode, response::Json, routing::{get, post}, Router, middleware, response::IntoResponse};
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

use crate::store::Store;
use sage_types::{BridgeBehaviorEvent, BridgeImportRequest};

pub const DEFAULT_PORT: u16 = 18522;

/// 扩展最后活跃时间（Unix 秒），每次收到请求时更新
static LAST_SEEN: AtomicI64 = AtomicI64::new(0);

/// 获取扩展最后活跃的 Unix 时间戳（秒），0 = 从未连接
pub fn bridge_last_seen() -> i64 {
    LAST_SEEN.load(Ordering::Relaxed)
}

/// 中间件：更新 last_seen 时间戳
async fn track_last_seen(req: axum::http::Request<axum::body::Body>, next: middleware::Next) -> impl IntoResponse {
    LAST_SEEN.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
    next.run(req).await
}

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
        .route("/api/messages", get(messages_handler))
        .route("/api/chat", post(chat_handler))
        .layer(middleware::from_fn(track_last_seen))
        .layer(cors)
        .with_state(store)
}

async fn status_handler(State(store): State<Arc<Store>>) -> Json<Value> {
    let memory_count = store.count_memories().unwrap_or(0);
    let behavior_count = store.count_browser_behaviors().unwrap_or(0);
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "memory_count": memory_count,
        "behavior_count": behavior_count,
    }))
}

async fn import_memories_handler(
    State(store): State<Arc<Store>>,
    Json(req): Json<BridgeImportRequest>,
) -> Result<Json<Value>, StatusCode> {
    // 浏览器数据不直写 memories，存入 browser_behaviors 由 AI 流程提炼
    let metadata = json!({ "source": req.source, "memories": req.memories });
    match store.save_browser_behavior(
        &format!("browser:{}", req.source),
        "imported_observation",
        &metadata.to_string(),
    ) {
        Ok(_) => {
            info!("Bridge: stored {} observations from {} to behaviors", req.memories.len(), req.source);
            Ok(Json(json!({"success": true, "stored": req.memories.len()})))
        }
        Err(e) => {
            error!("Bridge: failed to save imported observations: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn behavior_handler(
    State(store): State<Arc<Store>>,
    Json(event): Json<BridgeBehaviorEvent>,
) -> Result<Json<Value>, StatusCode> {
    let metadata = serde_json::to_string(&event.metadata).unwrap_or_default();
    match store.save_browser_behavior(&event.source, &event.event_type, &metadata) {
        Ok(_) => {
            info!("Bridge: behavior from {} — {}", event.source, event.event_type);
        }
        Err(e) => {
            error!("Bridge: failed to save behavior: {e}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // message_received 事件同时写入 messages 表
    if event.event_type == "message_received" {
        let m = &event.metadata;
        let sender = m.get("sender").and_then(|v| v.as_str()).unwrap_or("unknown");
        let channel = m.get("channel").and_then(|v| v.as_str()).unwrap_or("unknown");
        let content = m.get("content").and_then(|v| v.as_str());
        let msg_type = m.get("message_type").and_then(|v| v.as_str()).unwrap_or("text");
        let ts = m.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let ts = if ts.is_empty() {
            chrono::Local::now().to_rfc3339()
        } else {
            ts.to_string()
        };

        if let Err(e) = store.save_message(sender, channel, content, &event.source, msg_type, &ts) {
            error!("Bridge: failed to save message: {e}");
        }
    }

    Ok(Json(json!({"success": true})))
}

#[derive(serde::Deserialize)]
struct MessagesQuery {
    channel: Option<String>,
    source: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize { 50 }

async fn messages_handler(
    State(store): State<Arc<Store>>,
    Query(q): Query<MessagesQuery>,
) -> Json<Value> {
    let messages = if let Some(ch) = &q.channel {
        store.get_messages_by_channel(ch, q.limit).unwrap_or_default()
    } else if let Some(src) = &q.source {
        store.get_messages_by_source(src, q.limit).unwrap_or_default()
    } else {
        store.get_messages_by_source("teams", q.limit).unwrap_or_default()
    };
    Json(json!({ "messages": messages, "count": messages.len() }))
}

/// Digital Evan 对话请求
#[derive(serde::Deserialize)]
struct ChatRequest {
    message: String,
}

/// POST /api/chat — Digital Evan 外部对话（只读 public 记忆）
async fn chat_handler(
    State(store): State<Arc<Store>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<Value>, StatusCode> {
    let discovered = crate::discovery::discover_providers(&store);
    let configs = store.load_provider_configs().unwrap_or_default();
    let (info, config) = match crate::discovery::select_best_provider(&discovered, &configs) {
        Some(pair) => pair,
        None => {
            return Ok(Json(json!({"error": "未配置 LLM provider"})));
        }
    };

    let agent_config = crate::config::AgentConfig {
        provider: "claude".into(),
        claude_binary: "claude".into(),
        codex_binary: String::new(),
        gemini_binary: String::new(),
        default_model: "claude-sonnet-4-6".into(),
        project_dir: ".".into(),
        max_budget_usd: 1.0,
        permission_mode: "default".into(),
        max_iterations: 10,
    };
    let provider = crate::provider::create_provider_from_config(&info, &config, &agent_config);
    let persona = crate::persona::Persona::new(Arc::clone(&store));

    match persona.chat(&req.message, provider.as_ref()).await {
        Ok(reply) => Ok(Json(json!({"reply": reply}))),
        Err(e) => {
            error!("Bridge chat error: {e}");
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
    async fn test_import_memories_redirects_to_behaviors() {
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
        // 不再直写 memories，而是存入 browser_behaviors
        assert_eq!(store.count_memories().unwrap(), 0);
        assert_eq!(store.count_browser_behaviors().unwrap(), 1);
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

    #[tokio::test]
    async fn test_behavior_message_received_writes_messages() {
        let (app, store) = test_app();
        let body = json!({
            "source": "teams",
            "event_type": "message_received",
            "metadata": {
                "sender": "Alice",
                "channel": "#general",
                "content": "hello world",
                "message_type": "text",
                "timestamp": "2026-03-12T10:00:00"
            }
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/behaviors")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // browser_behaviors 写入 1 条
        assert_eq!(store.count_browser_behaviors().unwrap(), 1);
        // messages 也写入 1 条
        assert_eq!(store.count_messages().unwrap(), 1);
        let msgs = store.get_messages_by_channel("#general", 10).unwrap();
        assert_eq!(msgs[0].sender, "Alice");
        assert_eq!(msgs[0].content, Some("hello world".to_string()));
    }

    #[tokio::test]
    async fn test_behavior_non_message_no_messages() {
        let (app, store) = test_app();
        let body = json!({
            "source": "browser",
            "event_type": "page_visit",
            "metadata": {"domain": "github.com", "duration_seconds": 120}
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/behaviors")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 非 message_received 事件不写 messages 表
        assert_eq!(store.count_browser_behaviors().unwrap(), 1);
        assert_eq!(store.count_messages().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_messages_endpoint() {
        let (app, store) = test_app();
        // 先写入两条消息
        store.save_message("Alice", "#general", Some("hi"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.save_message("Bob", "#dev", Some("PR ready"), "teams", "text", "2026-03-12T10:01:00").unwrap();

        let req = Request::builder()
            .uri("/api/messages?source=teams&limit=10")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["count"], 2);
    }

    #[tokio::test]
    async fn test_messages_endpoint_by_channel() {
        let (app, store) = test_app();
        store.save_message("Alice", "#general", Some("hi"), "teams", "text", "2026-03-12T10:00:00").unwrap();
        store.save_message("Bob", "#dev", Some("yo"), "teams", "text", "2026-03-12T10:01:00").unwrap();

        let req = Request::builder()
            .uri("/api/messages?channel=%23general&limit=10")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["count"], 1);
    }
}
