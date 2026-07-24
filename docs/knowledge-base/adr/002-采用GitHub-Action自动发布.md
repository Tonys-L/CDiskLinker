# ADR-002: 采用 GitHub Action 自动发布

## 状态

Accepted

## 背景

CDiskLinker 是一款 Windows 桌面应用，基于 Tauri v2 构建。在引入自动发布流程前，每次发版需要开发者本地执行 `npx tauri build`，再手动将 `.exe` / `.msi` 产物上传到 GitHub Release。该手动流程存在以下问题：

1. **环境依赖**：构建者本地必须配置完整的 Rust + Node.js + MSVC 工具链。
2. **易出错**：手动上传产物容易遗漏或选错文件。
3. **不可复现**：本地环境差异可能导致不同版本的构建产物不一致。
4. **无法并行**：构建过程占用本地机器，阻塞开发。

需要一套自动化方案，在版本发布时自动构建并发布到 GitHub Release。

## 方案选项

### 选项 A: 本地脚本 + 手动上传

开发者本地运行脚本完成构建后，手动上传到 GitHub Release。

优点：
- 实现简单，无需 CI 配置。

缺点：
- 仍然依赖本地环境。
- 手动上传易遗漏。
- 无法保证构建环境一致性。

### 选项 B: 自建 CI 服务器

自建 Jenkins / GitLab Runner 等执行构建。

优点：
- 完全自主可控。

缺点：
- 维护成本高，需额外服务器。
- 项目为开源单机工具，自建 CI 过度设计。

### 选项 C: GitHub Actions + tauri-apps/tauri-action

使用 GitHub Actions 配合官方 `tauri-apps/tauri-action` 自动构建并发布。

优点：
- 官方维护，与 Tauri v2 兼容性好。
- 与 GitHub Release 原生集成，零额外服务。
- 构建环境一致（windows-latest runner）。
- 仅在推送 tag 时触发，不浪费 CI 资源。

缺点：
- 依赖外部 Action（tauri-apps/tauri-action）。
- 构建时长约 10-15 分钟（可接受）。

## 决策

选择 **选项 C (GitHub Actions + tauri-apps/tauri-action)**。

理由：项目为开源 Windows 工具，GitHub Actions 免费额度充足；tauri-action 是 Tauri 官方维护的发布方案，与项目技术栈天然契合；tag 触发方式语义清晰，与版本号绑定。

## 发布流程

### 触发方式

推送 `v*` 格式的 tag（如 `v1.0.0`）时自动触发，同时支持 `workflow_dispatch` 手动触发。

### Workflow 文件

位置：`.github/workflows/release.yml`

关键配置：
- **运行环境**：`windows-latest`（项目仅支持 Windows，依赖 Windows 原生 API）
- **权限**：`contents: write`（创建 GitHub Release 所需）
- **构建步骤**：checkout → Node.js LTS → Rust stable → `npm ci` → `tauri-apps/tauri-action@v0`
- **产物**：自动生成 `.exe` / `.msi` 安装包并上传到 GitHub Release

### 发布操作步骤

```bash
# 1. 确保版本号已更新（src-tauri/tauri.conf.json 和 src-tauri/Cargo.toml 的 version 字段）
# 2. 提交变更
git add -A
git commit -m "release: vx.x.x"

# 3. 打 tag 并推送
git tag vx.x.x
git push origin main --tags

# 4. 等待 GitHub Actions 自动构建完成
#    在仓库 Actions 页面查看构建进度
#    构建成功后 Release 自动创建
```

### 版本号约定

- 遵循语义化版本（SemVer）：`vMAJOR.MINOR.PATCH`
- tag 格式：`v` 前缀 + 版本号（如 `v1.0.0`）
- `tauri.conf.json` 和 `Cargo.toml` 中的 `version` 字段必须与 tag 一致（不含 `v` 前缀）

## 影响

- 发版不再依赖本地环境，任何有仓库写权限的成员均可发布。
- 构建环境标准化为 `windows-latest`，结果可复现。
- 后续如需支持 macOS/Linux，只需在 workflow 的 matrix 中增加平台（当前项目仅 Windows，无需扩展）。
- 需维护 workflow 文件随 Tauri 版本演进。

## 注意事项

- `tauri-apps/tauri-action@v0` 使用主版本标签，兼顾稳定性与新特性。如遇破坏性变更，需锁定具体 commit 或版本号。
- workflow 仅授予 `contents: write` 最小权限，遵循最小权限原则。
- 如构建失败，在 GitHub Actions 页面查看日志排查，常见原因为依赖安装失败或 Rust 编译错误。
