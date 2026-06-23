use crate::types::{ApiFormat, TestResult, Vendor, VendorReport};
use futures_util::StreamExt;
use reqwest::Client;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// 测试单个供应商
pub async fn test_vendor(
    vendor: &Vendor,
    repeat: usize,
    timeout: u64,
    stream: bool,
) -> VendorReport {
    let mut report = VendorReport::new(vendor);

    for _ in 0..repeat {
        let result = test_single_request(vendor, timeout, stream).await;
        report.add_result(result);
    }

    report
}

/// 发送单次测试请求
async fn test_single_request(vendor: &Vendor, timeout: u64, stream: bool) -> TestResult {
    let client = match Client::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return TestResult {
                ttfb_ms: None,
                total_ms: None,
                success: false,
                error: Some(format!("Failed to build HTTP client: {}", e)),
                status_code: None,
            };
        }
    };

    let start = Instant::now();
    let ttfb;

    // 构建请求
    let request = match vendor.api_format {
        ApiFormat::Anthropic => build_anthropic_request(vendor, stream),
        ApiFormat::OpenaiChat => build_openai_chat_request(vendor, stream),
        ApiFormat::OpenaiResponses => build_openai_responses_request(vendor, stream),
    };

    let request = match request {
        Ok(r) => r,
        Err(e) => {
            return TestResult {
                ttfb_ms: None,
                total_ms: None,
                success: false,
                error: Some(format!("Failed to build request: {}", e)),
                status_code: None,
            };
        }
    };

    // 发送请求
    let response = match client.execute(request).await {
        Ok(r) => r,
        Err(e) => {
            return TestResult {
                ttfb_ms: None,
                total_ms: Some(start.elapsed().as_millis() as u64),
                success: false,
                error: Some(format!("Request failed: {}", e)),
                status_code: None,
            };
        }
    };

    let status = response.status().as_u16();

    // 记录 TTFB
    ttfb = start.elapsed().as_millis() as u64;

    // 检查状态码
    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return TestResult {
            ttfb_ms: Some(ttfb),
            total_ms: Some(start.elapsed().as_millis() as u64),
            success: false,
            error: Some(format!("HTTP {}: {}", status, error_text)),
            status_code: Some(status),
        };
    }

    // 流式读取响应（加内层超时兜底，防止慢滴流导致无限挂起）
    if stream {
        let read_timeout = Duration::from_secs(timeout + 5);
        let read_result = tokio::time::timeout(read_timeout, async {
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(_) => {}
                    Err(e) => return Err(format!("Stream error: {}", e)),
                }
            }
            Ok::<(), String>(())
        })
        .await;

        match read_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                return TestResult {
                    ttfb_ms: Some(ttfb),
                    total_ms: Some(start.elapsed().as_millis() as u64),
                    success: false,
                    error: Some(e),
                    status_code: Some(status),
                };
            }
            Err(_) => {
                return TestResult {
                    ttfb_ms: Some(ttfb),
                    total_ms: Some(start.elapsed().as_millis() as u64),
                    success: false,
                    error: Some(format!("Stream read timeout ({}s)", timeout + 5)),
                    status_code: Some(status),
                };
            }
        }
    } else {
        // 非流式，读取完整响应（同样加超时兜底）
        let read_timeout = Duration::from_secs(timeout + 5);
        let read_result = tokio::time::timeout(read_timeout, response.text()).await;
        match read_result {
            Ok(_) => {}
            Err(_) => {
                return TestResult {
                    ttfb_ms: Some(ttfb),
                    total_ms: Some(start.elapsed().as_millis() as u64),
                    success: false,
                    error: Some(format!("Response read timeout ({}s)", timeout + 5)),
                    status_code: Some(status),
                };
            }
        }
    }

    let total = start.elapsed().as_millis() as u64;

    TestResult {
        ttfb_ms: Some(ttfb),
        total_ms: Some(total),
        success: true,
        error: None,
        status_code: Some(status),
    }
}

/// 构建 Anthropic 请求
fn build_anthropic_request(vendor: &Vendor, stream: bool) -> Result<reqwest::Request, String> {
    let base = vendor.endpoint.trim_end_matches('/');
    // 避免重复拼接：若已含 /v1/messages 或 /messages 则直接用，否则补 /v1/messages
    let url = if base.ends_with("/v1/messages") || base.ends_with("/messages") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{}/messages", base)
    } else {
        format!("{}/v1/messages", base)
    };
    let model = vendor.model.as_deref().unwrap_or("claude-3-haiku-20240307");

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": stream
    });

    reqwest::Client::new()
        .post(&url)
        .header("x-api-key", &vendor.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .build()
        .map_err(|e| e.to_string())
}

/// 构建 OpenAI Chat 请求
fn build_openai_chat_request(vendor: &Vendor, stream: bool) -> Result<reqwest::Request, String> {
    let url = build_endpoint_url(&vendor.endpoint, "chat/completions");
    let model = vendor.model.as_deref().unwrap_or("gpt-3.5-turbo");

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": stream
    });

    reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {}", &vendor.api_key))
        .header("content-type", "application/json")
        .json(&body)
        .build()
        .map_err(|e| e.to_string())
}

/// 构建 OpenAI Responses 请求
fn build_openai_responses_request(
    vendor: &Vendor,
    stream: bool,
) -> Result<reqwest::Request, String> {
    let url = build_endpoint_url(&vendor.endpoint, "responses");
    let model = vendor.model.as_deref().unwrap_or("gpt-4o-mini");

    let body = serde_json::json!({
        "model": model,
        "input": "Hi",
        "stream": stream
    });

    reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {}", &vendor.api_key))
        .header("content-type", "application/json")
        .json(&body)
        .build()
        .map_err(|e| e.to_string())
}

/// 智能拼接端点 URL：
/// - 若 endpoint 已含完整路径（如 /v1 或 /v1/messages），直接追加 suffix
/// - 否则补 /v1 前缀（Anthropic 除外，Anthropic 另行处理）
fn build_endpoint_url(endpoint: &str, suffix: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.ends_with("/v1") || base.contains("/v1/") || base.ends_with("/api") {
        format!("{}/{}", base, suffix)
    } else {
        format!("{}/v1/{}", base, suffix)
    }
}

/// 批量测试供应商
pub async fn test_vendors_batch(
    vendors: &[Vendor],
    repeat: usize,
    timeout: u64,
    stream: bool,
    concurrency: usize,
    progress_tx: Option<mpsc::UnboundedSender<(String, VendorReport)>>,
) -> Vec<VendorReport> {
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    for vendor in vendors {
        let semaphore = semaphore.clone();
        let vendor = vendor.clone();
        let progress_tx = progress_tx.clone();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let report = test_vendor(&vendor, repeat, timeout, stream).await;

            if let Some(tx) = progress_tx {
                let _ = tx.send((vendor.id.clone(), report.clone()));
            }

            report
        });

        handles.push(handle);
    }

    let mut reports = Vec::new();
    for handle in handles {
        if let Ok(report) = handle.await {
            reports.push(report);
        }
    }

    reports
}
