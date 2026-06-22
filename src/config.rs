use crate::types::{ApiFormat, Vendor};
use rusqlite::Connection;
use std::path::PathBuf;

/// 自动定位 CC Switch 数据库路径
pub fn find_db_path() -> Option<PathBuf> {
    // 优先级 1：用户主目录下的 .cc-switch/cc-switch.db（CC Switch 默认位置）
    // Windows: %USERPROFILE%\.cc-switch\cc-switch.db
    // macOS/Linux: ~/.cc-switch/cc-switch.db
    let candidates = vec![
        dirs::home_dir().map(|p| p.join(".cc-switch").join("cc-switch.db")),
        // Windows: %APPDATA%\cc-switch\cc-switch.db
        dirs::config_dir().map(|p| p.join("cc-switch").join("cc-switch.db")),
        // Windows: %LOCALAPPDATA%\cc-switch\cc-switch.db
        dirs::data_local_dir().map(|p| p.join("cc-switch").join("cc-switch.db")),
        // macOS: ~/Library/Application Support/cc-switch/config.db
        dirs::config_dir().map(|p| p.join("cc-switch").join("config.db")),
        // Linux: ~/.local/share/cc-switch/config.db
        dirs::data_local_dir().map(|p| p.join("cc-switch").join("config.db")),
        // 优先级 2：当前工作目录（CWD）—— 方便测试其他电脑的数据库
        std::env::current_dir().ok().map(|p| p.join("cc-switch.db")),
        std::env::current_dir().ok().map(|p| p.join("cc-switch").join("cc-switch.db")),
        // 优先级 2：可执行文件所在目录
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("cc-switch.db"))),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 优先级 3：搜索常见目录下的 cc-switch 子目录
    let search_dirs = vec![
        dirs::home_dir(),
        std::env::current_dir().ok(),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())),
        dirs::config_dir(),
        dirs::data_local_dir(),
        dirs::data_dir(),
    ];

    for dir in search_dirs.into_iter().flatten() {
        if let Some(db) = search_db_in_dir(&dir) {
            return Some(db);
        }
    }

    None
}

fn search_db_in_dir(dir: &std::path::Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name()?.to_str()?;
            if dir_name.contains("cc-switch") || dir_name.contains("ccswitch") {
                // 搜索这个子目录
                if let Some(db) = find_db_file_in_subdir(&path) {
                    return Some(db);
                }
            }
        }
    }
    None
}

fn find_db_file_in_subdir(dir: &std::path::Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let name = path.file_name()?.to_str()?;
            if name.ends_with(".db") || name.ends_with(".sqlite") || name.ends_with(".sqlite3") {
                return Some(path);
            }
        }
    }
    None
}

/// 从数据库读取供应商配置
pub fn load_vendors(db_path: &str) -> Result<Vec<Vendor>, String> {
    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let mut vendors = Vec::new();
    let mut skipped = Vec::new();

    // 新版本 CC Switch：providers 表本身有 app_type 列，meta 列含 apiFormat
    // 旧版本：app_type 在 provider_endpoints 表
    // 兼容两种：优先 providers.app_type，回退 provider_endpoints.app_type
    let query = r#"
        SELECT 
            p.id,
            p.app_type,
            p.name,
            p.settings_config,
            p.meta,
            pe.url as endpoint_url
        FROM providers p
        LEFT JOIN provider_endpoints pe ON p.id = pe.provider_id
        WHERE p.settings_config IS NOT NULL
    "#;

    let mut stmt = conn.prepare(query).map_err(|e| format!("Failed to prepare query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,                   // id
                row.get::<_, Option<String>>(1)?,           // p.app_type (新版本)
                row.get::<_, String>(2)?,                   // name
                row.get::<_, String>(3)?,                   // settings_config
                row.get::<_, Option<String>>(4)?,           // meta
                row.get::<_, Option<String>>(5)?,           // endpoint_url
            ))
        })
        .map_err(|e| format!("Failed to query: {}", e))?;

    // 用 seen 集合去重（LEFT JOIN 可能导致同一 provider 多行）
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let (id, p_app_type, name, settings_json, meta, endpoint_url) =
            row.map_err(|e| format!("Failed to read row: {}", e))?;

        if !seen.insert(id.clone()) {
            continue; // 已处理过该 provider（多 endpoint 场景取第一个）
        }

        // 解析配置 JSON
        match parse_vendor_config(&id, &name, &settings_json, endpoint_url, p_app_type, meta) {
            Ok(vendor) => {
                vendors.push(vendor);
            }
            Err(e) => {
                skipped.push((name.clone(), e));
            }
        }
    }

    if !skipped.is_empty() {
        println!("\n[WARN] Skipped {} vendors:", skipped.len());
        for (name, reason) in &skipped {
            println!("  - {}: {}", name, reason);
        }
    }

    Ok(vendors)
}

