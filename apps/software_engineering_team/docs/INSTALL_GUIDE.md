# AgentOS "中心管控 + 边缘执行" 联邦架构 — 安装与测试指导

## 目录结构概览

```
software_engineering_golang_team/
├── center/                          # 中心管控层 (Go)
│   ├── cmd/
│   │   ├── server/main.go           # API 服务器入口 (端口 8080)
│   │   └── worker/main.go           # Temporal Worker 入口
│   ├── internal/                    # 业务逻辑
│   │   ├── api/                     # REST API (29 条路由)
│   │   ├── workflow/                # Temporal 工作流 (SDLC/HumanReview/Callback)
│   │   ├── agent/                   # 边缘节点管理
│   │   ├── graph/                   # 全局图谱管理
│   │   ├── store/                   # SQLite MetaStore
│   │   ├── executor/                # 阶段执行器
│   │   ├── grpc/                    # Agent OS 内核通信
│   │   └── config/                  # 配置管理
│   ├── web/                         # 管理后台 (React 19 + Vite 8)
│   ├── config.yaml                  # 中心配置
│   └── .env.example                 # 环境变量模板
│
├── edge/                            # 边缘执行层
│   ├── daemon/                      # Rust Daemon
│   │   ├── src/                     # Agent Core/沙箱/图谱/同步
│   │   ├── config.yaml              # Daemon 配置
│   │   └── .env.example             # 环境变量模板
│   └── vscode/                      # VS Code 插件
│       ├── src/                     # 6 个 TypeScript 文件
│       ├── webview/                 # WebView 渲染 HTML
│       └── package.json
│
├── tests/                           # 集成测试
│   ├── e2e_center.py                # 中心侧 E2E (26 章)
│   └── e2e_daemon.py                # 边缘侧 E2E (6 章)
│
└── .trae/specs/refactor-center-edge/ # 设计文档
    ├── spec.md                      # 架构规范
    ├── tasks.md                     # 任务清单
    └── checklist.md                 # 验收标准
```

---

## 一、环境准备

### 1.1 前置依赖

| 组件 | 版本要求 | 用途 |
|------|----------|------|
| Go | >= 1.22 | 运行中心管控服务 |
| Rust | >= 1.75 | 编译边缘 Daemon |
| Node.js | >= 20 | 构建 Web 管理后台和 VS Code 插件 |
| Temporal Server | >= 1.21 | 工作流引擎 |
| SQLite | 系统自带 | 元数据存储 |
| Docker | >= 24 | 沙箱容器 (可选) |
| Python | >= 3.10 | 运行集成测试脚本 |

### 1.2 克隆与目录定位

```bash
cd /dev-data/ai-test/wild_agent_os/apps/software_engineering_golang_team
```

### 1.3 端口规划

| 端口 | 服务 | 说明 |
|------|------|------|
| 8080 | Center HTTP API | Go API 服务器 |
| 8081 | Center Web 管理后台 | Vite dev server |
| 7890 | Edge Daemon HTTP | Rust Axum Daemon |
| 7233 | Temporal Server | gRPC 工作流引擎 |
| 6333 | Qdrant (可选) | 向量数据库 |

---

## 二、中心管控层部署

### 2.1 启动 Temporal Server

```bash
# 方式一: Docker 启动 (推荐)
docker run -d --name temporal \
  -p 7233:7233 -p 8233:8233 \
  temporalio/auto-setup:1.21

# 方式二: 使用现有 Temporal 服务
# 修改 center/config.yaml 中的 temporal.host 和 temporal.port
```

### 2.2 配置中心服务

编辑 `center/config.yaml`:

```yaml
server:
  port: 8080
  host: "0.0.0.0"

temporal:
  host: "localhost"
  port: 7233
  task_queue: "sdlc-task-queue"

grpc:
  host: "localhost"
  port: 50051

meta_store:
  driver: "sqlite3"
  dsn: "./data/center.db"

llm:
  provider: "openai"
  api_key: ""              # 在管理后台设置
  model: "gpt-4o"
  base_url: "https://api.openai.com/v1"
```

复制环境变量模板:

```bash
cp center/.env.example center/.env
# 编辑 .env 填入实际值
```

`.env` 必须配置:

```bash
ONE_API_URL=http://localhost:3000   # LLM 网关地址 (如使用 One API)
ONE_API_KEY=your-one-api-key        # One API 密钥
```

### 2.3 启动中心 API 服务器

```bash
cd center
go build -o bin/server ./cmd/server/
./bin/server
```

