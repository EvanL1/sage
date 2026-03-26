/// Bridge REST API — 只读 GET 端点（给外部开发者用）
///
/// 端点列表：
///   GET /api/memories           — 查询记忆（支持 category/depth/limit/since）
///   GET /api/memories/:id       — 单条记忆详情（含 tags）
///   GET /api/tasks              — 查询任务（支持 status/limit）
///   GET /api/observations       — 最近观察（支持 limit/since）
///   GET /api/reports/latest     — 最新报告（支持 type=morning/evening/weekly）
///   GET /api/graph/connected/:id — 记忆图谱邻居（支持 depth/min_weight）
///   POST /api/pipeline/trigger  — 触发认知管线（写操作）
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::store::Store;

// ─── 通用响应工具 ────────────────────────────────────────────

fn ok(data: Value) -> Json<Value> {
    Json(json!({ "success": true, "data": data }))
}

fn err_resp(msg: &str) -> Json<Value> {
    Json(json!({ "success": false, "error": msg }))
}

// ─── 路由注册 ─────────────────────────────────────────────

/// 注册只读 REST API 路由到现有 Router
pub fn mount(router: Router<Arc<Store>>) -> Router<Arc<Store>> {
    router
        .route("/api/memories", get(memories_handler))
        .route("/api/memories/:id", get(memory_by_id_handler))
        .route("/api/tasks", get(tasks_handler))
        .route("/api/observations", get(observations_handler))
        .route("/api/reports/latest", get(reports_latest_handler))
        .route("/api/graph/connected/:id", get(graph_connected_handler))
        .route("/api/pipeline/trigger", post(pipeline_trigger_handler))
}

// ─── GET /api/memories ──────────────────────────────────────

#[derive(Deserialize)]
pub struct MemoriesQuery {
    category: Option<String>,
    depth: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    since: Option<String>,
}

fn default_limit() -> usize {
    50
}

async fn memories_handler(
    State(store): State<Arc<Store>>,
    Query(q): Query<MemoriesQuery>,
) -> Json<Value> {
    let all = match store.load_memories() {
        Ok(v) => v,
        Err(e) => {
            error!("memories_handler error: {e}");
            return err_resp("读取记忆失败");
        }
    };
    let limit = q.limit.min(200);
    let data: Vec<_> = all
        .into_iter()
        .filter(|m| q.category.as_deref().map_or(true, |c| m.category == c))
        .filter(|m| q.depth.as_deref().map_or(true, |d| m.depth == d))
        .filter(|m| {
            q.since
                .as_deref()
                .map_or(true, |s| m.created_at.as_str() >= s)
        })
        .take(limit)
        .map(|m| {
            json!({
                "id": m.id, "category": m.category, "content": m.content,
                "depth": m.depth, "confidence": m.confidence,
                "created_at": m.created_at, "about_person": m.about_person,
            })
        })
        .collect();
    ok(json!(data))
}

// ─── GET /api/memories/:id ───────────────────────────────────

async fn memory_by_id_handler(
    State(store): State<Arc<Store>>,
    Path(id): Path<i64>,
) -> (StatusCode, Json<Value>) {
    // load_memories 然后按 id 过滤（无专用 get_by_id，但记忆数可控）
    let mem = match store.load_memories() {
        Ok(v) => v.into_iter().find(|m| m.id == id),
        Err(e) => {
            error!("memory_by_id_handler error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                err_resp("读取记忆失败"),
            );
        }
    };
    match mem {
        None => (StatusCode::NOT_FOUND, err_resp("记忆不存在")),
        Some(m) => {
            let tags = store.get_tags(m.id).unwrap_or_default();
            let data = json!({
                "id": m.id, "category": m.category, "content": m.content,
                "depth": m.depth, "confidence": m.confidence,
                "created_at": m.created_at, "about_person": m.about_person,
                "tags": tags,
            });
            (StatusCode::OK, ok(data))
        }
    }
}

// ─── GET /api/tasks ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct TasksQuery {
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

async fn tasks_handler(
    State(store): State<Arc<Store>>,
    Query(q): Query<TasksQuery>,
) -> Json<Value> {
    let limit = q.limit.min(200);
    let rows = match store.list_tasks(q.status.as_deref(), limit) {
        Ok(v) => v,
        Err(e) => {
            error!("tasks_handler error: {e}");
            return err_resp("读取任务失败");
        }
    };
    let data: Vec<_> = rows
        .into_iter()
        .map(
            |(id, content, status, priority, due_date, source, created_at, ..)| {
                json!({
                    "id": id, "content": content, "status": status,
                    "priority": priority, "due_date": due_date,
                    "source": source, "created_at": created_at,
                })
            },
        )
        .collect();
    ok(json!(data))
}

