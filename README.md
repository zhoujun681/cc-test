# CC Switch API Tester

cc-switch加多了中转，可用性需要一个一个测试很不方便。于是用AI写了这个简单工具，暂时只支持claude code、codex和opencode的设置，其余的需要实现和其他功能各位自己拉取让AI跑哈。
下面是AI帮写的^-^。

一个用于批量测试 CC Switch 中配置的各类 AI API 供应商（Claude / Codex / OpenAI / OpenCode 等）延迟与可用性的工具。支持终端命令行测试和 Web 可视化界面，自动读取 CC Switch 数据库，按供应商分组，统计 TTFB（首字节时间）和总耗时，实时展示测试进度与每次请求详情。

## 功能特性

- **自动识别 CC Switch 数据库**：默认读取 `~/.cc-switch/cc-switch.db`，读不到时回退到当前目录
- **多格式配置解析**：支持 Claude（`env`）、Codex（TOML `base_url`）、OpenCode（`options.baseURL`）等多种配置格式
- **多 API 协议**：Anthropic Messages、OpenAI Chat Completions、OpenAI Responses
- **Web 可视化界面**
  - 按分组（claude / codex / opencode 等）分类显示，支持分组筛选和按组批量测试
  - 表格固定首列（名称）和右侧两列（状态、操作），中间列横向滚动
  - 实时进度条 + 每完成一个供应商立即更新
  - 并发数可配置（默认 10）
  - 详情对话框：每次请求的状态码、TTFB、总耗时、错误信息
  - 中英文对照界面
- **终端测试模式**：带实时进度的并发批量测试，支持表格/JSON 输出
- **网页手动选择数据库**：支持路径加载和文件上传两种方式

## 快速开始

### 环境要求

- Rust 1.75+（推荐通过 [rustup](https://rustup.rs/) 安装）
- 已安装 CC Switch 并配置了至少一个 API 供应商

### 编译运行

```bash
git clone <repo-url>
cd cc-test2
cargo run
```

启动后会自动打开浏览器访问 `http://localhost:38080`。

### 三种运行模式

```bash
# 1. 默认模式：启动 Web 界面（推荐）
cargo run

# 2. 启动 Web 前，先在终端跑一次批量测试
cargo run -- --run

# 3. 纯终端模式：测试完即退出，不启动 Web
cargo run -- --no-web --output result.json
```

## 命令行参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--db-path <PATH>` | 指定 CC Switch 数据库路径 | 自动检测 |
| `--repeat <N>` | 每个 API 重复测试次数 | 3 |
| `--concurrency <N>` | 最大并发数 | 无限制（终端）/ 10（Web） |
| `--timeout <SEC>` | 单次请求超时秒数 | 30 |
| `--port <PORT>` | Web 服务器端口 | 38080 |
| `--vendor <NAME>` | 只测试指定供应商（可多次指定） | 全部 |
| `--model <NAME>` | 覆盖测试用的模型名 | 从配置读取 |
| `--output <FILE>` | 导出结果到 JSON 文件 | 无 |
| `--no-stream` | 禁用流式请求（无法测量 TTFB） | 流式开启 |
| `--run` | 启动 Web 前先跑终端批量测试 | 关闭 |
| `--no-web` | 纯终端模式 | 关闭 |

### 示例

```bash
# 测试全部供应商，每个 5 次，导出 JSON
cargo run -- --no-web --repeat 5 --output report.json

# 只测 Claude 分组，端口改为 8080
cargo run -- --port 8080

# 使用其他电脑的数据库文件
cargo run -- --db-path /path/to/cc-switch.db
```

## Web 界面说明

### 工具栏
- **重载(Reload)**：重新读取数据库
- **选择数据库(Select DB)**：手动指定 db 文件（路径加载或文件上传）
- **并发(Concurrency)**：调整并发数（1-100，默认 10）
- **测试全部(Test All)**：测试当前分组筛选下的所有供应商
- **测试选中(Test Selected)**：只测试勾选的供应商
- **自定义测试(Custom)**：手工输入端点/Key/模型进行测试

### 表格列
| 列 | 说明 |
|----|------|
| 名称(NAME) | 供应商名称（固定左侧） |
| 分组(GROUP) | claude / codex / opencode 等 |
| 端点(ENDPOINT) | API 地址 |
| 模型(MODEL) | 测试用的模型 |
| 格式(FORMAT) | API 协议格式 |
| 首字节时间/最小/最大(TTFB) | 首字节延迟统计 |
| 总耗时/最小/最大(Total) | 完整请求耗时统计 |
| 状态(STATUS) | 成功/失败/部分成功（固定右侧） |
| 操作(ACTION) | 测试(Test)、详情(Detail)按钮（固定右侧） |

### 延迟颜色标识
- 🟢 绿色：< 2000ms
- 🟡 黄色：2000 - 4000ms
- 🔴 红色：> 4000ms

### 详情对话框
点击任意供应商的"详情(Detail)"按钮，可查看：
- 基本信息：端点、模型、格式、分组、总请求数、成功/失败数、成功率
- 延迟指标：TTFB 和总耗时的 avg/min/max
- 每次请求详情：状态、HTTP 状态码、TTFB、总耗时、错误信息

## 数据库识别优先级

程序按以下顺序自动查找 `cc-switch.db`：

1. `~/.cc-switch/cc-switch.db`（CC Switch 默认位置）
2. `%APPDATA%/cc-switch/`、`%LOCALAPPDATA%/cc-switch/`
3. **当前工作目录**下的 `cc-switch.db`
4. 可执行文件所在目录
5. 递归搜索上述目录的 cc-switch 子目录

也可在 Web 界面通过"选择数据库"手动加载其他数据库文件。

## 支持的配置格式

### Claude 格式（`env` 字段）
```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-xxx",
    "ANTHROPIC_MODEL": "claude-sonnet-4-20250514"
  }
}
```

### Codex 格式（TOML `config`）
```toml
[model_providers.custom]
name = "codex站点"
base_url = "https://codex.example.com"
wire_api = "responses"
```

### OpenCode 格式（`options` 字段）
```json
{
  "models": { "deepseek-chat": { "name": "DeepSeek V3.2" } },
  "npm": "@ai-sdk/openai-compatible",
  "options": {
    "apiKey": "sk-xxx",
    "baseURL": "https://api.deepseek.com/v1"
  }
}
```

## 项目结构

```
cc-test2/
├── src/
│   ├── main.rs        # 入口、CLI 参数、运行模式
│   ├── config.rs      # 数据库定位与供应商配置解析
│   ├── tester.rs      # API 请求构建与测试逻辑
│   ├── web.rs         # Web 服务器与 API 路由
│   ├── reporter.rs    # 终端结果输出
│   └── types.rs       # 数据结构与全局状态
├── static/
│   └── index.html     # Web 前端界面
├── Cargo.toml
└── README.md
```

## 技术栈

- **后端**：Rust + Axum（Web）+ Reqwest（HTTP）+ Rusqlite（数据库）
- **前端**：原生 HTML/CSS/JavaScript（嵌入二进制）
- **并发**：Tokio + `futures::stream::for_each_concurrent`

## 许可证

MIT