预期输出:

```
[INFO] Starting server on :8080
[INFO] Connected to MetaStore: ./data/center.db
[INFO] gRPC client connecting to localhost:50051...
[INFO] Temporal client connected to localhost:7233
[INFO] All 29 routes registered
```

验证:

```bash
curl http://localhost:8080/health
# 预期: {"status":"ok","version":"0.1.0"}
```

### 2.4 启动 Temporal Worker

```bash
cd center
go build -o bin/worker ./cmd/worker/
./bin/worker
```

预期输出:

```
[INFO] Worker starting...
[INFO] Registered workflows: SDLCDSLWorkflow
[INFO] Registered activities: ExecuteStage, AIReview, ValidateContract, DispatchTask
[INFO] Worker connected to localhost:7233
[INFO] BuildID: center-worker-v1
```

Worker 必须与 Server **同时运行** — Worker 负责执行工作流 Activity，Server 提供 API 入口。

---

## 三、Web 管理后台部署

### 3.1 安装依赖

```bash
cd center/web
npm install
```

### 3.2 启动开发服务器

```bash
npm run dev
```

预期输出:

```
VITE v8.0.12  ready in 320ms
  ➜  Local:   http://localhost:8081/
  ➜  Proxy:   /api → http://localhost:8080
               /ws → ws://localhost:8080
```

Vite 开发服务器自动代理 `/api` → `localhost:8080`，无需额外配置。

### 3.3 生产构建

```bash
npm run build
# 产物在 dist/ 目录
# 可用 npx serve dist 部署
```

### 3.4 管理后台功能

| 路由 | 功能 | 依赖运行 |
|------|------|----------|
| `/` | Dashboard — 项目统计 + 系统状态 | center server |
| `/projects` | 项目列表 — 创建/搜索/删除 | center server |
| `/projects/:id` | 项目详情 — 阶段 Timeline | center server |
| `/projects/:id/editor` | Pipeline 编辑器 | center server |
| `/pipeline-config` | 管线配置管理 | center server |
| `/chat` | AI 对话 — 流式消息 | center server |
| `/reviews` | 人工审查队列 | center server |
| `/graph` | 知识图谱可视化 | center server |
| `/settings` | LLM 配置管理 | center server |
| `/monitor` | 系统监控 | center server + worker |
| `/logs` | 系统日志 | center server |

---

## 四、边缘 Daemon 部署

### 4.1 配置 Daemon

编辑 `edge/daemon/config.yaml`:

```yaml
server:
  host: "127.0.0.1"
  port: 7890

center:
  url: "http://localhost:8080"
  auth_token: ""                # 注册后自动获取

llm:
  provider: "openai"
  api_key: "sk-your-key-here"  # 本地 LLM API Key
  model: "gpt-4o"
  base_url: "https://api.openai.com/v1"

sandbox:
  docker_socket: "/var/run/docker.sock"
  default_image: "python:3.12-slim"

graph:
  db_path: "./data/graph"
```

### 4.2 编译并启动 Daemon

```bash
cd edge/daemon
cargo build --release
./target/release/agentos-daemon daemon start
```

预期输出:

```
[INFO] AgentOS Daemon starting...
[INFO] Config loaded from ./config.yaml
[INFO] HTTP server listening on 127.0.0.1:7890
[INFO] Registered routes: GET /api/health, POST /api/chat, WS /ws/events
[INFO] Connecting to center at http://localhost:8080...
```

验证:

```bash
curl http://localhost:7890/api/health
# 预期: {"status":"ok","version":"0.1.0"}
```

### 4.3 Daemon CLI 命令

```bash
# 启动 Daemon 服务
./target/release/agentos-daemon daemon start

# 注册到中心 (向 center 注册此边缘节点)
./target/release/agentos-daemon register \
  --center-url http://localhost:8080 \
  --capabilities "code,review,test"

# 查看帮助
./target/release/agentos-daemon --help
```

---

## 五、VS Code 插件安装

### 5.1 前置条件

- VS Code >= 1.85.0
- Edge Daemon 已启动 (localhost:7890)
- (可选) Center Server 已启动 (localhost:8080)

### 5.2 安装插件

**方式一: 从源码安装 (推荐)**

```bash
cd edge/vscode

# 1. 安装依赖 (如果用到 npm)
npm install

# 2. 打包 vsix
npx @vscode/vsce package
# 生成 agentos-daemon-0.1.0.vsix

# 3. 在 VS Code 中安装
code --install-extension agentos-daemon-0.1.0.vsix
```

