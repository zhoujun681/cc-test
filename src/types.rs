use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// API 格式枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    Anthropic,
    OpenaiChat,
    OpenaiResponses,
}

impl std::fmt::Display for ApiFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiFormat::Anthropic => write!(f, "anthropic"),
            ApiFormat::OpenaiChat => write!(f, "openai_chat"),
            ApiFormat::OpenaiResponses => write!(f, "openai_responses"),
        }
    }
}

impl std::str::FromStr for ApiFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anthropic" | "anthropic_messages" | "messages" => Ok(ApiFormat::Anthropic),
            "openai_chat" | "openai" | "chat" | "chat_completions" => Ok(ApiFormat::OpenaiChat),
            "openai_responses" | "responses" => Ok(ApiFormat::OpenaiResponses),
            _ => Err(format!("Unknown API format: {}", s)),
        }
    }
}

/// 供应商配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vendor {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub api_key: String,
    pub api_format: ApiFormat,
    pub model: Option<String>,
    pub group: String,
}

/// 单次测试结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub ttfb_ms: Option<u64>,
    pub total_ms: Option<u64>,
    pub success: bool,
    pub error: Option<String>,
    pub status_code: Option<u16>,
}

/// 供应商汇总报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorReport {
    pub vendor_id: String,
    pub vendor_name: String,
    pub endpoint: String,
    pub model: Option<String>,
    pub api_format: ApiFormat,
    pub group: String,
    pub results: Vec<TestResult>,
    pub ttfb_avg_ms: Option<u64>,
    pub ttfb_min_ms: Option<u64>,
    pub ttfb_max_ms: Option<u64>,
    pub total_avg_ms: Option<u64>,
    pub total_min_ms: Option<u64>,
    pub total_max_ms: Option<u64>,
    pub success_count: usize,
    pub fail_count: usize,
}

impl VendorReport {
    pub fn new(vendor: &Vendor) -> Self {
        Self {
            vendor_id: vendor.id.clone(),
            vendor_name: vendor.name.clone(),
            endpoint: vendor.endpoint.clone(),
            model: vendor.model.clone(),
            api_format: vendor.api_format.clone(),
            group: vendor.group.clone(),
            results: Vec::new(),
            ttfb_avg_ms: None,
            ttfb_min_ms: None,
            ttfb_max_ms: None,
            total_avg_ms: None,
            total_min_ms: None,
            total_max_ms: None,
            success_count: 0,
            fail_count: 0,
        }
    }

    pub fn add_result(&mut self, result: TestResult) {
        if result.success {
            self.success_count += 1;
        } else {
            self.fail_count += 1;
        }
        self.results.push(result);
        self.calculate_stats();
    }

    fn calculate_stats(&mut self) {
        let successful: Vec<&TestResult> = self.results.iter().filter(|r| r.success).collect();
        if successful.is_empty() {
            return;
        }

        let ttfbs: Vec<u64> = successful.iter().filter_map(|r| r.ttfb_ms).collect();
        let totals: Vec<u64> = successful.iter().filter_map(|r| r.total_ms).collect();

        if !ttfbs.is_empty() {
            self.ttfb_avg_ms = Some(ttfbs.iter().sum::<u64>() / ttfbs.len() as u64);
            self.ttfb_min_ms = ttfbs.iter().copied().min();
            self.ttfb_max_ms = ttfbs.iter().copied().max();
        }

        if !totals.is_empty() {
            self.total_avg_ms = Some(totals.iter().sum::<u64>() / totals.len() as u64);
            self.total_min_ms = totals.iter().copied().min();
            self.total_max_ms = totals.iter().copied().max();
        }
    }
}

/// 手工测试请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTestRequest {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub api_format: ApiFormat,
    pub repeat: Option<usize>,
}

/// 批量测试请求
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BatchTestRequest {
    #[serde(default)]
    pub vendor_ids: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    /// 并发数（None 则使用默认值）
    #[serde(default)]
    pub concurrency: Option<usize>,
}

/// 单个测试请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleTestRequest {
    pub vendor_id: String,
}

/// WebSocket 进度消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "progress")]
    Progress {
        current: usize,
        total: usize,
        vendor: String,
    },
    #[serde(rename = "result")]
    Result {
        vendor_id: String,
        result: VendorReport,
    },
    #[serde(rename = "complete")]
    Complete {
        summary: TestSummary,
    },
}

/// 测试汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSummary {
    pub total_vendors: usize,
    pub success_count: usize,
    pub fail_count: usize,
    pub avg_ttfb_ms: Option<u64>,
}

/// 全局应用状态
pub struct AppState {
    pub vendors: tokio::sync::RwLock<Vec<Vendor>>,
    pub reports: tokio::sync::RwLock<HashMap<String, VendorReport>>,
    pub db_path: tokio::sync::RwLock<Option<String>>,
    pub ws_senders: tokio::sync::RwLock<Vec<tokio::sync::mpsc::UnboundedSender<WsMessage>>>,
    /// 默认每个 API 测试次数
    pub default_repeat: std::sync::atomic::AtomicUsize,
    /// 默认超时秒数
    pub default_timeout: std::sync::atomic::AtomicU64,
    /// 是否启用流式（测量 TTFB）
    pub default_stream: std::sync::atomic::AtomicBool,
    /// 默认并发数
    pub default_concurrency: std::sync::atomic::AtomicUsize,
}

impl AppState {
    pub fn new(db_path: Option<String>) -> Self {
        Self {
            vendors: tokio::sync::RwLock::new(Vec::new()),
            reports: tokio::sync::RwLock::new(HashMap::new()),
            db_path: tokio::sync::RwLock::new(db_path),
            ws_senders: tokio::sync::RwLock::new(Vec::new()),
            default_repeat: std::sync::atomic::AtomicUsize::new(3),
            default_timeout: std::sync::atomic::AtomicU64::new(30),
            default_stream: std::sync::atomic::AtomicBool::new(true),
            default_concurrency: std::sync::atomic::AtomicUsize::new(10),
        }
    }

    /// 设置默认测试参数（从 CLI 参数读取）
    pub fn set_defaults(&self, repeat: usize, timeout: u64, stream: bool) {
        self.default_repeat.store(repeat, std::sync::atomic::Ordering::Relaxed);
        self.default_timeout.store(timeout, std::sync::atomic::Ordering::Relaxed);
        self.default_stream.store(stream, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_concurrency(&self, n: usize) {
        self.default_concurrency.store(n, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get_repeat(&self) -> usize {
        self.default_repeat.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get_timeout(&self) -> u64 {
        self.default_timeout.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get_stream(&self) -> bool {
        self.default_stream.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get_concurrency(&self) -> usize {
        self.default_concurrency.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn broadcast(&self, msg: WsMessage) {
        let senders = self.ws_senders.read().await;
        for sender in senders.iter() {
            let _ = sender.send(msg.clone());
        }
    }
}
