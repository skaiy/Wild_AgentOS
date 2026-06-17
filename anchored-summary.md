# Gliding Horse — Anchored Summary

> 当前 session 的关键上下文锚点。每次实现新功能后更新。

---

## 项目目标

在 Rust 中为 AI Agent 构建一个「可观测的文件变更通知 + 主动感知」机制，让 Agent 能够：

1. **实时感知**工作区文件状态变化（通过通知 + 轮询双通道）
2. **主动恢复**被外部修改的文件（git restore）
3. **在系统提示中注入感知上下文**，使 Agent 在规划时就能感知到文件状态
4. **冲突检测 & 主动干预**：在 Agent 即将对 stale 文件写入时产生告警

---

## 已完成的任务

### Phase 1: 核心数据结构与文件监控

- [x] `src/tools/workspace_monitor/file_state.rs` — `FileState` 枚举 (ReadFresh, ReadStale, WrittenUnread, ReadWrite, Discovered, Deleted)，以及 `FileInventory`（路径到状态的映射，带 `list_by_state`, `list_by_language`, `list_all`）
- [x] `src/tools/workspace_monitor/mod.rs` — `WorkspaceMonitor` 核心结构体（rwlock），`notify_read`（标记文件为 ReadFresh + 保存 hash + 启动 watcher）、`notify_write`（标记为 WrittenUnread 或 ReadWrite）、`check_stale`（比较 hash 判断是否过期）、`list_stale_files`、`get_status_report`；集成 `notify` crate 的文件系统事件监听（多事件去重与 cooldown）
- [x] 文件系统事件监听去重机制：基于文件名 + 事件类型的 `DelayMap`，300ms cooldown
- [x] `poll_workspace` 外部轮询机制（git status），用于兜底检测通知遗漏的变化

### Phase 2: Hook 上下文系统

- [x] `src/tools/hooks/mod.rs` — `HookPoint` (BeforeRead, AfterRead, BeforeWrite, AfterWrite, BeforeToolUse, AfterToolUse, BeforeThink, AfterThink)、`HookContext`（HashMap 扩展，支持 `.data` / `.metadata` / `.set()` / `.get<T>()` / `to_prompt_segment`）、`HookManager`（注册/运行时上下文注入）
- [x] Hook 上下文 `to_prompt_segment()` — 在 SA 思考前注入 workspace 状态描述，包括 stale file 警告、written-unread 计数

### Phase 3: 系统提示注入

- [x] `src/tools/hooks/mod.rs` — `HookContext.data` 存储 workspace 感知条目
- [x] `src/core/agent_runner/perception_region.rs` — `SystemPromptRegion` 枚举 + `SystemPromptBuilder` 构建器
- [x] `src/core/agent_runner/perception_region.rs` — `PerceptionContextManager` 聚合所有感知区域上下文：workspace 状态、方法论提示、冲突警告，合并为 `build_injected_system_prompt()`（加入分隔标记 `<!-- perception-start -->` / `<!-- perception-end -->`）
- [x] `src/core/agent_runner/perception_region.rs` — `inject_perception_context` 函数入口，供 AgentRunner 调用
- [x] `src/core/agent_runner/mod.rs` — `AgentRunner::exec()` 中集成感知区域注入（`mod perception_region`、`use perception_region::inject_perception_context`、在 think 阶段调用的 `inject_perception_context()`）

### Phase 4: 主动感知 (ProactiveEngine) & 冲突检测

- [x] `src/perception/mod.rs` — 模块声明
- [x] `src/perception/proactive_engine.rs` — `ProactiveEngine` 结构体：带缓存、告警历史、L0 存储引用；周期性地检测 stale 文件、工作区异常、资源冲突
- [x] `src/perception/proactive_engine.rs` — `InterventionPlan`：告警 & 诊断 & 动作列表，区分高中低优先级
- [x] `src/perception/proactive_engine.rs` — `PerceptionConfig`：可配置的轮询/超时/阈值参数
- [x] `src/perception/proactive_engine.rs` — `check_workspace_snapshot`：检测已删除文件、新增未知文件、stale 文件，生成告警
- [x] `src/perception/proactive_engine.rs` — `check_resource_conflicts`：检测 3 种冲突（stale 文件写入、并发写入冲突、未读写入），生成干预计划
- [x] 冲突检测集成：`EventBus::on` 订阅 + `on_resource_conflict` 回调
- [x] `lib.rs` 公开 `perception::*` 模块
- [x] PerceptionStore** 跨组件感知共享存储，支持按 agent_id 隔离的感知条目和全局条目；支持基于优先级的排序输出
- [x] **PerceptionEntry + PerceptionSource** 感知数据结构，含源信息、优先级向量、时间戳

### Phase 5: AgentRunner 集成

- [x] `src/core/agent_runner/execution.rs` — `AgentRunner` 在 `exec()` 入口注入 `PerceptionStore` 引用到 `AgentContext`
- [x] `src/core/agent_runner/execution.rs` — `AgentContext` 新增 `perception_store` 字段
- [x] `src/core/agent_runner/mod.rs` — `mod perception_region` 公开
- [x] `src/core/agent_runner/execution.rs` — 感知上下文注入入口 `inject_perception_context`
- [x] `src/core/agent_runner/mod.rs` — `AgentRunner` 初始化 `PerceptionStore`、构造时传入、`exec()` 使用
- [x] `src/tools/mod.rs` — 重新导出 `workspace_monitor::FileState`
- [x] **WorkspaceMonitor -> PerceptionStore 集成**：在 `notify_read`, `notify_write`, `check_stale` 等方法中注入对应感知条目

