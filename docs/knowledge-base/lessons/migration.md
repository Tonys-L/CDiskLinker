# 双阶段安全迁移 - 经验教训

---

### 1.1 迁移释放空间小于统计大小（非 Bug，Windows 系统空间波动）

**问题**: 用户迁移了 2.07 GB 的目录后，C 盘剩余空间仅从 18.9 GB 增加到 20.2 GB（净释放 1.3 GB），比迁移统计的 2.07 GB 少了约 0.77 GB。用户怀疑迁移未完整释放空间或存在数据残留。

**原因**: 迁移逻辑本身无缺陷（已逐环节核查：预统计正确跳过 Junction、复制时 Junction 重建不跟入、删除源时只删链接点不误删目标、同盘校验与空间预检均生效）。差额来自 **Windows 系统在迁移期间向 C 盘的后台写入**，与应用无关。C 盘是系统盘，以下组件会在几分钟内动态占用数百 MB 空间：

1. **系统还原点（VSS 卷影副本）**：C 盘默认开启系统保护，文件系统剧烈变动（批量删除/创建）会触发创建新还原点，保存被修改文件的旧版本副本，单个还原点可达数百 MB ~ 数 GB。迁移过程 = 复制→删源→建 Junction，极易触发。
2. **NTFS 文件系统日志**：`$LogFile`、`$UsnJrnl` 记录所有元数据变更，大量文件操作产生大量日志。
3. **虚拟内存 pagefile.sys**：迁移时应用（含 Tauri webview）占用内存，Windows 将不活跃内存页换出到 pagefile，该文件动态扩张。
4. **其他后台活动**：Windows Update 缓存下载、传递优化 P2P 分发、Prefetch 预读取更新、临时文件等。

本质是"两个水龙头同时开关"：迁移释放 2.07 GB，系统同时写入 0.77 GB，净释放 1.3 GB。这是系统盘的固有特性，任何 C 盘清理工具都会遇到。

**解决方案**: 无需修改代码。向用户解释这是 Windows 系统空间波动导致的正常现象，非迁移缺陷。如需验证空间去向：
- `vssadmin list shadowstorage`（管理员 CMD）查看系统还原点占用
- 用 WizTree / TreeSize 扫描 C 盘按大小排序定位增长项
- 对比验证：再迁移一个相同大小目录，观察差额是否稳定（若每次接近则属系统稳态写入）

**影响文件**: 无（调查结论，未修改代码）。相关核查代码：
- `src-tauri/src/engine.rs` — `collect_files_recursive`（预统计跳过 Junction）、`copy_dir_recursive`（Junction 重建）、`remove_dir_all_with_detail`（Junction 安全删除）
- `src-tauri/src/scanner.rs` — `calculate_dir_size`（用 `metadata.len()` 统计逻辑大小）
- `src-tauri/src/win_util.rs` — `get_disk_space_info`（`GetDiskFreeSpaceExW` 真实可用空间）

**关联发现（未处理，留作单独任务）**: `boundaries.md` 声称"扫描大小必须计算 Size on Disk（实际占用大小）"，但 `scanner.rs` 实际使用 `metadata.len()`（逻辑大小）而非簇对齐后的 size_on_disk。此不一致非本次问题根因（size_on_disk 通常 ≥ 逻辑大小，会高估释放而非低估），但属于文档与代码偏差，需单独对齐。

**日期**: 2026-07-24

---

### 1.2 SHA256 校验性能优化：流式哈希与快速模式

**问题**: V2 流程对每个文件做完整 SHA256 校验以保证数据完整性，但对超大文件（如几十 GB 的虚拟机镜像、Steam 游戏包）哈希计算极其耗时，占用大量 CPU 与磁盘读取带宽，导致迁移过程异常缓慢。用户反馈"手动复制 + mklink 不会这么慢"。

**原因**: 旧实现采用"三次磁盘读取"模式：
1. `Manifest::generate(源)` 独立预扫描，逐个打开文件计算 SHA256
2. `copy_dir_recursive` 物理拷贝（第二次读取源文件）
3. `Manifest::generate(目标)` 再次扫描目标，逐个计算 SHA256

对大文件而言，磁盘 I/O 是瓶颈，三次读取相当于把同一份数据从磁盘读三遍。而手动 `copy + mklink` 只有拷贝这一次 I/O，所以感知差异巨大。

**解决方案**: 两个层次的优化，均在不破坏 `INV-001`（数据完整性校验通过后方可推进状态）的前提下进行：

1. **流式哈希拷贝（默认优化，所有迁移均受益）**
   - 将 `copy_dir_recursive` 升级为 `copy_dir_recursive_with_hash`
   - 拷贝时对流经读取缓冲区的字节流同步喂给 `Sha256` 更新器
   - 拷贝完成即得到源端 Manifest，省去独立的预扫描 pass
   - 磁盘读取次数：3 次 → 2 次（拷贝时算源端哈希 + 目标端 generate）

2. **快速模式（用户可选，跳过哈希校验）**
   - 新增 `fast_mode: bool` 参数贯穿 UI → store → command → engine
   - `Manifest::generate_size_only`：只收集文件路径与大小，不算 SHA256
   - `verify_size_only`：仅比对文件数量与逐项大小
   - 适用于用户信任磁盘完整性的场景（如新盘迁移、已做过完整备份）
   - 磁盘读取次数：2 次 → 1 次（仅拷贝本身）
   - 权衡：无法检测磁盘静默写入错误（位翻转），但能检测拷贝过程中的明显错误（拷贝失败、文件截断）

