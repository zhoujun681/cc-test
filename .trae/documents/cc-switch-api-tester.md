# CC Switch API 测速工具

## 摘要

构建一个轻量级 Rust 程序，自动读取 CC Switch 的 SQLite 数据库中的供应商配置，向各供应商的 API 端点发送最小化 chat 请求，测量首字节响应时间(TTFB)、延迟、请求完成时间，每个端点测试 3 次并支持并发。启动本地 Web 服务器，提供完整的交互式界面，支持：
- 重新读取 CC Switch 配置
- 批量测试所有供应商
- 单个供应商测试
- 手工测试（自定义端点、Key、模型）
- 实时测试进度显示

## 当前状态分析

- 项目目录 `d:\Projects\Rust\cc-test2` 为空，需要从零创建
- CC Switch 使用 SQLite 存储供应商配置（端点 URL、API Key、API 格式等）
- 需要支持的 API 格式：Anthropic Messages、OpenAI Chat Completions、OpenAI Responses API
- 用户要求轻量化，需要 Web 界面展示结果

## 设计方案

### 技术栈

- **语言**: Rust
- **HTTP 客户端**: `reqwest`（支持流式响应，测量 TTFB）
- **数据库**: `rusqlite`（读取 CC Switch SQLite 数据库）
- **序列化**: `serde` + `serde_json`
- **并发**: `tokio` 异步运行时
- **CLI**: `clap`（命令行参数解析）
- **表格输出**: `comfy-table`（终端表格美化）
- **Web 服务器**: `axum`（轻量级 Web 框架）
- **HTML 模板**: 内嵌静态 HTML（使用 `rust-embed` 嵌入二进制）
- **打开浏览器**: `open`（跨平台打开默认浏览器）

### 架构

```
src/
  main.rs          - CLI 入口，参数解析
  config.rs        - 配置读取（CC Switch DB 解析）
  tester.rs        - API 测试核心（发送请求、计时）
  reporter.rs      - 终端结果输出
  web.rs           - Web 服务器（提供可视化界面）
  types.rs         - 数据结构定义
static/
  index.html       - Web 界面页面（单文件，内嵌 CSS + JS）
```

### 核心流程

1. **定位 CC Switch 数据库**
   - Windows: `%APPDATA%/cc-switch/` 或 `%LOCALAPPDATA%/cc-switch/`
   - macOS: `~/Library/Application Support/cc-switch/`
   - Linux: `~/.config/cc-switch/` 或 `~/.local/share/cc-switch/`
   - 数据库文件名可能是 `.db` 或 `.sqlite`，需搜索确认
   - 支持用户通过 `--db-path` 手动指定路径

2. **解析供应商配置**
   - 从 SQLite 中读取供应商表，提取：名称、端点 URL、API Key、API 格式、模型
   - 根据 API 格式决定请求构造方式

3. **构造最小化测试请求**
   - **Anthropic Messages 格式**: `POST {base_url}/v1/messages`
     ```json
     {"model": "claude-haiku-3-5-20241022", "max_tokens": 10, "messages": [{"role": "user", "content": "Hi"}]}
     ```
   - **OpenAI Chat 格式**: `POST {base_url}/v1/chat/completions`
     ```json
     {"model": "gpt-4o-mini", "max_tokens": 10, "messages": [{"role": "user", "content": "Hi"}]}
     ```
   - **OpenAI Responses 格式**: `POST {base_url}/v1/responses`
     ```json
     {"model": "gpt-4o-mini", "input": "Hi"}
     ```
   - 所有请求使用 `stream: true` 以测量 TTFB
   - 使用极小的 `max_tokens` 减少 token 消耗

4. **测量指标**
   - **TTFB (Time to First Byte)**: 从发送请求到收到第一个响应字节的时间
   - **延迟 (Latency)**: 即 TTFB，首字节到达时间
   - **完成时间 (Total Time)**: 从发送请求到完整接收响应的时间
   - 如果请求失败，记录错误信息（HTTP 状态码、错误消息）

5. **测试策略**
   - 每个供应商测试 3 次（可通过 `--repeat` 修改）
   - 3 次之间取平均值和中位数
   - 支持并发测试所有供应商（默认并发数 = 供应商数量，可通过 `--concurrency` 限制）