/// 从 map 中查找 API Key 字段（支持多种命名方式）
fn find_api_key_in_map(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let key_names = [
        "api_key", "apikey", "key", "token", "auth_token", "access_token",
        "secret", "api_secret", "api_token",
    ];
    
    // 先查找精确匹配
    for key_name in &key_names {
        if let Some(val) = map.get(*key_name) {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    
    // 查找包含 key/token/secret 的字段
    for (key, val) in map.iter() {
        let key_lower = key.to_lowercase();
        if key_lower.contains("key") || key_lower.contains("token") || key_lower.contains("secret") {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    
    None
}

/// 从 TOML 字符串中提取端点 URL（支持 url 和 base_url，优先 base_url）
fn extract_url_from_toml(toml_str: &str) -> Option<String> {
    let mut base_url: Option<String> = None;
    let mut plain_url: Option<String> = None;
    for line in toml_str.lines() {
        let line = line.trim();
        if !line.contains('=') {
            continue;
        }
        let mut parts = line.splitn(2, '=');
        let key = parts.next()?.trim();
        let val = parts.next()?.trim();
        let val = val.trim_matches('"').trim_matches('\'');
        if val.is_empty() || !val.starts_with("http") {
            continue;
        }
        if key == "base_url" {
            base_url = Some(val.to_string());
        } else if key == "url" {
            plain_url = Some(val.to_string());
        }
    }
    base_url.or(plain_url)
}

/// 从 TOML 字符串中提取模型名称
fn extract_model_from_toml(toml_str: &str) -> Option<String> {
    for line in toml_str.lines() {
        let line = line.trim();
        if line.starts_with("model") && line.contains('=') && !line.contains("model_provider") && !line.contains("model_context") && !line.contains("model_auto") {
            if let Some(model) = line.split('=').nth(1) {
                let model = model.trim().trim_matches('"').trim_matches('\'');
                if !model.is_empty() {
                    return Some(model.to_string());
                }
            }
        }
    }
    None
}

/// 从 map 中查找 URL 字段（支持多种命名方式）
fn find_url_in_map(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let url_keys = [
        "url", "base_url", "base", "endpoint", "api_url", "api_base",
        "api_endpoint", "host", "server", "server_url",
    ];
    
    for key in &url_keys {
        if let Some(val) = map.get(*key) {
            if let Some(s) = val.as_str() {
                if !s.is_empty() && s.starts_with("http") {
                    return Some(s.to_string());
                }
            }
        }
    }
    
    // 搜索任何包含 url/base/endpoint 的键
    for (key, val) in map.iter() {
        let key_lower = key.to_lowercase();
        if key_lower.contains("url") || key_lower.contains("base") || key_lower.contains("endpoint") {
            if let Some(s) = val.as_str() {
                if !s.is_empty() && s.starts_with("http") {
                    return Some(s.to_string());
                }
            }
        }
    }
    
    None
}

/// 解析供应商配置 JSON
fn parse_vendor_config(
    id: &str,
    name: &str,
    config_json: &str,
    endpoint_url: Option<String>,
    app_type: Option<String>,
    meta_json: Option<String>,
) -> Result<Vendor, String> {
    let config: serde_json::Value =
        serde_json::from_str(config_json).map_err(|e| format!("Invalid JSON: {}", e))?;

    // 解析 meta 字段（新版 CC Switch 存放 apiFormat 等）
    let meta = meta_json
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned());

    // 从 meta.apiFormat 提取格式提示（新版本最准确的格式来源）
    let meta_api_format = meta
        .as_ref()
        .and_then(|m| m.get("apiFormat"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    // 提取环境变量 - 检查 env / auth / options 三个常见位置
    let env = config.get("env").and_then(|v| v.as_object());
    let auth = config.get("auth").and_then(|v| v.as_object());
    let options = config.get("options").and_then(|v| v.as_object());

    // 提取端点 - 优先级：
    //   1) provider_endpoints.url（关联表）
    //   2) options.baseURL / options.url（opencode 格式）
    //   3) env/auth 中搜索包含 url/base/endpoint 的字段
    //   4) TOML config 中的 base_url / url（codex 格式）
    let endpoint = endpoint_url
        .filter(|url| !url.is_empty())
        .or_else(|| {
            // opencode 格式：options.baseURL / options.url
            options.and_then(|o| {
                o.get("baseURL")
                    .or_else(|| o.get("base_url"))
                    .or_else(|| o.get("url"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty() && s.starts_with("http"))
                    .map(|s| s.to_string())
            })
        })
        .or_else(|| env.and_then(|e| find_url_in_map(e)))
        .or_else(|| auth.and_then(|a| find_url_in_map(a)))
        .or_else(|| {
            config.get("config").and_then(|v| v.as_str()).and_then(|toml_str| {
                extract_url_from_toml(toml_str)
            })
        })
        .unwrap_or_else(|| {
            // 如果没有找到 endpoint，根据 app_type 返回默认值
            match app_type.as_deref() {
                Some("claude") | Some("claude-desktop") => "https://api.anthropic.com".to_string(),
                Some("codex") => "https://api.openai.com".to_string(),
                _ => "".to_string(),
            }
        });

    // 提取 API Key - 支持多种字段名和位置
    let api_key = options
        .and_then(|o| find_api_key_in_map(o))   // opencode 格式
        .or_else(|| env.and_then(|e| find_api_key_in_map(e)))
        .or_else(|| auth.and_then(|a| find_api_key_in_map(a)))
        .unwrap_or_default();

    // 提取模型 - 多种来源
    let model = env
        .and_then(|e| e.get("ANTHROPIC_MODEL"))
        .or_else(|| env.and_then(|e| e.get("OPENAI_MODEL")))
        .or_else(|| env.and_then(|e| e.get("GEMINI_MODEL")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // 从 TOML config 中提取 model（codex 格式）
            config.get("config").and_then(|v| v.as_str()).and_then(|toml_str| {
                extract_model_from_toml(toml_str)
            })
        })
        .or_else(|| {
            // opencode 格式：models 表的第一个 key 作为默认模型
            config.get("models").and_then(|v| v.as_object()).and_then(|m| {
                m.keys().next().map(|k| k.to_string())
            })
        });

    // 读取 TOML config（用于格式和分组判断）
    let toml_config = config.get("config").and_then(|v| v.as_str());

    // 判断是否为 Codex 类型的配置（TOML 中有 model_providers 表）
    let is_codex_config = toml_config
        .map(|s| s.contains("[model_providers") || s.contains("wire_api"))
        .unwrap_or(false);

    // 从 TOML 的 wire_api 字段推断 API 格式
    let toml_wire_api = toml_config.and_then(|s| {
        for line in s.lines() {
            let line = line.trim();
            if line.starts_with("wire_api") && line.contains('=') {
                if let Some(v) = line.split('=').nth(1) {
                    return Some(v.trim().trim_matches('"').trim_matches('\'').to_lowercase());
                }
            }
        }
        None
    });

    // 检测 API 格式 - 优先级：meta.apiFormat > app_type > TOML wire_api > opencode 格式识别 > env 字段推断 > 默认 Anthropic
    let api_format = if let Some(fmt) = meta_api_format.as_deref() {
        match fmt {
            "anthropic" | "claude" => ApiFormat::Anthropic,
            "chat" | "openai" | "openai_chat" | "openaichat" => ApiFormat::OpenaiChat,
            "responses" | "openai_responses" | "openairesponses" | "gemini" | "google" => {
                ApiFormat::OpenaiResponses
            }
            _ => ApiFormat::Anthropic,
        }
    } else if let Some(app) = &app_type {
        match app.to_lowercase().as_str() {
            "claude" | "anthropic" | "claude-desktop" => ApiFormat::Anthropic,
            "openai" | "gpt" | "opencode" => ApiFormat::OpenaiChat,
            "codex" => ApiFormat::OpenaiResponses,
            "gemini" | "google" => ApiFormat::OpenaiResponses,
            _ => {
                // 未知 app_type，看是否有 opencode 标志
                if options.is_some() || config.get("npm").and_then(|v| v.as_str()).is_some() {
                    ApiFormat::OpenaiChat
                } else {
                    ApiFormat::Anthropic
                }
            }
        }
    } else if let Some(wire) = toml_wire_api.as_deref() {
        // 从 TOML wire_api 推断
        match wire {
            "responses" => ApiFormat::OpenaiResponses,
            "chat" => ApiFormat::OpenaiChat,
            _ => ApiFormat::OpenaiChat,
        }
    } else if options.is_some() || config.get("npm").and_then(|v| v.as_str()).is_some() {
        // opencode 格式默认为 OpenAI Chat
        ApiFormat::OpenaiChat
    } else if env.and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN")).is_some()
        || env.and_then(|e| e.get("ANTHROPIC_API_KEY")).is_some()
        || env.and_then(|e| e.get("ANTHROPIC_BASE_URL")).is_some()
        || auth.and_then(|a| a.get("ANTHROPIC_AUTH_TOKEN")).is_some()
        || auth.and_then(|a| a.get("ANTHROPIC_API_KEY")).is_some()
    {
        ApiFormat::Anthropic
    } else if env.and_then(|e| e.get("OPENAI_API_KEY")).is_some()
        || env.and_then(|e| e.get("OPENAI_BASE_URL")).is_some()
        || auth.and_then(|a| a.get("OPENAI_API_KEY")).is_some()
    {
        ApiFormat::OpenaiChat
    } else if env.and_then(|e| e.get("GEMINI_API_KEY")).is_some()
        || env.and_then(|e| e.get("GOOGLE_GEMINI_BASE_URL")).is_some()
        || auth.and_then(|a| a.get("GEMINI_API_KEY")).is_some()
    {
        ApiFormat::OpenaiResponses
    } else {
        ApiFormat::Anthropic
    };

    // 确定分组 - 优先使用 app_type，Codex 配置归为 codex 组，否则按 api_format
    let group = if let Some(app) = &app_type {
        app.to_lowercase()
    } else if is_codex_config {
        "codex".to_string()
    } else {
        match api_format {
            ApiFormat::Anthropic => "claude".to_string(),
            ApiFormat::OpenaiChat => "openai".to_string(),
            ApiFormat::OpenaiResponses => "gemini".to_string(),
        }
    };

    if endpoint.is_empty() || api_key.is_empty() {
        let reason = if endpoint.is_empty() && api_key.is_empty() {
            "Missing both endpoint and API key".to_string()
        } else if endpoint.is_empty() {
            "Missing endpoint".to_string()
        } else {
            "Missing API key".to_string()
        };
        return Err(reason);
    }

    Ok(Vendor {
        id: id.to_string(),
        name: name.to_string(),
        endpoint,
        api_key,
        api_format,
        model,
        group,
    })
}