**方式二: 直接复制 (开发模式)**

```bash
cp -r edge/vscode ~/.vscode/extensions/agentos-daemon-0.1.0
```

**方式三: F5 调试模式**

在 `edge/vscode/` 目录:

```bash
code .
# VS Code 中按 F5 → 选择 "Extension Development Host"
```

### 5.3 插件配置

在 VS Code 设置中搜索 `agentos`:

| 设置项 | 默认值 | 说明 |
|--------|--------|------|
| `agentos.daemonUrl` | `http://localhost:7890` | Edge Daemon 地址 |
| `agentos.autoConnect` | `true` | 启动时自动连接 |
| `agentos.heartbeatInterval` | `30` | 心跳间隔 (秒) |

或编辑 `.vscode/settings.json`:

```json
{
  "agentos.daemonUrl": "http://localhost:7890",
  "agentos.autoConnect": true,
  "agentos.heartbeatInterval": 30
}
```

### 5.4 注册的命令

| 命令 | 快捷键 | 说明 |
|------|--------|------|
| `AgentOS: Connect to Daemon` | — | 手动连接 Daemon |
| `AgentOS: Disconnect` | — | 断开连接 |
| `AgentOS: Refresh Available Tasks` | — | 刷新任务列表 |
| `AgentOS: Open Chat Panel` | `Ctrl+Shift+A` | 打开聊天面板 |
| `AgentOS: Open Graph View` | `Ctrl+Shift+G` | 打开图谱视图 |

### 5.5 界面功能

| 位置 | 功能 |
|------|------|
| **活动栏** (左侧) | AgentOS 图标，点击显示侧边栏 |
| **侧边栏 - Available Tasks** | 显示可申领的任务列表 |
| **侧边栏 - Daemon Status** | 显示连接状态、节点信息 |
| **状态栏** (底部) | 绿色 "AgentOS: Connected" / 红色 "Disconnected" |
| **聊天面板** (`Ctrl+Shift+A`) | AI 对话、代码渲染、Mermaid 图、Diff 视图 |
| **图谱面板** (`Ctrl+Shift+G`) | IRI 知识图谱可视化 |

---

## 六、端到端测试流程

### 6.1 测试前检查清单

在运行测试前，确认以下服务已启动:

```bash
# 1. 验证中心 API
curl http://localhost:8080/health

# 2. 验证 Daemon (可选，部分测试需要)
curl http://localhost:7890/api/health

# 3. 验证 Temporal Server
curl http://localhost:8233/  # Temporal Web UI

# 4. 验证必要端口
ss -tlnp | grep -E "8080|7890|7233"
```

### 6.2 运行中心侧 E2E 测试

```bash
cd tests
python3 e2e_center.py
```

测试覆盖 26 个章节:

| 章节 | 测试内容 | 依赖 |
|------|----------|------|
| 1 | Health Check | center server |
| 2 | Project CRUD | center server |
| 3 | Task CRUD | center server |
| 4 | Stage List (N+1 验证) | center server |
| 5 | 完整 Pipeline 启动 | center + worker |
| 6 | Pipeline 状态查询 | center server |
| 7 | 图谱快照 | center + worker |
| 8 | LLM 配置 | center server |
| 9-19 | 重复场景覆盖 | center server |
| 20 | **边缘离线场景** | center server |
| 21 | **空状态测试** | center server |
| 22 | **错误处理** | center server |
| 23 | **Review 工作流** | center server |
| 24 | **LLM 配置持久化** | center server |
| 25 | **分页测试** | center server |
| 26 | **Agent 注册边界** | center server |

预期结果:

```
============================================================
           AgentOS Center E2E Test Report
============================================================
Total:  26  |  Passed:  26  |  Failed:  0
============================================================
```

### 6.3 运行边缘侧 E2E 测试

```bash
cd tests
python3 e2e_daemon.py
```

测试覆盖 6 个章节:

| 章节 | 测试内容 | 依赖 |
|------|----------|------|
| 1 | Daemon 健康检查 | daemon (7890) |
| 2 | Chat API | daemon + LLM |
| 3 | 注册/心跳流程 | daemon + center |
| 4 | 可用任务查询 | daemon + center |
| 5 | 图谱同步 | daemon + center |
| 6 | WebSocket 连接 | daemon |

> Daemon 不可达时，仅第 1 章失败，后续章节自动跳过；
> Center 不可达时，相关章节自动跳过并打印 `SKIP`。