6. **输出格式**
   - 终端表格：供应商名称、TTFB(avg/min/max)、总时间(avg/min/max)、状态
   - JSON 文件：完整结果（可通过 `--output` 指定输出路径）
   - 颜色标识：TTFB < 500ms 绿色, 500-1000ms 黄色, > 1000ms 红色
   - **Web 界面**：测试完成后启动本地 Web 服务器，自动打开浏览器展示结果

## 具体实现步骤

### Step 1: 初始化 Rust 项目
- `cargo init` 创建项目
- 配置 `Cargo.toml` 添加依赖

### Step 2: 定义数据结构 (`types.rs`)
- `Vendor`: 供应商配置（名称、端点、API Key、API 格式、模型）
- `TestResult`: 单次测试结果（TTFB、总时间、状态、错误信息）
- `VendorReport`: 供应商汇总报告（3 次测试的统计结果）
- `ApiFormat` 枚举：Anthropic / OpenAIChat / OpenAIResponses

### Step 3: 实现数据库读取 (`config.rs`)
- 自动定位 CC Switch 数据库路径
- 使用 `rusqlite` 读取供应商表
- 解析 JSON 字段提取端点、Key、模型等信息
- 支持 `--db-path` 覆盖默认路径

### Step 4: 实现测试核心 (`tester.rs`)
- 使用 `reqwest` 发送流式请求
- 精确计时 TTFB 和总时间
- 处理 3 种 API 格式的请求构造
- 错误处理和重试逻辑
- 使用 `tokio` 实现并发测试

### Step 5: 实现终端结果输出 (`reporter.rs`)
- `comfy-table` 终端表格输出
- JSON 文件输出
- 颜色标识

### Step 6: 实现 Web 服务器 (`web.rs`)
- 使用 `axum` 启动轻量 Web 服务器
- 提供路由：
  - `GET /` - 返回 HTML 页面
  - `GET /api/vendors` - 获取供应商列表
  - `POST /api/test/single` - 测试单个供应商
  - `POST /api/test/batch` - 批量测试所有供应商
  - `POST /api/test/custom` - 手工测试自定义配置
  - `POST /api/config/reload` - 重新读取 CC Switch 配置
  - `GET /api/results` - 获取测试结果
- 使用 `rust-embed` 嵌入静态资源
- 支持 WebSocket 实时推送测试进度

### Step 7: 创建 Web 界面 (`static/index.html`)
- 单文件 HTML（内嵌 CSS + JS），无外部依赖
- 简洁现代的深色主题界面
- **功能区域**：
  - **顶部工具栏**：
    - "重新读取配置" 按钮 - 重新扫描 CC Switch 数据库
    - "批量测试全部" 按钮 - 并发测试所有供应商
    - "手工测试" 按钮 - 打开自定义测试对话框
  - **供应商列表区**：
    - 表格展示所有供应商（名称、端点、模型、API格式）
    - 每行有 "测试" 按钮 - 单个测试
    - 复选框支持多选批量测试
  - **测试结果区**：
    - 测试概览卡片（总供应商数、成功数、失败数、平均 TTFB）
    - 结果表格（名称、端点、TTFB avg/min/max、总时间、状态）
    - 颜色标识（绿色 < 500ms，黄色 500-1000ms，红色 > 1000ms）
    - 点击列头排序
  - **手工测试对话框**：
    - 输入：端点 URL、API Key、模型、API 格式（下拉选择）
    - 测试次数设置
    - 实时显示测试结果
  - **实时进度**：
    - 测试进行时显示进度条
    - 显示当前正在测试的供应商
    - 使用 WebSocket 实时更新

### Step 8: CLI 入口 (`main.rs`)
- `clap` 解析命令行参数
- 串联各模块
- 测试完成后：先输出终端表格，再启动 Web 服务器并打开浏览器

## 命令行接口设计

```
cc-test2 [OPTIONS]

Options:
  --db-path <PATH>        CC Switch 数据库路径（自动检测）
  --repeat <N>            每个 API 测试次数（默认 3）
  --concurrency <N>       最大并发数（默认不限）
  --output <PATH>         结果输出 JSON 文件路径
  --vendor <NAME>         只测试指定供应商（支持多次指定）
  --model <MODEL>         覆盖测试使用的模型名
  --timeout <SECONDS>     请求超时时间（默认 30s）
  --port <PORT>           Web 服务器端口（默认 38080）
  --no-web                不启动 Web 服务器，仅终端输出
  --no-stream             不使用流式请求（无法测 TTFB）
  -h, --help              帮助信息
```