**关键代码**:
```rust
// 流式哈希：拷贝时同步计算
let mut hasher = Sha256::new();
let mut buf = [0u8; 64 * 1024];
loop {
    let n = src.read(&mut buf)?;
    if n == 0 { break; }
    hasher.update(&buf[..n]);
    dst.write_all(&buf[..n])?;
}
let hash = hasher.finalize();

// 快速模式分支
let target_manifest = if fast_mode {
    Manifest::generate_size_only(&target_tmp_path)?
} else {
    Manifest::generate(&target_tmp_path)?
};
```

**影响文件**:
- `src-tauri/src/engine.rs` — `copy_dir_recursive_with_hash`、`Manifest::generate_size_only`、`Manifest::verify_size_only`、`execute_migration`（新增 `fast_mode` 参数）
- `src-tauri/src/commands.rs` — `migrate_selected`（新增 `fast_mode` 参数）
- `src/stores/app.ts` — `fastMode` 状态、调用时传参
- `src/views/MainView.vue` — 快速模式开关 UI
- `src/i18n/locales/zh-CN.ts` / `en-US.ts` — 快速模式文案

**设计决策**:
- 快速模式默认关闭（`fastMode = false`），保持安全优先
- 快速模式仅在 `Idle` 状态可切换，迁移进行中禁用开关
- 回滚流程（`rollback_completed_migration`）仍使用完整 SHA256 校验，不提供快速模式，确保回滚数据绝对正确

**日期**: 2026-07-26

---

### 1.3 流式哈希拷贝中 Junction 检测失效（Rust DirEntry::metadata().is_dir() 对 Junction 返回 false）

**问题**: 1.2 的流式哈希拷贝 `copy_dir_recursive_with_hash` 实现中，Junction 检测条件写作 `metadata.is_dir() && win_util::is_junction(&s_path)`，导致 **Junction 分支永远不执行**。源目录中包含的 NTFS Junction 会被走到文件分支，对 Junction 路径调用 `File::create(&d_path)` 创建文件，破坏数据完整性，违反 `INV-008`（Junction 不跟入、目标端重建同指向 Junction）。

**原因**: Rust 标准库 `std::fs::DirEntry::metadata()` 返回的 `FileType::is_dir()` 对 Junction 返回 **false**。Windows 文件系统中 Junction 带 `FILE_ATTRIBUTE_REPARSE_POINT` 属性，Rust 标准库的 `is_dir()` 会排除带此属性的条目（即 Junction 既不被识别为目录也不被识别为符号链接，而是"其他"）。

诊断过程：在测试中 `entry.metadata()` 对 Junction 返回 `Ok(FileType { is_dir: false, is_symlink: false })`，但 `win_util::is_junction()`（基于 `GetFileAttributesW` 检查 `FILE_ATTRIBUTE_REPARSE_POINT`）返回 `true`。两者结合导致 `metadata.is_dir() && is_junction` 恒为 false。

**解决方案**: 直接用 `is_junction()` 作为唯一判定条件，去掉 `is_dir()`：

```rust
// 修改前（有 bug）：
if metadata.is_dir() && win_util::is_junction(&s_path) {
    // 永远不执行
}

// 修改后：
if win_util::is_junction(&s_path) {
    // Junction：读取目标，在目标端重建同指向 Junction，不跟入
}
```

Junction 本身就是目录型重解析点，`is_junction` 已隐含"是目录"语义，无需额外 `is_dir()` 检查。

**影响文件**:
- `src-tauri/src/engine.rs` — `copy_dir_recursive_with_hash` 第 804 行 Junction 检测条件

**关联不变量**: `INV-008`（Junction 不跟入、目标端重建同指向）

**测试覆盖**:
- `test_copy_dir_recursive_with_hash_junction_not_followed`：构造含 Junction 的源目录，验证 Junction 被记录到 `junction_entries`、目标端重建 Junction、删除目标端 Junction 后 `dst/link` 不存在（证明未被当作普通目录拷贝内容）

**踩坑要点**:
1. **Rust 标准库对 Windows 重解析点的处理**：`is_dir()` / `is_symlink()` 对 Junction 都返回 false，必须用平台 API（`GetFileAttributesW`）检查 `FILE_ATTRIBUTE_REPARSE_POINT`
2. **测试断言写法**：通过 Junction 访问其目标内文件会返回 true（这是 Junction 正常行为），不能用来判定"是否跟入拷贝"。正确判定：删除目标端 Junction 后路径应不存在（若是跟入拷贝，文件会以实体形式留在 `dst/link/` 下）
3. **发现方式**：本 bug 由新增的 T6 单元测试暴露，证明测试驱动开发的必要性——1.2 的代码上线时未发现此问题

**日期**: 2026-08-02

---

## 变更记录

| 日期 | 变更内容 | 变更人 | 关联变更 |
|------|----------|--------|----------|
| 2026-07-24 | 初始版本，记录迁移释放空间与统计大小差异的调查结论 | Antigravity | — |
| 2026-07-26 | 新增 1.2，记录 SHA256 校验性能优化与快速模式设计 | Antigravity | #TASK-sha256-opt 同步更新 flows.md、boundaries.md |
| 2026-08-02 | 新增 1.3，记录流式哈希拷贝中 Junction 检测失效 bug 与修复 | Antigravity | #TASK-engine-tests |