### 6.4 Go 单元测试

```bash
cd center
CCACHE_DIR=/tmp/ccache CGO_ENABLED=1 go test ./...
```

| 包 | 测试数 | 说明 |
|----|--------|------|
| `internal/agent` | 11 | AgentManager 注册/心跳/超时/匹配 |
| `internal/api` | 30 | 29 条路由 + Gin mode |
| `internal/config` | 3 | 加载/保存/持久化 |
| `internal/executor` | 47 | 7 个阶段执行器 |
| `internal/graph` | 7 | 图谱同步/版本链 |
| `internal/grpc` | 6 | 客户端连接/超时 |
| `internal/store` | 17 | SQLite CRUD/N+1/分页 |
| `internal/workflow` | 11 | 工作流/断点续传/审查/回调 |
| `internal/workflow/pipeline` | 7 | DSL 校验 |

---

## 七、完整端到端验收流程

以下是从零开始验证整个系统的完整步骤:

### 7.1 启动所有服务 (按顺序)

```bash
# 终端 1: Temporal Server
docker run -d --name temporal -p 7233:7233 -p 8233:8233 temporalio/auto-setup:1.21

# 终端 2: Center Worker
cd center && ./bin/worker

# 终端 3: Center API Server
cd center && ./bin/server

# 终端 4: Web 管理后台
cd center/web && npm run dev

# 终端 5: Edge Daemon
cd edge/daemon && ./target/release/agentos-daemon daemon start
```

### 7.2 验证基础功能

```bash
# 1. 健康检查
curl http://localhost:8080/health
curl http://localhost:7890/api/health

# 2. 通过 API 创建一个项目
curl -X POST http://localhost:8080/api/v1/projects \
  -H "Content-Type: application/json" \
  -d '{"project_name": "test-project", "description": "E2E test"}'

# 3. 通过 API 启动一个 Pipeline
curl -X POST http://localhost:8080/api/v1/projects/{project_id}/pipeline \
  -H "Content-Type: application/json" \
  -d '{"dsl_name": "standard-sdlc"}'
```

### 7.3 验证 Web 管理后台

浏览器打开 http://localhost:8081/

1. **Dashboard**: 确认统计卡片显示正常
2. **Projects**: 确认刚才创建的项目显示在列表中
3. **Chat**: 发送一条消息，确认流式回复正常
4. **Settings**: 配置 LLM API Key，保存，刷新确认持久化

### 7.4 验证 VS Code 插件

1. 确认状态栏显示綠色 "AgentOS: Connected"
2. 按 `Ctrl+Shift+A` 打开聊天面板
3. 输入消息，确认流式回复正确渲染 (代码块/Markdown/Mermaid/Diff)
4. 点击 "Refresh Tasks"，确认 Available Tasks 面板显示任务列表
5. 右键任务 → "Claim Task"，确认认领成功

### 7.5 运行全量测试

```bash
# 1. Go 单元测试
cd center && CCACHE_DIR=/tmp/ccache CGO_ENABLED=1 go test ./... -v

# 2. 中心 E2E
cd tests && python3 e2e_center.py

# 3. 边缘 E2E
cd tests && python3 e2e_daemon.py
```

---

## 八、常见问题排查

### 8.1 连接问题

| 症状 | 原因 | 解决 |
|------|------|------|
| `Connection refused` on :8080 | Center server 未启动 | `cd center && ./bin/server` |
| `Connection refused` on :7890 | Daemon 未启动 | `cd edge/daemon && cargo run -- daemon start` |
| `Connection refused` on :7233 | Temporal Server 未启动 | `docker start temporal` |
| WS 连接失败 | 跨域/端口限制 | 确认 `vite.config.ts` proxy 配置正确 |

### 8.2 构建问题

| 症状 | 原因 | 解决 |
|------|------|------|
| `go build` 失败: cgo | 缺少 gcc/ccache | 设置 `CGO_ENABLED=0` 或安装 gcc |
| `cargo check` 失败 | Rust 版本过低 | `rustup update stable` |
| npm install 失败 | Node 版本过低 | `nvm use 20` |
| VS Code 插件不显示 | 安装路径错误 | 使用 `vsce package` 打包后安装 |

### 8.3 工作流问题

