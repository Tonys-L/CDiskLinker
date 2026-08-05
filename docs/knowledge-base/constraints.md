# 约束 (Constraints)

> ⚠️ **必读文档**：任何任务都必须阅读本文档。约束不可被绕过。

---

## 设计原则

优先级裁决（冲突时按此顺序）：

```text
业务优先 > 职责优先 > 变更成本 > 简单优先 > 扩展优先
```

核心哲学：

```text
业务优先于技术
业务规则内聚优先于分层隔离
变更成本优先于开发速度
简单优先于复杂
扩展优先于修改
```

任何设计都应首先解决真实业务问题，而非追求某种特定的技术概念。

---

### 优先关注能力，而非数据

在开发移链助手时，优先表达其**功能能力**，而不是只将文件看作数据库的简单数据表：
- 错误：`文件移动`、`配置项修改`
- 正确：`独占解除 (File Lock Release)`、`事务恢复 (Transaction Recovery)`、`重解析点创建 (Reparse Point Creation)`

---

### 职责清晰

每个模块必须能够回答：

```text
我负责什么？
我不负责什么？
```

---

### 避免过度设计

- 不要为猜测中的需求设计（如在 V1.0 提前编写庞大的插件扩展体系）。
- 抽象升级条件：出现真实重复，或变化方向完全一致。

---

## 架构约束

### 三层隔离

逻辑分层，业务内聚、变更隔离、依赖单向。

```text
策略层（UI 调度、重试逻辑）
    ↓
核心层（引擎、日志不变量）
    ↑
技术层（Windows API 适配）
```

#### 架构状态

- **当前状态**：`未实施` (项目处于脚手架搭建的初始化状态)
- **未隔离的模块**：由于尚未编写 Rust 代码，所有模块在代码编写时必须遵循以下隔离规范。

#### 模块定位（架构状态为"未实施"或"部分实施"时填写）

因为系统逻辑处于开发前期，使用该模块定位表来规范架构隔离边界：

| 模块 | 负责什么 | 业务规则位置 | 依赖哪些模块 | 不负责什么 |
|------|----------|-------------|-------------|-----------|
| `ui` | 提供 Vue 3 + Naive UI 界面呈现与事件传递 | `src-ui/` 中的 Vue 组件和 Tauri 事件分发 | `engine`, `scanner` | 具体的扫描算法与文件移动操作 |
| `scanner` | 遍历磁盘目录，统计大小，自动过滤系统黑名单 | `src/scanner.rs` | `win_util` | UI 展现及物理文件迁移 |
| `engine` | 执行双阶段拷贝验证，维护迁移状态机 | `src/engine.rs` | `win_util`, `journal` | UI 样式与底层 Junction 的物理绑定 |
| `journal` | `pending_jobs.json` 事务的存盘与启动恢复自检 | `src/journal.rs` | 无外部依赖 | 底层 Junction 接口的调用 |
| `win_util` | Junction 创建/删除、Restart Manager 占用检测、提权 | `src/win_util.rs` | 无外部依赖 | 迁移流程的状态流转与逻辑控制 |

---

### 依赖方向

- 所有的核心逻辑依赖方向只能单向流入，禁止出现 `engine` 模块与 `ui` 模块的双向隐式耦合。
- `win_util` 仅作为技术实现层，不能带有任何业务迁移状态逻辑。

---

## 业务不变量

