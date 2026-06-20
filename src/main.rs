mod config;
mod reporter;
mod tester;
mod types;
mod web;

use clap::Parser;
use std::sync::Arc;
use types::AppState;

#[derive(Parser)]
#[command(name = "cc-test2")]
#[command(about = "CC Switch API Tester - Test API endpoints from CC Switch configuration")]
struct Cli {
    /// CC Switch database path (auto-detect if not specified)
    #[arg(long)]
    db_path: Option<String>,

    /// Number of times to test each API (default: 3)
    #[arg(long, default_value = "3")]
    repeat: usize,

    /// Maximum concurrent tests (default: unlimited)
    #[arg(long)]
    concurrency: Option<usize>,

    /// Export results to JSON file
    #[arg(long)]
    output: Option<String>,

    /// Only test specific vendors (can be specified multiple times)
    #[arg(long)]
    vendor: Vec<String>,

    /// Override model name for testing
    #[arg(long)]
    model: Option<String>,

    /// Request timeout in seconds (default: 30)
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Web server port (default: 38080)
    #[arg(long, default_value = "38080")]
    port: u16,

    /// Don't start web server, only output to terminal
    #[arg(long)]
    no_web: bool,

    /// Run terminal batch test immediately on startup (before web server)
    #[arg(long)]
    run: bool,

    /// Don't use streaming requests (cannot measure TTFB)
    #[arg(long)]
    no_stream: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Find database path
    let db_path = match cli.db_path {
        Some(path) => Some(path),
        None => config::find_db_path().map(|p| p.to_string_lossy().to_string()),
    };

    let db_path = match db_path {
        Some(path) => {
            println!("Using database: {}", path);
            path
        }
        None => {
            eprintln!("Error: Could not find CC Switch database. Please specify with --db-path");
            std::process::exit(1);
        }
    };

    // Load vendors
    let vendors = match config::load_vendors(&db_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error loading vendors: {}", e);
            std::process::exit(1);
        }
    };

    if vendors.is_empty() {
        println!("No vendors found in database.");
        std::process::exit(0);
    }

    println!("Found {} vendors", vendors.len());

    // Filter vendors if specified
    let vendors_to_show: Vec<_> = if cli.vendor.is_empty() {
        vendors
    } else {
        vendors
            .into_iter()
            .filter(|v| cli.vendor.contains(&v.name))
            .collect()
    };

    if vendors_to_show.is_empty() {
        println!("No matching vendors found.");
        std::process::exit(0);
    }

    // Create app state
    let state = Arc::new(AppState::new(Some(db_path)));
    {
        let mut v = state.vendors.write().await;
        *v = vendors_to_show.clone();
    }
    // 保存默认测试参数到 state（供 Web 界面调用）
    state.set_defaults(cli.repeat, cli.timeout, !cli.no_stream);

    // --no-web：纯终端模式，执行一次性批量测试后退出
    if cli.no_web {
        println!("\nStarting terminal tests...");
        let concurrency = cli.concurrency.unwrap_or(vendors_to_show.len());
        let reports = run_terminal_batch(
            &vendors_to_show,
            cli.repeat,
            cli.timeout,
            !cli.no_stream,
            concurrency,
        )
        .await;

        // Store results
        {
            let mut results = state.reports.write().await;
            for report in reports {
                results.insert(report.vendor_id.clone(), report);
            }
        }

        // Print terminal results
        let reports = state.reports.read().await;
        let report_vec: Vec<_> = reports.values().cloned().collect();
        reporter::print_results(&report_vec);

        // Export JSON if requested
        if let Some(output_path) = cli.output {
            reporter::export_json(&report_vec, &output_path)?;
        }
        return Ok(());
    }

    // --run：启动 Web 前，先在终端跑一次批量测试（带实时进度）
    if cli.run {
        println!("\nPre-running terminal tests before starting web server...");
        let concurrency = cli.concurrency.unwrap_or(5.min(vendors_to_show.len()));
        let reports = run_terminal_batch(
            &vendors_to_show,
            cli.repeat,
            cli.timeout,
            !cli.no_stream,
            concurrency,
        )
        .await;
        // 把结果存入 state，供 Web 界面直接显示
        {
            let mut results = state.reports.write().await;
            for report in reports {
                results.insert(report.vendor_id.clone(), report);
            }
        }
        let reports = state.reports.read().await;
        let report_vec: Vec<_> = reports.values().cloned().collect();
        reporter::print_results(&report_vec);
        if let Some(output_path) = &cli.output {
            reporter::export_json(&report_vec, output_path)?;
        }
        println!("\nTerminal tests done. Starting web server...");
    }

    // 默认：启动 Web 服务器，让用户在网页上选择后再测试
    let url = format!("http://localhost:{}", cli.port);
    println!("Opening browser: {}", url);

    // Open browser in background
    let _ = open::that(&url);

    // Start server
    web::start_server(state, cli.port).await?;

    Ok(())
}

/// 终端批量测试：带实时进度输出，返回所有报告
async fn run_terminal_batch(
    vendors: &[types::Vendor],
    repeat: usize,
    timeout: u64,
    stream: bool,
    concurrency: usize,
) -> Vec<types::VendorReport> {
    use futures_util::stream::{self, StreamExt};
    let total = vendors.len();
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reports = Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(total)));

    println!("Testing {} vendors (concurrency={}, repeat={}, timeout={}s)",
        total, concurrency.min(total), repeat, timeout);
    println!("{:-<70}", "");

    let vendors_iter = stream::iter(vendors.iter().cloned());
    vendors_iter
        .for_each_concurrent(concurrency, |vendor| {
            let completed = Arc::clone(&completed);
            let reports = Arc::clone(&reports);
            async move {
                let now = completed.load(std::sync::atomic::Ordering::SeqCst) + 1;
                println!("[{}/{}] Testing {} ({}) ...", now, total, vendor.name, vendor.endpoint);

                let report = tester::test_vendor(&vendor, repeat, timeout, stream).await;

                let done = completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let status_tag = if report.fail_count > 0 && report.success_count == 0 {
                    "FAILED"
                } else if report.fail_count > 0 {
                    "PARTIAL"
                } else {
                    "OK"
                };
                let ttfb_str = report.ttfb_avg_ms
                    .map(|t| format!("{}ms", t))
                    .unwrap_or_else(|| "-".to_string());
                println!("[{}/{}] ✓ {} [{}] TTFB(avg)={}",
                    done, total, vendor.name, status_tag, ttfb_str);

                let mut r = reports.lock().await;
                r.push(report);
            }
        })
        .await;

    let r = reports.lock().await;
    r.clone()
}
