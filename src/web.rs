use crate::types::*;
use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Embed)]
#[folder = "static/"]
struct Asset;

pub async fn start_server(state: Arc<AppState>, port: u16) -> Result<(), String> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/vendors", get(get_vendors))
        .route("/api/config/reload", post(reload_config))
        .route("/api/config/db", post(load_db))
        .route("/api/config/db/upload", post(upload_db))
        .route("/api/config/db", get(get_db_path))
        .route("/api/settings", get(get_settings))
        .route("/api/settings", post(update_settings))
        .route("/api/test/single", post(test_single))
        .route("/api/test/batch", post(test_batch))
        .route("/api/test/custom", post(test_custom))
        .route("/api/test/status", get(get_test_status))
        .route("/api/results", get(get_results))
        .route("/ws", get(ws_handler))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind: {}", e))?;

    println!("Web server running at http://localhost:{}", port);

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e))?;

    Ok(())
}

async fn index_handler() -> impl IntoResponse {
    match Asset::get("index.html") {
        Some(content) => {
            let data = content.data.into_owned();
            let text = String::from_utf8(data).unwrap_or_default();
            Html(text)
        }
        None => Html("<h1>404 - Page not found</h1>".to_string()),
    }
}

async fn get_vendors(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let vendors = state.vendors.read().await;
    Json(vendors.clone())
}

async fn reload_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let db_path = state.db_path.read().await.clone();
    match db_path {
        Some(path) => match crate::config::load_vendors(&path) {
            Ok(vendors) => {
                let count = vendors.len();
                let mut v = state.vendors.write().await;
                *v = vendors;
                (StatusCode::OK, Json(serde_json::json!({"message": format!("Loaded {} vendors", count)})))
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))),
        },
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Database path not configured"}))),
    }
}

/// 加载指定路径的数据库文件
#[derive(Debug, Clone, serde::Deserialize)]
struct LoadDbRequest {
    /// CC Switch 数据库文件路径
    db_path: String,
}

async fn load_db(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoadDbRequest>,
) -> impl IntoResponse {
    let path = req.db_path.trim();
    if path.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "db_path is required"})));
    }

    // 校验文件存在
    let p = std::path::Path::new(path);
    if !p.exists() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("File not found: {}", path)})));
    }
    if !p.is_file() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Not a file"})));
    }

    // 尝试加载
    match crate::config::load_vendors(path) {
        Ok(vendors) => {
            let count = vendors.len();
            {
                let mut db = state.db_path.write().await;
                *db = Some(path.to_string());
            }
            {
                let mut v = state.vendors.write().await;
                *v = vendors;
            }
            {
                let mut r = state.reports.write().await;
                r.clear();
            }
            (StatusCode::OK, Json(serde_json::json!({"message": format!("Loaded {} vendors from {}", count, path), "db_path": path})))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))),
    }
}

/// 上传数据库文件（multipart），后端保存为临时文件并加载
async fn upload_db(
    State(state): State<Arc<AppState>>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    use std::io::Write;

    // 读取上传的字段
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name = "uploaded.db".to_string();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            if let Some(fname) = field.file_name() {
                file_name = fname.to_string();
            }
            match field.bytes().await {
                Ok(bytes) => file_bytes = Some(bytes.to_vec()),
                Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("Read file failed: {}", e)}))),
            }
        }
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "No file uploaded"}))),
    };

    // 保存到系统临时目录
    let temp_dir = std::env::temp_dir();
    let save_path = temp_dir.join(format!("cc-switch-test-{}-{}", std::process::id(), file_name));
    let mut f = match std::fs::File::create(&save_path) {
        Ok(f) => f,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Save file failed: {}", e)}))),
    };
    if let Err(e) = f.write_all(&bytes) {
        let _ = std::fs::remove_file(&save_path);
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Write file failed: {}", e)})));
    }

    let save_path_str = save_path.to_string_lossy().to_string();

    // 加载数据库
    match crate::config::load_vendors(&save_path_str) {
        Ok(vendors) => {
            let count = vendors.len();
            {
                let mut db = state.db_path.write().await;
                *db = Some(save_path_str.clone());
            }
            {
                let mut v = state.vendors.write().await;
                *v = vendors;
            }
            {
                let mut r = state.reports.write().await;
                r.clear();
            }
            (StatusCode::OK, Json(serde_json::json!({
                "message": format!("Loaded {} vendors", count),
                "db_path": save_path_str,
            })))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&save_path);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e})))
        }
    }
}

