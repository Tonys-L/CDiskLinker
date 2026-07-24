# ADR-001: 采用 Rust 技术栈与 Slint 图形界面框架

## 状态

Accepted

## 背景

《C盘移链助手》定位于一款极轻量级、零运行库依赖且需要执行高风险 Windows 底层 API（如创建目录联接 Junction、Restart Manager 提权操作）的工具。为此，技术栈选型需要达到：
1. 单 exe 独立部署，打包体积极小。
2. 零外部运行库依赖（无需用户预装 .NET 运行时或 WebView 引擎）。
3. Windows 底层 API 互操作效率高、调用健全。

## 方案选项

### 选项 A: C# (.NET 8 WPF/WinForms)
- **优点**：微软官方生态支持，WPF 做 GUI 开发效率高，与 Windows API 互操作成熟。
- **缺点**：普通用户机可能无 .NET 8 运行时，若打包为独立（Self-contained）发布体积过大；若使用 Native AOT，生态库裁剪限制较大。

### 选项 B: Python + PySide6
- **优点**：开发效率快。
- **缺点**：打包后体积巨大（PyInstaller 打包通常 >60MB），且启动慢，不利于轻量系统级工具的定位。

### 选项 C: Rust + Slint 框架
- **优点**：无垃圾回收器（GC），编译生成单个二进制 exe 文件；打包体积极小（约 5-8MB），内存占用低；Slint 框架没有 WebView 或 JS 运行时依赖，开发出的界面速度极快且完全自主掌控。
- **缺点**：Slint 的成熟度不及 WPF，需要手工编写一部分树形高级控件。

## 决策

选择 **选项 C (Rust + Slint 框架)**。

为了达成极轻量级、绿色无污染的系统级工具定位，Rust 的高性能与 Slint 的无运行时依赖是极佳配合，能够把打包体积和运行内存压制在极低水平，同时保障 Windows API 调用的类型安全性。

## 影响

- 团队需要配置 Rust 2021 开发环境，配置 MSVC 工具链。
- UI 交互需使用 `.slint` 声明式文件编写，并通过 Slint 编译器生成 Rust 绑定。
- 系统级调用将优先寻找微软官方的 `windows` crate。