| 症状 | 原因 | 解决 |
|------|------|------|
| Pipeline 卡住 (Pending) | Worker 未运行 | 启动 `./bin/worker` |
| Human Review 不触发 | Signal 名称不匹配 | 确认 `callback-{taskID}-{stageID}` 格式 |
| AI Review 失败 | gRPC 连接断开 | 确认 Agent OS 内核运行在 :50051 |
| 断点续传不生效 | WorkflowState 未更新 | 检查 `ContinueAsNew` 是否携带完整 State |

### 8.4 测试问题

| 症状 | 原因 | 解决 |
|------|------|------|
| `e2e_center.py` 全 FAIL | Center server 未运行 | 先启动 `./bin/server` |
| `e2e_daemon.py` 全 SKIP | Daemon 未运行 | 先启动 `cargo run -- daemon start` |
| Go test 中 workflow 测试卡住 | Temporal Server 未运行 | `docker start temporal` |
| SQLite 测试失败 | CGO 未启用 | `CGO_ENABLED=1` |

---

## 九、快速启动脚本

创建 `start-all.sh` 一键启动所有服务:

```bash
#!/bin/bash
# AgentOS 一键启动脚本

echo "=== Starting AgentOS ==="

# 1. Temporal
echo "[1/5] Starting Temporal Server..."
docker start temporal 2>/dev/null || docker run -d --name temporal \
  -p 7233:7233 -p 8233:8233 temporalio/auto-setup:1.21

# 2. Center Worker
echo "[2/5] Starting Center Worker..."
cd center && go build -o bin/worker ./cmd/worker/
./bin/worker &
WORKER_PID=$!

# 3. Center Server
echo "[3/5] Starting Center Server..."
go build -o bin/server ./cmd/server/
./bin/server &
SERVER_PID=$!
cd ..

# 4. Web Admin
echo "[4/5] Starting Web Admin..."
cd center/web && npm run dev &
WEB_PID=$!
cd ../..

# 5. Edge Daemon
echo "[5/5] Starting Edge Daemon..."
cd edge/daemon && cargo run -- daemon start &
DAEMON_PID=$!

echo ""
echo "=== All services started ==="
echo "Center API:  http://localhost:8080"
echo "Web Admin:   http://localhost:8081"
echo "Daemon:      http://localhost:7890"
echo "Temporal UI: http://localhost:8233"
echo ""
echo "Press Ctrl+C to stop all services"

trap "kill $WORKER_PID $SERVER_PID $WEB_PID $DAEMON_PID 2>/dev/null" EXIT
wait
```

---

## 十、附录: API 端点参考

### 中心管控 API (localhost:8080)

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查 |
| GET | `/api/v1/dashboard/stats` | Dashboard 统计 |
| GET | `/api/v1/dashboard/activity` | 最近活动 |
| GET | `/api/v1/projects` | 项目列表 |
| POST | `/api/v1/projects` | 创建项目 |
| GET | `/api/v1/projects/:id` | 项目详情 |
| PUT | `/api/v1/projects/:id` | 更新项目 |
| DELETE | `/api/v1/projects/:id` | 删除项目 |
| GET | `/api/v1/projects/:id/tasks` | 任务列表 |
| POST | `/api/v1/projects/:id/tasks` | 创建任务 |
| GET | `/api/v1/tasks/:id/stages` | 阶段列表 |
| POST | `/api/v1/projects/:id/pipeline` | 启动 Pipeline |
| GET | `/api/v1/pipeline/:id/result` | Pipeline 结果 |
| GET | `/api/v1/projects/:id/graph` | 项目图谱 |
| GET | `/api/v1/projects/:id/snapshot` | 项目快照 |
| GET | `/api/v1/reviews/pending` | 待审查列表 |
| POST | `/api/v1/reviews/submit` | 提交审查 |
| GET | `/api/v1/config/llm` | LLM 配置 |
| PUT | `/api/v1/config/llm` | 更新 LLM 配置 |
| POST | `/api/v1/agents/register` | 边缘节点注册 |
| POST | `/api/v1/agents/heartbeat` | 节点心跳 |
| GET | `/api/v1/tasks/available` | 可用任务 |
| POST | `/api/v1/tasks/:id/claim` | 认领任务 |
| POST | `/api/v1/tasks/:id/callback` | 阶段完成回调 |
| POST | `/api/v1/graph/sync` | 图谱同步 |
| GET | `/api/v1/graph/context` | 图谱上下文 |
| WS | `/ws/events` | WebSocket 事件 |

### 边缘 Daemon API (localhost:7890)

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | 健康检查 |
| POST | `/api/chat` | AI 对话 |
| WS | `/ws/events` | WebSocket 事件流 |