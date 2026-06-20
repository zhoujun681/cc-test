use crate::types::VendorReport;
use comfy_table::Table;
use std::fs;
use std::path::Path;

/// 打印终端表格
pub fn print_results(reports: &[VendorReport]) {
    if reports.is_empty() {
        println!("No test results to display.");
        return;
    }

    let mut table = Table::new();
    table.set_header(vec![
        "Vendor",
        "Endpoint",
        "Model",
        "TTFB(avg)",
        "TTFB(min)",
        "TTFB(max)",
        "Total(avg)",
        "Status",
    ]);

    for report in reports {
        let status = if report.fail_count > 0 && report.success_count == 0 {
            "FAILED"
        } else if report.fail_count > 0 {
            "PARTIAL"
        } else {
            "OK"
        };

        let ttfb_avg = report.ttfb_avg_ms.map(|ms| colorize_latency(ms)).unwrap_or_else(|| "N/A".to_string());
        let ttfb_min = report.ttfb_min_ms.map(|ms| format!("{}ms", ms)).unwrap_or_else(|| "N/A".to_string());
        let ttfb_max = report.ttfb_max_ms.map(|ms| format!("{}ms", ms)).unwrap_or_else(|| "N/A".to_string());
        let total_avg = report.total_avg_ms.map(|ms| format!("{}ms", ms)).unwrap_or_else(|| "N/A".to_string());
        let model = report.model.as_deref().unwrap_or("N/A");

        table.add_row(vec![
            &report.vendor_name,
            &report.endpoint,
            model,
            &ttfb_avg,
            &ttfb_min,
            &ttfb_max,
            &total_avg,
            status,
        ]);
    }

    println!("\n{}", table);
}

/// 根据延迟返回颜色字符串
fn colorize_latency(ms: u64) -> String {
    if ms < 500 {
        format!("{}ms (fast)", ms)
    } else if ms < 1000 {
        format!("{}ms (medium)", ms)
    } else {
        format!("{}ms (slow)", ms)
    }
}

/// 导出 JSON 结果
pub fn export_json(reports: &[VendorReport], path: &str) -> Result<(), String> {
    let json = serde_json::to_string_pretty(reports)
        .map_err(|e| format!("Failed to serialize results: {}", e))?;
    fs::write(Path::new(path), json)
        .map_err(|e| format!("Failed to write file: {}", e))?;
    println!("Results exported to: {}", path);
    Ok(())
}