## 依赖清单 (Cargo.toml)

```toml
[dependencies]
reqwest = { version = "0.12", features = ["stream", "json", "rustls-tls"], default-features = false }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
clap = { version = "4", features = ["derive"] }
comfy-table = "7"
futures-util = "0.3"
chrono = { version = "0.4", features = ["serde"] }
dirs = "5"
axum = "0.7"
rust-embed = "8"
open = "5"
tower-http = { version = "0.5", features = ["cors"] }
```

## Web 界面设计

### 页面结构
- **顶部工具栏**：
  - 标题 "CC Switch API Tester"
  - "重新读取配置" 按钮 - 重新扫描 CC Switch 数据库
  - "批量测试全部" 按钮 - 并发测试所有供应商
  - "手工测试" 按钮 - 打开自定义测试对话框
  - 测试进度指示器（测试中显示）

- **供应商列表区**：
  - 表格展示所有供应商（名称、端点、模型、API格式）
  - 每行有 "测试" 按钮 - 单个测试
  - 复选框支持多选批量测试
  - 选中后显示 "测试选中" 按钮

- **测试结果区**：
  - 测试概览卡片（总供应商数、成功数、失败数、平均 TTFB）
  - 结果表格（名称、端点、TTFB avg/min/max、总时间、状态）
  - 颜色标识（绿色 < 500ms，黄色 500-1000ms，红色 > 1000ms）
  - 点击列头排序
  - 错误信息展示

- **手工测试对话框**：
  - 输入：端点 URL、API Key、模型、API 格式（下拉选择）
  - 测试次数设置
  - 实时显示测试结果

- **实时进度**：
  - 测试进行时显示进度条
  - 显示当前正在测试的供应商
  - 使用 WebSocket 实时更新

### 颜色方案
- 深色背景 (#1a1a2e)
- 绿色 (#00d68f): TTFB < 500ms
- 黄色 (#ffaa00): TTFB 500-1000ms
- 红色 (#ff3d71): TTFB > 1000ms 或失败

## Web API 设计

### REST API
- `GET /` - 返回 HTML 页面
- `GET /api/vendors` - 获取供应商列表
- `POST /api/config/reload` - 重新读取 CC Switch 配置
- `POST /api/test/single` - 测试单个供应商（请求体：vendor_id）
- `POST /api/test/batch` - 批量测试（请求体：vendor_ids 数组）
- `POST /api/test/custom` - 手工测试自定义配置（请求体：endpoint, api_key, model, api_format）
- `GET /api/results` - 获取所有测试结果

### WebSocket
- `WS /ws` - 实时推送测试进度
  - 消息格式：`{"type": "progress", "current": 1, "total": 10, "vendor": "PackyCode"}`
  - 消息格式：`{"type": "result", "vendor_id": "xxx", "result": {...}}`
  - 消息格式：`{"type": "complete", "summary": {...}}`

## 假设与决策

1. **数据库位置**: 假设 CC Switch 数据库在标准应用数据目录下，程序会搜索 `.db` / `.sqlite` 文件。如果找不到，用户可通过 `--db-path` 手动指定。
2. **数据库表结构**: 需要在运行时探索确认实际的表名和字段。程序首次运行时打印发现的表结构供调试。
3. **模型选择**: 优先使用供应商配置中的模型；如果未配置，根据 API 格式使用默认最小模型。
4. **流式请求**: 默认使用流式请求以测量 TTFB。`--no-stream` 选项可退化为非流式。
5. **token 消耗**: 使用 `max_tokens: 10` 和最短 prompt "Hi"，单次测试消耗极少 token。
6. **Web 端口**: 默认使用 38080 端口（避免与常用端口冲突），可通过 `--port` 修改。
7. **Web 界面**: 单文件 HTML，无外部 CDN 依赖，所有样式和脚本内嵌。

## 验证步骤

1. `cargo build` 确保编译通过
2. `cargo run -- --help` 验证 CLI 参数解析
3. `cargo run` 自动检测数据库并执行测试
4. 验证终端表格输出格式正确
5. 验证 JSON 输出文件内容完整
6. 验证 Web 服务器启动并自动打开浏览器
7. 验证 Web 界面正确展示结果、排序功能、颜色标识
8. 测试错误场景：无效 API Key、不可达端点