// ─── GET /api/observations ───────────────────────────────────

#[derive(Deserialize)]
pub struct ObservationsQuery {
    #[serde(default = "default_obs_limit")]
    limit: usize,
    since: Option<String>,
}

fn default_obs_limit() -> usize {
    30
}

async fn observations_handler(
    State(store): State<Arc<Store>>,
    Query(q): Query<ObservationsQuery>,
) -> Json<Value> {
    let limit = q.limit.min(200);
    // load_recent_observations 返回 (category, observation)
    let rows = match store.load_recent_observations(limit * 2) {
        Ok(v) => v,
        Err(e) => {
            error!("observations_handler error: {e}");
            return err_resp("读取观察失败");
        }
    };
    // load_recent_observations 没有带 created_at，使用 ObservationRow 版本
    let feed_rows = store.load_unprocessed_observations(limit * 4).unwrap_or_default();
    // 合并：优先用 unprocessed（有 created_at），fallback 用 recent
    let data: Vec<_> = if feed_rows.is_empty() {
        rows.into_iter()
            .filter(|(_cat, _obs)| true) // load_recent_observations 无 created_at，无法按 since 过滤
            .take(limit)
            .map(|(category, observation)| {
                json!({ "category": category, "content": observation, "created_at": null })
            })
            .collect()
    } else {
        feed_rows
            .into_iter()
            .filter(|r| {
                q.since
                    .as_deref()
                    .map_or(true, |s| r.created_at.as_str() >= s)
            })
            .take(limit)
            .map(|r| {
                json!({
                    "category": r.category,
                    "content": r.observation,
                    "created_at": r.created_at,
                })
            })
            .collect()
    };
    ok(json!(data))
}

// ─── GET /api/reports/latest ─────────────────────────────────

#[derive(Deserialize)]
pub struct ReportsQuery {
    #[serde(rename = "type", default = "default_report_type")]
    report_type: String,
}

fn default_report_type() -> String {
    "morning".to_string()
}

