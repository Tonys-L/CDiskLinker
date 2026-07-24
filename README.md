# CDiskLinker

C 盘空间释放工具 — 将大型目录安全迁移至其他磁盘，并通过 NTFS Junction 实现透明访问。

## 功能特性

- **安全迁移**：复制→校验→删除→建链，全流程 Manifest 清单逐文件 SHA256 校验，零文件丢失
- **透明访问**：迁移后在原位置创建 NTFS Junction，应用程序无感知
- **崩溃恢复**：事务日志 + Manifest 持久化，任意步骤崩溃后可自动恢复
- **占用检测**：迁移前检测文件占用进程，删除失败时定位具体被占用的文件
- **异步扫描**：目录树秒开，文件大小后台异步计算
- **管理员提权**：自动检测并请求 UAC 提权

## 技术栈

| 层 | 技术 |
|---|------|
| 前端 | Vue 3 + TypeScript + Naive UI + Pinia |
| 后端 | Rust + Tauri 2.x |
| 构建 | Vite + Cargo |
| 安装包 | NSIS / MSI |

## 安全保障

1. **Manifest 清单校验**：删除源之前，每个文件的路径、大小、SHA256 必须完全一致
2. **三级降级检测**：目录级 Restart Manager → 递归批量检测 → 文件锁定定位
3. **Junction 安全**：复制时重建 Junction（不跟入），删除时只删链接点（不跟入目标）
4. **唯一副本保护**：源删除后，目标数据是唯一副本，任何步骤失败都不删除
5. **竞态修复**：源被修改时自动同步差异文件并重新校验

## 开发

```bash
# 安装依赖
npm install

# 开发模式
npx tauri dev

# 打包
npx tauri build
```

## 项目结构

```
CDiskLinker/
├── src/                    # Vue 3 前端
│   ├── components/         # UI 组件
│   ├── stores/             # Pinia 状态管理
│   └── views/              # 页面视图
├── src-tauri/              # Rust 后端
│   └── src/
│       ├── engine.rs       # 迁移引擎（Manifest 校验、复制、删除、建链）
│       ├── win_util.rs     # Windows API（Junction、文件锁、提权）
│       ├── journal.rs      # 事务日志（崩溃恢复）
│       ├── scanner.rs      # 目录扫描
│       └── commands.rs     # Tauri Command 桥接
└── docs/knowledge-base/    # 知识库（约束、流程、术语）
```

## License

MIT