### Optimization 2 (已完成): HookContext.data → metadata 迁移

- [x] `src/tools/hooks/mod.rs` — `HookContext` 新增 `metadata: Arc<RwLock<HashMap<String, Value>>>` 字段
- [x] `src/tools/hooks/mod.rs` — 新增 `metadata_mut()`, `inject_perception_to_context()` 方法
- [x] `src/tools/hooks/mod.rs` — `to_prompt_segment` 支持 metadata 序列化
- [x] `src/core/agent_runner/mod.rs` — `AgentRunner::think()` 调用 `inject_perception_to_context()`

### Optimization 3 (已完成): workspace_status MCP tool

- [x] `src/tools/tool_executor/mod.rs` — 注册 `workspace_status` 工具，通过 WorkspaceMonitor 读取 FileInventory，返回按 state/language 分组的统计摘要（stale 文件、written-unread 文件、各语言计数）

### Optimization 4 — Batch Agent 集成 PerceptionStore (已完成)

- [x] `src/engine/batch_agent.rs` — 创建 BatchAgent 时传入 `Arc<PerceptionStore>`
- [x] `src/engine/batch_agent.rs` — 在每个 `dispatch_agent_task` 或 `create_agent_run` 入口处注入感知上下文
- [x] `src/engine/mod.rs` — 创建 BatchAgent 时传入 PerceptionStore

### Phase 6 — ProactiveEngine 集成 PerceptionStore (已完成)

- [x] `src/perception/proactive_engine.rs` — 新增 `perception_store: Option<Arc<PerceptionStore>>` 字段
- [x] `src/perception/proactive_engine.rs` — `with_perception_store()` 构建器方法
- [x] `src/perception/proactive_engine.rs` — `inject_conflict_perception()` 内部方法，将冲突告警写入 PerceptionStore
- [x] `src/perception/proactive_engine.rs` — `on_resource_conflict()` 中调用 `inject_conflict_perception()`

### 分析文档

- [x] `PR-res/文件变更通知Agent机制分析.md` — 全面分析：Hook 模式、轮询 vs 通知、EventBus、WorkspaceMonitor、感知区域、ProactiveEngine 干预
- [x] `PR-res/Agent主动感知区域设计与实现方案.md` — 设计方案：感知区域定义、分层聚合、优先队列、生命周期管理

---

## 关键决策记录

| 决策 | 选择 | 理由 |
|------|------|------|
| 双通道感知 | 事件通知 + 周期轮询 | 通知低延迟 + 轮询做兜底，防止丢失事件 |
| 状态流转 | ReadFresh(刚读) → ReadStale(外部修改) / WrittenUnread(写入) | 明确区分"能安全写"与"可能需要重读" |
| 无需事务日志 | 不增加 write-ahead log | 当前架构不需要跨进程/崩溃恢复，增加复杂度不合理 |
| 感知区域 | 分层聚合，分隔标记注入 | 与现有 prompt 工程兼容，Agent 在思考时可感知完整上下文 |
| Conflict Detection | ProactiveEngine 在干预点检查 | 与 WorkspaceMonitor 状态联动，在写操作前做验证 |
| `.data` → `.metadata` | 独立存储感知数据的序列化字段 | 避免与原始事件数据混淆，便于独立输出为提示片段 |

---

## 关键文件位置

| 文件 | 用途 |
|------|------|
| `src/tools/workspace_monitor/mod.rs` | WorkspaceMonitor 核心实现 |
| `src/tools/workspace_monitor/file_state.rs` | FileState 枚举与 FileInventory |
| `src/tools/hooks/mod.rs` | HookContext, HookManager, HookPoint |
| `src/core/agent_runner/perception_region.rs` | 感知区域构建与系统提示注入 |
| `src/core/agent_runner/execution.rs` | exec() 入口集成感知上下文 |
| `src/core/agent_runner/mod.rs` | AgentRunner 主模块 |
| `src/perception/proactive_engine.rs` | ProactiveEngine 主动感知引擎 |
| `src/perception/collectors.rs` | 感知数据收集器 |
| `src/core/perception_store.rs` | PerceptionStore 跨组件共享存储 |
| `src/core/mod.rs` | 核心模块声明（公开 PerceptionStore） |
| `PR-res/文件变更通知Agent机制分析.md` | 机制分析文档 |
| `PR-res/Agent主动感知区域设计与实现方案.md` | 设计方案文档 |

---

## 下一步

等待用户指导。潜在方向：

1. **测试覆盖** — 为 PerceptionStore、WorkspaceMonitor 集成、ProactiveEngine 集成添加单元测试
2. **端到端场景测试** — 启动 Agent → 监听文件 → 外部修改 → 感知注入 → Agent 响应
3. **注册感知数据的全局观察工具** — 为 SA 或系统工程师提供检查 PerceptionStore 内容的手段
4. **监控仪表盘集成** — 将通知事件、感知条目可视化到 UI