async fn reports_latest_handler(
    State(store): State<Arc<Store>>,
    Query(q): Query<ReportsQuery>,
) -> (StatusCode, Json<Value>) {
    match store.get_latest_report(&q.report_type) {
        Err(e) => {
            error!("reports_latest_handler error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, err_resp("读取报告失败"))
        }
        Ok(None) => (StatusCode::NOT_FOUND, err_resp("没有该类型的报告")),
        Ok(Some(r)) => {
            let data = json!({
                "type": r.report_type,
                "content": r.content,
                "created_at": r.created_at,
            });
            (StatusCode::OK, ok(data))
        }
    }
}

// ─── GET /api/graph/connected/:id ───────────────────────────

#[derive(Deserialize)]
pub struct GraphQuery {
    #[serde(default = "default_graph_depth")]
    depth: usize,
    #[serde(default)]
    min_weight: f64,
}

fn default_graph_depth() -> usize {
    2
}

async fn graph_connected_handler(
    State(store): State<Arc<Store>>,
    Path(id): Path<i64>,
    Query(q): Query<GraphQuery>,
) -> (StatusCode, Json<Value>) {
    let depth = q.depth.min(4);
    let connected = match store.get_connected_memories(id, depth) {
        Ok(v) => v,
        Err(e) => {
            error!("graph_connected_handler error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, err_resp("图谱查询失败"));
        }
    };
    let edges = match store.get_memory_edges(id) {
        Ok(v) => v,
        Err(e) => {
            error!("get_memory_edges error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, err_resp("图谱边查询失败"));
        }
    };

    let nodes: Vec<_> = connected
        .into_iter()
        .filter(|(_, w)| *w >= q.min_weight)
        .map(|(m, activation)| {
            json!({
                "id": m.id, "category": m.category, "content": m.content,
                "depth": m.depth, "confidence": m.confidence, "activation": activation,
            })
        })
        .collect();

    let edges_data: Vec<_> = edges
        .into_iter()
        .filter(|e| e.weight >= q.min_weight)
        .map(|e| {
            json!({
                "from_id": e.from_id, "to_id": e.to_id,
                "relation": e.relation, "weight": e.weight,
            })
        })
        .collect();

    (StatusCode::OK, ok(json!({ "nodes": nodes, "edges": edges_data })))
}

// ─── POST /api/pipeline/trigger ──────────────────────────────

#[derive(Deserialize)]
pub struct PipelineTriggerRequest {
    pipeline: Option<String>,
    stage: Option<String>,
}

/// 触发管线（写操作，目前记录到 browser_behaviors 作为信号，
/// 实际执行依赖 Daemon tick；此端点不阻塞等待结果）
async fn pipeline_trigger_handler(
    State(store): State<Arc<Store>>,
    Json(req): Json<PipelineTriggerRequest>,
) -> Json<Value> {
    let target = req
        .pipeline
        .or(req.stage)
        .unwrap_or_else(|| "evening".to_string());
    let metadata = json!({ "trigger": target, "source": "bridge_api" }).to_string();
    if let Err(e) = store.save_browser_behavior("bridge", "pipeline_trigger", &metadata) {
        error!("pipeline_trigger_handler store error: {e}");
        return err_resp("记录触发信号失败");
    }
    Json(json!({ "success": true, "status": "triggered", "pipeline": target }))
}

// ─── 测试 ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::build_router;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app() -> (Router, Arc<Store>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = build_router(store.clone());
        (app, store)
    }

    // ── GET /api/memories ──

    #[tokio::test]
    async fn test_get_memories_empty() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/memories").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true);
        assert!(json["data"].is_array());
    }

    #[tokio::test]
    async fn test_get_memories_with_data() {
        let (app, store) = test_app();
        store.save_memory("identity", "software engineer", "test", 0.9).unwrap();
        store.save_memory("behavior", "morning person", "test", 0.8).unwrap();

        let resp = app
            .oneshot(Request::builder().uri("/api/memories?limit=10").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_get_memories_category_filter() {
        let (app, store) = test_app();
        store.save_memory("identity", "engineer", "test", 0.9).unwrap();
        store.save_memory("behavior", "early riser", "test", 0.8).unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/memories?category=identity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let arr = json["data"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["category"], "identity");
    }

    // ── GET /api/memories/:id ──

    #[tokio::test]
    async fn test_get_memory_by_id_found() {
        let (app, store) = test_app();
        let id = store.save_memory("identity", "coder", "test", 0.9).unwrap();
        store.add_tag(id, "work").unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/memories/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["id"], id);
        assert!(json["data"]["tags"].as_array().unwrap().contains(&Value::String("work".into())));
    }

    #[tokio::test]
    async fn test_get_memory_by_id_not_found() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/memories/9999").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── GET /api/tasks ──

    #[tokio::test]
    async fn test_get_tasks_empty() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/tasks").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_tasks_with_status_filter() {
        let (app, store) = test_app();
        store.create_task("write tests", "manual", None, Some("high"), None, None).unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/tasks?status=open&limit=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        assert_eq!(json["data"][0]["status"], "open");
    }

    // ── GET /api/observations ──

    #[tokio::test]
    async fn test_get_observations_empty() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/observations").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true);
    }

    // ── GET /api/reports/latest ──

    #[tokio::test]
    async fn test_reports_latest_not_found() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/reports/latest?type=morning")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_reports_latest_found() {
        let (app, store) = test_app();
        store.save_report("morning", "Good morning! Today looks productive.").unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/reports/latest?type=morning")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["type"], "morning");
        assert!(json["data"]["content"].as_str().unwrap().contains("Good morning"));
    }

    // ── GET /api/graph/connected/:id ──

    #[tokio::test]
    async fn test_graph_connected_returns_ok() {
        let (app, store) = test_app();
        let id1 = store.save_memory("identity", "a", "test", 0.9).unwrap();
        let id2 = store.save_memory("behavior", "b", "test", 0.8).unwrap();
        store.save_memory_edge(id1, id2, "similar", 0.7).unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/graph/connected/{id1}?depth=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true);
        assert!(json["data"]["nodes"].is_array());
        assert!(json["data"]["edges"].is_array());
    }

    // ── POST /api/pipeline/trigger ──

    #[tokio::test]
    async fn test_pipeline_trigger_records_signal() {
        let (app, store) = test_app();
        let body = serde_json::json!({ "pipeline": "evening" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/pipeline/trigger")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp_body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let json: Value = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["status"], "triggered");
        // 信号已记录到 browser_behaviors
        assert!(store.count_browser_behaviors().unwrap() >= 1);
    }
}
