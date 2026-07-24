# CDiskLinker UI - Slint 前端源码

本目录包含 C盘移链助手的完整 Slint UI 源码，基于 Rust + Slint 技术栈。

## 📁 文件清单

| 文件 | 说明 |
|------|------|
| `appwindow.slint` | 主窗口根组件，包含全局主题定义和数据结构 |
| `TreeView.slint` | 可展开树形扫描结果列表（核心组件） |
| `WarningDialog.slint` | 高风险迁移确认弹窗 |
| `StatusPanel.slint` | 实时迁移状态面板（含状态机可视化） |
| `LogConsole.slint` | 操作日志滚动面板 |
| `JournalBar.slint` | 底部事务日志状态栏 |
| `DiskOverview.slint` | C盘空间环形概览图 |
| `TargetSelector.slint` | 目标迁移盘选择器 |
| `ui_bridge.rs` | Rust 后端数据桥接示例代码 |

## 🏗️ 项目结构

```
CDiskLinker/
├── Cargo.toml
├── build.rs                  # Slint 编译配置
├── ui/
│   ├── appwindow.slint
│   └── components/
│       ├── TreeView.slint
│       ├── WarningDialog.slint
│       ├── StatusPanel.slint
│       ├── LogConsole.slint
│       ├── JournalBar.slint
│       ├── DiskOverview.slint
│       └── TargetSelector.slint
└── src/
    ├── main.rs
    ├── ui_bridge.rs          # ← 数据桥接层
    ├── scanner.rs
    ├── engine.rs
    ├── journal.rs
    └── win_util.rs
```

## 🔧 使用方式

### 1. 添加 Slint 依赖

在 `Cargo.toml` 中添加：

```toml
[dependencies]
slint = "1.5"

[build-dependencies]
slint-build = "1.5"
```

### 2. 配置 build.rs

```rust
fn main() {
    slint_build::compile("ui/appwindow.slint").unwrap();
}
```

### 3. 在 main.rs 中启动

```rust
slint::include_modules!();

fn main() {
    let app = AppWindow::new().unwrap();

    // 设置回调...
    app.on_scan_disk(|| { /* ... */ });

    app.run().unwrap();
}
```

## 🎨 主题配色

所有颜色定义在 `appwindow.slint` 的 `Theme` 全局对象中：

| 语义 | 色值 | 用途 |
|------|------|------|
| 安全 | `#10b981` | 可迁移目录标识 |
| 警告 | `#f59e0b` | AppData等高风险目录 |
| 禁止 | `#ef4444` | 系统核心目录 |
| 信息 | `#3b82f6` | 按钮、进度条 |
| 联接 | `#8b5cf6` | 已建立Junction的目录 |

## 📋 数据流

```
Rust 后端 (main.rs)
    │
    ├── scanner.rs ──► ScanEntry[] ──► ui_bridge.rs
    │                                    │
    │                                    ▼
    │                              TreeNode[] (Slint Model)
    │                                    │
    │                                    ▼
    │                            Slint UI 渲染
    │                                    │
    │    ◄── 用户点击回调 ──────────────┘
    │
    ├── engine.rs ──► MigrationStatus ──► StatusPanel
    │
    └── journal.rs ──► JournalStage ──► JournalBar
```

## ⚠️ 注意事项

1. **Slint 版本**：本代码基于 Slint 1.5+ 语法，低版本可能需要调整
2. **Path 组件**：`DiskOverview.slint` 中的环形图使用了简化版 Path，如需精确圆弧可能需要自定义绘制
3. **树形展开**：当前实现通过 `is_visible` 字段控制显隐，实际项目中建议维护一个独立的展开状态映射
4. **复选框级联**：父节点勾选时不会自动级联子节点，需在 Rust 回调中手动处理

## 📝 License

MIT