/// 查询当前数据库路径
async fn get_db_path(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let db = state.db_path.read().await.clone();
    Json(serde_json::json!({"db_path": db.unwrap_or_default()}))
}

/// 查询/设置默认测试参数（并发数、重复次数等）
#[derive(Debug, Clone, serde::Deserialize)]
struct SettingsRequest {
    #[serde(default)]
    pub concurrency: Option<usize>,
    #[serde(default)]
    pub repeat: Option<usize>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

async fn get_settings(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(serde_json::json!({
        "concurrency": state.get_concurrency(),
        "repeat": state.get_repeat(),
        "timeout": state.get_timeout(),
        "stream": state.get_stream(),
    }))
}

async fn update_settings(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SettingsRequest>,
) -> impl IntoResponse {
    if let Some(c) = req.concurrency {
        state.set_concurrency(c.max(1));
    }
    if let Some(r) = req.repeat {
        state.default_repeat.store(r.max(1), std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(t) = req.timeout {
        state.default_timeout.store(t.max(1), std::sync::atomic::Ordering::Relaxed);
    }
    Json(serde_json::json!({
        "message": "Settings updated",
        "concurrency": state.get_concurrency(),
        "repeat": state.get_repeat(),
        "timeout": state.get_timeout(),
    }))
}

async fn test_single(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SingleTestRequest>,
) -> impl IntoResponse {
    let vendors = state.vendors.read().await;
    let vendor = vendors.iter().find(|v| v.id == req.vendor_id).cloned();
    drop(vendors);

    match vendor {
        Some(v) => {
            let repeat = state.get_repeat();
            let timeout = state.get_timeout();
            let stream = state.get_stream();
            // 加超时兜底，防止单个测试挂起
            let single_timeout = std::time::Duration::from_secs(timeout * repeat as u64 + 15);
            let report = match tokio::time::timeout(
                single_timeout,
                crate::tester::test_vendor(&v, repeat, timeout, stream),
            ).await {
                Ok(r) => r,
                Err(_) => {
                    return (StatusCode::GATEWAY_TIMEOUT, Json(serde_json::json!({
                        "error": format!("Test timeout ({}s)", timeout * repeat as u64 + 15)
                    })));
                }
            };
            {
                let mut reports = state.reports.write().await;
                reports.insert(v.id.clone(), report.clone());
            }
            let _ = state.broadcast(WsMessage::Result {
                vendor_id: v.id.clone(),
                result: report.clone(),
            }).await;
            (StatusCode::OK, Json(serde_json::json!(report)))
        }
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Vendor not found"}))),
    }
}

async fn test_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchTestRequest>,
) -> impl IntoResponse {
    let vendors = state.vendors.read().await;
    // 筛选逻辑：优先用 vendor_ids；若为空则用 groups；两者都为空则测全部
    let to_test: Vec<Vendor> = if !req.vendor_ids.is_empty() {
        vendors.iter().filter(|v| req.vendor_ids.contains(&v.id)).cloned().collect()
    } else if !req.groups.is_empty() {
        vendors.iter().filter(|v| req.groups.contains(&v.group)).cloned().collect()
    } else {
        vendors.clone()
    };
    drop(vendors);

    let total = to_test.len();
    if total == 0 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "No vendors to test"})));
    }

    // 从 state 读取默认测试参数
    let repeat = state.get_repeat();
    let timeout = state.get_timeout();
    let stream = state.get_stream();
    // 并发数：优先使用请求中的值，否则用 state 默认值；上限 = 供应商数
    let concurrency = req.concurrency.unwrap_or_else(|| state.get_concurrency()).max(1).min(total);

    let state_clone = Arc::clone(&state);
    // 设置批量测试状态
    state.batch_running.store(true, std::sync::atomic::Ordering::SeqCst);
    state.batch_done.store(0, std::sync::atomic::Ordering::SeqCst);
    state.batch_total.store(total, std::sync::atomic::Ordering::SeqCst);
    tokio::spawn(async move {
        let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let total_usize = total;

        // 批量任务外层超时兜底（防止个别 vendor 挂起导致整个任务永不结束）
        let batch_timeout = std::time::Duration::from_secs(
            (total as u64 * repeat as u64 * (timeout + 5)) / concurrency.max(1) as u64 + 60
        );

        use futures_util::stream::{self, StreamExt};
        let vendors_stream = stream::iter(to_test);
        let batch_work = vendors_stream
            .for_each_concurrent(concurrency, |vendor| {
                let state_clone = Arc::clone(&state_clone);
                let completed = Arc::clone(&completed);
                async move {
                    // 测试前推送 progress（current = 已完成数 + 1，表示正在测第几个）
                    let before = completed.fetch_add(0, std::sync::atomic::Ordering::SeqCst);
                    let _ = state_clone.broadcast(WsMessage::Progress {
                        current: before + 1,
                        total: total_usize,
                        vendor: vendor.name.clone(),
                    }).await;

                    // 执行测试
                    let report = crate::tester::test_vendor(&vendor, repeat, timeout, stream).await;

                    // 写入结果并广播
                    {
                        let mut reports = state_clone.reports.write().await;
                        reports.insert(vendor.id.clone(), report.clone());
                    }
                    let _ = state_clone.broadcast(WsMessage::Result {
                        vendor_id: vendor.id.clone(),
                        result: report,
                    }).await;

                    // 更新完成计数
                    completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    state_clone.batch_done.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            });

        // 无论超时还是正常完成，都继续往下发 Complete（避免前端永久卡死）
        let _ = tokio::time::timeout(batch_timeout, batch_work).await;

        // 标记批量测试结束
        state_clone.batch_running.store(false, std::sync::atomic::Ordering::SeqCst);

        // 全部完成（或超时），推送 complete
        let reports = state_clone.reports.read().await;
        let summary = build_summary(&reports);
        let _ = state_clone.broadcast(WsMessage::Complete { summary }).await;
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({
        "message": format!("Testing {} vendors", total),
        "concurrency": concurrency,
        "repeat": repeat,
        "timeout": timeout
    })))
}

