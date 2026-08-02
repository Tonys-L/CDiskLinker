# 开发环境 - 经验教训

---

### 1.1 Tauri dev 模式无法打开 localhost（Vite IPv6 与 Windows IPv4 解析不一致）

**问题**: 运行 `npm run tauri dev` 后，Tauri 窗口打开但页面显示"无法打开 localhost"。终端日志显示 Vite 已正常启动在 `http://localhost:1420/`，Rust 编译也成功，但 webview 就是连不上。

**原因**: Vite 8.x 默认监听 IPv6 `[::1]:1420`（Node.js 的默认行为），但 Windows 的 DNS 解析 `localhost` 优先返回 IPv4 `127.0.0.1`。Tauri webview 按 `tauri.conf.json` 中的 `devUrl: "http://localhost:1420"` 连接时，实际访问的是 `127.0.0.1:1420`，而该 IPv4 端口上无监听，故连接失败。

关键诊断方法：`netstat -ano | findstr ":1420"` 查看监听地址。若显示 `[::1]:1420` 即为 IPv6 only 监听，与 Tauri webview 的 IPv4 连接不兼容。

**解决方案**: 在 `vite.config.ts` 的 `server.host` 显式绑定 IPv4 地址：

```typescript
// vite.config.ts
server: {
  port: 1420,
  strictPort: true,
  // Windows 上 localhost 默认解析为 IPv4 (127.0.0.1)，
  // 而 Vite 默认监听 IPv6 ([::1])，导致 Tauri webview 连接失败（"无法打开 localhost"）。
  // 显式绑定 127.0.0.1 确保 Tauri webview 可访问。
  host: '127.0.0.1',
},
```

修改后 Vite 日志会显示 `Local: http://127.0.0.1:1420/`（而非 `http://localhost:1420/`），Tauri webview 即可正常加载页面。

**影响文件**:
- `vite.config.ts` — `server.host` 配置

**排查要点**:
1. 确认 Vite 日志中 `Local` 显示的是 `localhost` 还是 `127.0.0.1`，前者可能是 IPv6
2. 用 `netstat -ano | findstr ":1420"` 查看监听地址是 `[::1]`（IPv6）还是 `127.0.0.1`（IPv4）
3. 若 `[::1]` 则需要显式配置 `host: '127.0.0.1'`
4. 注意端口 TIME_WAIT 状态：重启 dev server 前等待 30 秒，或更换端口避免冲突

**适用范围**: 所有 Tauri + Vite 在 Windows 上的开发环境。Linux/macOS 通常无此问题（DNS 解析行为不同）。

**日期**: 2026-08-02

---

## 变更记录

| 日期 | 变更内容 | 变更人 | 关联变更 |
|------|----------|--------|----------|
| 2026-08-02 | 初始版本，记录 Tauri dev 模式 IPv6/IPv4 解析不一致问题 | Antigravity | #TASK-fix-localhost |