| 编号 | 不变量描述 | 检查位置 |
|------|-----------|----------|
| **INV-001** | 跨盘迁移时，在数据完整性校验通过且目标目录重命名为正式名称（final）之前，目标目录必须处于 `.tmp_` 临时前缀状态。在 Junction 创建之前，数据必须已处于目标盘的最终位置（final），防止数据不一致。 | `src/engine.rs` |
| **INV-002** | 任何迁移发起前，目标磁盘的空闲空间在扣除本次迁移所需大小后，必须保留至少 1GB 的硬性安全余量。 | `src/engine.rs` (空间预检) |
| **INV-003** | 绝对禁止对系统黑名单目录（如 `C:\Windows`）及其子目录执行任何读取、修改或删除。 | `src/scanner.rs` 和 `src/engine.rs` |
| **INV-004** | 源目录重命名后（状态 ≥ SourceRenamed），目标盘上的正式目录 `final` 成为唯一权威数据副本，崩溃恢复与异常处理中**绝对禁止删除**该目录。`_cdisklinker_old` 目录为冗余副本（可安全删除），但 `final` 目录受唯一副本保护。 | `src/engine.rs` (handle_crash_recovery) |
| **INV-005** | 目标盘文件系统必须为 NTFS。Junction 重解析点仅在 NTFS 上受支持，FAT32/exFAT/网络盘会导致创建失败。 | `src/engine.rs` (输入校验 0f) |
| **INV-006** | 源路径不能是盘符根目录（如 `C:\`），否则 `file_name()` 为空导致目标目录名异常。 | `src/engine.rs` (输入校验 0e) |
| **INV-007** | 源目录在创建 Junction 之前必须重命名（而非删除），重命名为 `_cdisklinker_old` 后缀。这确保用户发现软件异常时可以即时回滚（重命名回源目录即可），无需重新拷贝数据。 | `src/engine.rs` |
| **INV-008** | `_cdisklinker_old` 目录在用户明确确认迁移正常之前不得删除。仅当用户确认（`Linked` → `Completed` 转换）后方可删除旧源目录。 | `src/engine.rs` |
| **INV-009** | 迁移档案双副本写入顺序：必须先写目标目录的自包含档案（`.cdisklinker_meta.json`），再更新全局索引（`migration_history.json`）。此顺序不可反转，确保自包含档案始终是权威来源。一致性原则：当两者不一致时以自包含档案为准，全局索引可重建。档案写入采用 best_effort 策略，失败不阻止迁移完成。 | `src/migration_history.rs` (write_archive) |

---

## 禁止事项

### 架构禁止

- `engine` 核心层禁止直接调用操作系统原生的命令行工具（如调用 `cmd.exe /c mklink`），必须使用原生的 Windows API 绑定进行重解析点创建。
- `journal` 事务日志文件禁止放在除软件运行目录之外的任何公共位置，防止用户权限不足导致存盘失败。
- 禁止在没有管理员提权的进程中执行迁移操作。

### 编码禁止

- 禁止在 Windows API 报错时默默吞掉（即空 `catch` 或只打印 `println!` 却返回 `Ok(())`），必须将系统 OS 错误码映射为明确的业务异常。
- 树形扫描（DFS）中，禁止采用无界递归，必须设置安全递归深度（最大 32 层）或者转为非递归实现，防止极端深度的路径导致栈溢出。

---

## 项目约束

### 技术约束

- **开发语言**：Rust (Edition 2021) + TypeScript (Vue 3 前端)
- **GUI 框架**：Vue 3 + Naive UI (via Tauri 2.x)
- **构建目标**：`x86_64-pc-windows-msvc`

### 环境约束

- 运行时依赖系统 WebView2（Windows 10 1803+ 内置或自动安装），不需要预装 Node.js/.NET 运行时。
- 单个打包的 `.exe` 独立执行，体积控制在 10MB 内。

---

## 测试约束

| 模块类型 | 覆盖率要求 | 重点测试 |
|----------|-----------|----------|
| 核心层 (`engine`) | >= 80% | 双阶段拷贝状态机的流转、异常中断后的回滚与恢复机制；流式哈希拷贝（含 Junction 不跟入）；Manifest 生成/校验（默认模式 + 快速模式）；Manifest 持久化与 self_hash 篡改检测 |
| 技术层 (`win_util`) | >= 70% | 创建 Junction、解除文件占用、提权验证的 API 边界 |

---

## 发版约定（Release Convention）

> ⚠️ **任何 agent 执行发版操作前必须阅读本章节**。本约定适用于所有 AI 工具（不限于 TRAE），跨 agent / 跨电脑 / 跨工具均需遵守。

### commit message 双语格式（强制）

发版提交的 commit message 必须使用双语格式。CI 会提取 commit body 作为 GitHub Release body，进而写入 `latest.json` 的 `notes` 字段；客户端 `UpdateDialog.vue` 按 `---` 分隔符解析中英文，按当前 locale 显示对应语言。

**格式**：

```
<type>: <中文标题> (v<版本号>)

<中文说明>
---
<英文说明>
```

**规则**：

- 首行：标题，包含 type（feat/fix/docs/...）+ 中文摘要 + 版本号
- 标题后空一行
- 中文说明段落
- 仅含 `---` 的行（前后各空一行）
- 英文说明段落
- CI 使用 `git log -1 --pretty=%b` 提取 body（不含首行标题）作为 Release body

**示例**：

```
feat: 帮助页加版本号与链接 (v1.5.2)

修正官网地址；GitHub 链接改用 SVG 图标；release notes 支持中英文。
---
Fix official website URL; use GitHub SVG icon; release notes support i18n.
```

### 发版流程检查点

每次发版必须：

1. 升版本号（`src-tauri/tauri.conf.json` + `src-tauri/Cargo.toml`）
2. 用双语格式提交 commit message（见上）
3. 创建 tag `v<版本号>`
4. 推送 master 分支 + tag 触发 CI
5. 验证 CI 构建成功
6. 检查 GitHub Release 页：body 应包含以 `---` 分隔的双语内容

### 客户端解析逻辑

`UpdateDialog.vue` 的 `displayNotes` computed：

- 用正则 `/\r?\n---\r?\n/` 拆分 `store.updateInfo.body`
- locale=zh-CN → 取 parts[0]（中文）
- locale=en-US → 取 parts[1]（英文），缺失时回退到 parts[0]
- 无 `---` 分隔符 → 整体显示（向后兼容旧版本）

### CI 实现

见 `.github/workflows/release.yml` 的 `Extract release notes from commit` 步骤。

---

## 变更记录

| 日期 | 变更内容 | 变更人 | 关联变更 |
|------|----------|--------|----------|
| 2026-07-21 | 初始化版本，建立模块定位表与三个核心不变量 | Antigravity | — |
| 2026-07-23 | 新增 INV-004（唯一副本保护）、INV-005（NTFS 强制）、INV-006（根目录禁止） | Antigravity | #TASK-crash-recovery 同步更新 flows.md |
| 2026-07-25 | V2 迁移流程：更新 INV-001（Junction 前数据须在 final 位置）、INV-004（源重命名后 final 为唯一权威副本）；新增 INV-007（源须重命名非删除）、INV-008（_old 须用户确认后方可删）；技术栈更新为 Vue 3 + Naive UI + Tauri 2.x | Antigravity | #TASK-v2-migration-flow 同步更新 flows.md |
| 2026-08-02 | 测试约束补充：核心层重点测试项增加流式哈希拷贝（含 Junction 不跟入）、Manifest 生成/校验、Manifest 持久化与 self_hash 篡改检测 | Antigravity | #TASK-engine-tests |
| 2026-08-04 | 新增 INV-009（迁移档案双副本写入顺序约束） | Antigravity | #TASK-migration-archive 同步更新 boundaries.md、flows.md、glossary.md |
| 2026-08-05 | 新增"发版约定"章节：commit message 双语格式、发版流程检查点、客户端解析逻辑、CI 实现引用 | Antigravity | #TASK-release-convention |