async fn test_custom(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CustomTestRequest>,
) -> impl IntoResponse {
    let vendor = Vendor {
        id: "custom".to_string(),
        name: "Custom Test".to_string(),
        endpoint: req.endpoint,
        api_key: req.api_key,
        api_format: req.api_format,
        model: Some(req.model),
        group: "custom".to_string(),
    };

    let repeat = req.repeat.unwrap_or_else(|| state.get_repeat());
    let timeout = state.get_timeout();
    let stream = state.get_stream();
    // 加超时兜底，防止单个测试挂起
    let single_timeout = std::time::Duration::from_secs(timeout * repeat as u64 + 15);
    let report = match tokio::time::timeout(
        single_timeout,
        crate::tester::test_vendor(&vendor, repeat, timeout, stream),
    ).await {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::GATEWAY_TIMEOUT, Json(serde_json::json!({
                "error": format!("Test timeout ({}s)", timeout * repeat as u64 + 15)
            })));
        }
    };

    {
        let mut reports = state.reports.write().await;
        reports.insert("custom".to_string(), report.clone());
    }
    let _ = state.broadcast(WsMessage::Result {
        vendor_id: "custom".to_string(),
        result: report.clone(),
    }).await;

    (StatusCode::OK, Json(serde_json::json!(report)))
}

async fn get_test_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let running = state.batch_running.load(std::sync::atomic::Ordering::SeqCst);
    let done = state.batch_done.load(std::sync::atomic::Ordering::SeqCst);
    let total = state.batch_total.load(std::sync::atomic::Ordering::SeqCst);
    Json(serde_json::json!({
        "running": running,
        "done": done,
        "total": total,
    }))
}

async fn get_results(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let reports = state.reports.read().await;
    let results: Vec<VendorReport> = reports.values().cloned().collect();
    Json(results)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WsMessage>();

    {
        let mut senders = state.ws_senders.write().await;
        senders.push(tx);
    }

    // Forward messages to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Receive messages (for client commands)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // Cleanup
    let mut senders = state.ws_senders.write().await;
    senders.retain(|s| !s.is_closed());
}

fn build_summary(reports: &std::collections::HashMap<String, VendorReport>) -> TestSummary {
    let total = reports.len();
    let success = reports.values().filter(|r| r.success_count > 0).count();
    let fail = total - success;
    let avg_ttfb = reports.values()
        .filter_map(|r| r.ttfb_avg_ms)
        .collect::<Vec<_>>();
    let avg = if avg_ttfb.is_empty() {
        None
    } else {
        Some(avg_ttfb.iter().sum::<u64>() / avg_ttfb.len() as u64)
    };

    TestSummary {
        total_vendors: total,
        success_count: success,
        fail_count: fail,
        avg_ttfb_ms: avg,
    }
}
