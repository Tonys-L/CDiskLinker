use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::fs::{self, File};
use std::io::{Read, Write};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use widestring::U16CString;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

use crate::scanner::ScanEntry;
use crate::journal::{self, PendingJob, MigrationStage};
use crate::win_util;

/// 结构化迁移进度事件
#[derive(Debug, Clone, Serialize)]
pub struct MigrationProgress {
    pub stage: String,
    pub progress: f32,
    pub total_files: usize,
    pub copied_files: usize,
    pub total_size: u64,
    pub copied_size: u64,
    pub current_file: String,
    pub detail: String,
    /// 以下字段仅在 PendingConfirmation 阶段填充，用于前端确认对话框
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_target_path: Option<String>,
}

impl MigrationProgress {
    fn new(stage: &str, progress: f32, detail: &str) -> Self {
        Self {
            stage: stage.to_string(),
            progress,
            total_files: 0,
            copied_files: 0,
            total_size: 0,
            copied_size: 0,
            current_file: String::new(),
            detail: detail.to_string(),
            source_path: None,
            renamed_source_path: None,
            final_target_path: None,
        }
    }

    /// 创建带进度的 MigrationProgress（不含确认对话框字段）
    fn with_progress(stage: &str, progress: f32, detail: &str,
                     total_files: usize, copied_files: usize,
                     total_size: u64, copied_size: u64,
                     current_file: &str) -> Self {
        Self {
            stage: stage.to_string(),
            progress,
            total_files,
            copied_files,
            total_size,
            copied_size,
            current_file: current_file.to_string(),
            detail: detail.to_string(),
            source_path: None,
            renamed_source_path: None,
            final_target_path: None,
        }
    }
}

/// 查询指定驱动器（如 "D:\"）的剩余可用字节数
pub fn get_disk_free_space(drive: &str) -> Result<u64, String> {
    let drive_w = U16CString::from_str(drive)
        .map_err(|e| format!("驱动器路径编码失败: {}", e))?;

    let mut free_bytes_available = 0u64;
    let mut total_number_of_bytes = 0u64;
    let mut total_number_of_free_bytes = 0u64;

    unsafe {
        let result = GetDiskFreeSpaceExW(
            PCWSTR(drive_w.as_ptr()),
            Some(&mut free_bytes_available),
            Some(&mut total_number_of_bytes),
            Some(&mut total_number_of_free_bytes),
        );

        if result.is_ok() {
            Ok(free_bytes_available)
        } else {
            Err("调用 Windows API 获取剩余空间失败".to_string())
        }
    }
}


// ==================== Manifest 清单校验系统 ====================
// 设计目标：确保迁移过程中零文件丢失。
// 核心原则：删除源之前生成完整清单（含每个文件 SHA256），删除后用清单校验目标。
// 不跟入 Junction：Junction 记录目标路径，复制时重建，校验时比对目标指向。

/// 计算文件的完整 SHA256 哈希 + 返回实际文件大小
/// 关键设计：通过文件句柄获取 metadata，确保 size 和 sha256 对应同一时刻的文件内容
/// 避免目录项缓存（entry.metadata）返回过时大小的问题
fn calculate_full_sha256_with_size(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path)
        .map_err(|e| format!("打开文件失败 {:?}: {}", path, e))?;

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024]; // 256KB 缓冲
    let mut total_read: u64 = 0;
    loop {
        let n = file.read(&mut buffer)
            .map_err(|e| format!("读取文件失败 {:?}: {}", path, e))?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
        total_read += n as u64;
    }

    // 从文件句柄获取实际大小（比目录项缓存更准确）
    // 如果句柄的 metadata 不可用，则用实际读取的字节数
    let actual_size = file.metadata()
        .map(|m| m.len())
        .unwrap_or(total_read);

    Ok((format!("{:x}", hasher.finalize()), actual_size))
}

/// 文件清单条目：每个文件一项
#[derive(Serialize, Deserialize, Debug, Clone)]
struct ManifestFileEntry {
    relative_path: String,  // 相对路径（正斜杠分隔，如 "office6/wps.exe"）
    size: u64,
    sha256: String,
}

/// Junction 清单条目：每个 Junction 一项
#[derive(Serialize, Deserialize, Debug, Clone)]
struct ManifestJunctionEntry {
    relative_path: String,
    target: String,         // Junction 目标绝对路径
}

/// 空目录清单条目：每个空目录一项（非空目录由其文件推断）
#[derive(Serialize, Deserialize, Debug, Clone)]
struct ManifestDirEntry {
    relative_path: String,
}

/// 完整清单：迁移前生成，用于迁移后逐项校验
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Manifest {
    /// 源根目录绝对路径
    pub source_root: String,
    /// 所有文件（不含 Junction 目标内容）
    pub files: Vec<ManifestFileEntry>,
    /// 所有 Junction（不跟入）
    pub junctions: Vec<ManifestJunctionEntry>,
    /// 所有空目录（保持目录结构完整）
    pub empty_dirs: Vec<ManifestDirEntry>,
    /// 文件总数（冗余字段，便于快速校验）
    pub total_files: usize,
    /// 总大小（冗余字段）
    pub total_size: u64,
    /// Manifest 自身内容的 SHA256（持久化时计算，读取时校验）
    #[serde(default)]
    pub self_hash: String,
    /// 生成时间戳（Unix 秒）
    pub created_at: u64,
}

impl Manifest {
    /// 递归生成目录的完整 Manifest
    /// 关键：不跟入 Junction，Junction 单独记录目标路径
    pub fn generate(root: &Path) -> Result<Self, String> {
        let root_str = root.to_string_lossy().to_string();
        let mut files = Vec::new();
        let mut junctions = Vec::new();
        let mut empty_dirs = Vec::new();

        Self::collect_inner(root, root, &mut files, &mut junctions, &mut empty_dirs)?;

        let total_size: u64 = files.iter().map(|f| f.size).sum();
        let total_files = files.len();

        let mut manifest = Manifest {
            source_root: root_str,
            files,
            junctions,
            empty_dirs,
            total_files,
            total_size,
            self_hash: String::new(), // 持久化时填充
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        // 计算自哈希（基于不含 self_hash 的内容）
        manifest.self_hash = manifest.compute_self_hash();

        Ok(manifest)
    }

    /// 递归收集文件、Junction、空目录
    fn collect_inner(
        base: &Path,
        current: &Path,
        files: &mut Vec<ManifestFileEntry>,
        junctions: &mut Vec<ManifestJunctionEntry>,
        empty_dirs: &mut Vec<ManifestDirEntry>,
    ) -> Result<(), String> {
        let mut has_entries = false;

        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(metadata) = entry.metadata() {
                    has_entries = true;

                    // 统一路径分隔符为正斜杠（跨平台一致）
                    let rel = path.strip_prefix(base)
                        .map_err(|e| format!("路径前缀剥离失败: {}", e))?
                        .to_string_lossy()
                        .replace('\\', "/");

                    // Junction：记录目标路径，不跟入
                    if metadata.is_dir() && win_util::is_junction(&path) {
                        let target = win_util::read_junction_target(&path)?
                            .to_string_lossy().to_string();
                        junctions.push(ManifestJunctionEntry {
                            relative_path: rel,
                            target,
                        });
                        continue;
                    }

                    // 文件级符号链接：跳过
                    if metadata.file_type().is_symlink() {
                        continue;
                    }

                    if metadata.is_dir() {
                        // 普通目录：递归
                        Self::collect_inner(base, &path, files, junctions, empty_dirs)?;
                    } else {
                        // 普通文件：计算完整 SHA256 + 取实时文件大小
                        // 关键：先计算 SHA256（读取文件内容），再获取文件大小
                        // 这样确保 size 和 sha256 对应同一时刻的文件内容
                        // 使用 fs::metadata（实时查询）而非 entry.metadata（目录项缓存）
                        let (sha256, actual_size) = calculate_full_sha256_with_size(&path)?;
                        files.push(ManifestFileEntry {
                            relative_path: rel,
                            size: actual_size,
                            sha256,
                        });
                    }
                }
            }
        }

        // 空目录：记录以保持目录结构
        if !has_entries && current != base {
            let rel = current.strip_prefix(base)
                .map_err(|e| format!("路径前缀剥离失败: {}", e))?
                .to_string_lossy()
                .replace('\\', "/");
            empty_dirs.push(ManifestDirEntry { relative_path: rel });
        }

        Ok(())
    }

    /// 计算自身内容哈希（不含 self_hash 字段）
    fn compute_self_hash(&self) -> String {
        let mut copy = self.clone();
        copy.self_hash = String::new();
        let json = serde_json::to_string(&copy).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// 持久化到磁盘（JSON + 自校验哈希）
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Manifest 序列化失败: {}", e))?;
        let mut file = File::create(path)
            .map_err(|e| format!("创建 Manifest 文件失败 {:?}: {}", path, e))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("写入 Manifest 失败: {}", e))?;
        file.flush()
            .map_err(|e| format!("Manifest 落盘失败: {}", e))?;
        Ok(())
    }

    /// 从磁盘读取并校验自哈希
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path)
            .map_err(|e| format!("打开 Manifest 文件失败 {:?}: {}", path, e))?;
        let mut data = String::new();
        file.read_to_string(&mut data)
            .map_err(|e| format!("读取 Manifest 文件失败: {}", e))?;

        let manifest: Manifest = serde_json::from_str(&data)
            .map_err(|e| format!("解析 Manifest 失败: {}", e))?;

        // 校验自哈希
        let expected = manifest.compute_self_hash();
        if manifest.self_hash != expected {
            return Err(format!(
                "Manifest 自校验失败：哈希不匹配（文件可能损坏）。期望 {}，实际 {}",
                expected, manifest.self_hash
            ));
        }

        Ok(manifest)
    }

    /// 逐项校验：将目标目录生成的 Manifest 与本清单比对
    /// 任何一项不匹配返回错误，全部一致返回 Ok
    pub fn verify_against(&self, target_manifest: &Manifest) -> Result<(), String> {
        // 1. 文件总数
        if self.files.len() != target_manifest.files.len() {
            return Err(format!(
                "文件数量不一致：源 {} 个，目标 {} 个",
                self.files.len(), target_manifest.files.len()
            ));
        }

        // 排序后逐项比对（路径 + 大小 + SHA256）
        let mut src_files = self.files.clone();
        let mut tgt_files = target_manifest.files.clone();
        src_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        tgt_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        for (i, (src, tgt)) in src_files.iter().zip(tgt_files.iter()).enumerate() {
            if src.relative_path != tgt.relative_path {
                return Err(format!(
                    "第 {} 个文件路径不匹配：源 {:?}，目标 {:?}",
                    i + 1, src.relative_path, tgt.relative_path
                ));
            }
            if src.size != tgt.size {
                return Err(format!(
                    "文件大小不一致 {:?}：源 {} 字节，目标 {} 字节",
                    src.relative_path, src.size, tgt.size
                ));
            }
            if src.sha256 != tgt.sha256 {
                return Err(format!(
                    "文件 SHA256 不一致 {:?}：源 {}，目标 {}",
                    src.relative_path, src.sha256, tgt.sha256
                ));
            }
        }

        // 2. Junction 数量 + 目标指向
        if self.junctions.len() != target_manifest.junctions.len() {
            return Err(format!(
                "Junction 数量不一致：源 {} 个，目标 {} 个",
                self.junctions.len(), target_manifest.junctions.len()
            ));
        }

        let mut src_jcns = self.junctions.clone();
        let mut tgt_jcns = target_manifest.junctions.clone();
        src_jcns.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        tgt_jcns.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        for (i, (src, tgt)) in src_jcns.iter().zip(tgt_jcns.iter()).enumerate() {
            if src.relative_path != tgt.relative_path {
                return Err(format!(
                    "第 {} 个 Junction 路径不匹配：源 {:?}，目标 {:?}",
                    i + 1, src.relative_path, tgt.relative_path
                ));
            }
            // Junction 目标比对（大小写不敏感，Windows 路径不区分大小写）
            if !src.target.eq_ignore_ascii_case(&tgt.target) {
                return Err(format!(
                    "Junction 目标不一致 {:?}：源 {:?}，目标 {:?}",
                    src.relative_path, src.target, tgt.target
                ));
            }
        }

        // 3. 空目录
        if self.empty_dirs.len() != target_manifest.empty_dirs.len() {
            return Err(format!(
                "空目录数量不一致：源 {} 个，目标 {} 个",
                self.empty_dirs.len(), target_manifest.empty_dirs.len()
            ));
        }

        let mut src_dirs = self.empty_dirs.clone();
        let mut tgt_dirs = target_manifest.empty_dirs.clone();
        src_dirs.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        tgt_dirs.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        for (src, tgt) in src_dirs.iter().zip(tgt_dirs.iter()) {
            if src.relative_path != tgt.relative_path {
                return Err(format!(
                    "空目录不匹配：源 {:?}，目标 {:?}",
                    src.relative_path, tgt.relative_path
                ));
            }
        }

        Ok(())
    }
}

/// 找出两个 Manifest 之间的差异文件列表（大小或 SHA256 不一致的文件）
fn find_manifest_diff_files(source: &Manifest, target: &Manifest) -> Vec<String> {
    let mut diff = Vec::new();

    // 构建 target 文件查找表（相对路径 → 条目）
    let mut tgt_map: std::collections::HashMap<&str, &ManifestFileEntry> = std::collections::HashMap::new();
    for f in &target.files {
        tgt_map.insert(&f.relative_path, f);
    }

    // 比对每个源文件
    for src_file in &source.files {
        if let Some(tgt_file) = tgt_map.get(src_file.relative_path.as_str()) {
            if src_file.size != tgt_file.size || src_file.sha256 != tgt_file.sha256 {
                diff.push(src_file.relative_path.clone());
            }
        } else {
            // 目标缺少此文件
            diff.push(src_file.relative_path.clone());
        }
    }

    diff
}

fn collect_files_recursive(dir: &Path, list: &mut Vec<(PathBuf, u64)>) -> Result<(), String> {
    collect_files_inner(dir, dir, list)
}

fn collect_files_inner(base_dir: &Path, current_dir: &Path, list: &mut Vec<(PathBuf, u64)>) -> Result<(), String> {
    if let Ok(entries) = fs::read_dir(current_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                // Junction 不跟入：只统计本目录树内的真实文件，不统计 Junction 目标中的文件
                // 这与 copy_dir_recursive 的行为一致（Junction 重建而非跟入复制）
                if metadata.is_dir() && win_util::is_junction(&path) {
                    continue; // Junction：不跟入，不统计其目标内容
                }
                if metadata.file_type().is_symlink() {
                    continue; // 文件级符号链接：跳过
                }

                if metadata.is_dir() {
                    collect_files_inner(base_dir, &path, list)?;
                } else {
                    let relative_path = path.strip_prefix(base_dir)
                        .map_err(|e| format!("路径前缀剥离失败: {}", e))?
                        .to_path_buf();
                    list.push((relative_path, metadata.len()));
                }
            }
        }
    }
    Ok(())
}

/// 迁移前预统计：递归统计源目录的文件数量和总大小
pub fn pre_scan_migration(source: &Path) -> Result<(usize, u64), String> {
    let mut files = Vec::new();
    collect_files_recursive(source, &mut files)?;
    let total_size: u64 = files.iter().map(|(_, size)| size).sum();
    Ok((files.len(), total_size))
}

/// 逐个删除目录及其内容，返回第一个删除失败的文件路径
///
/// 与 fs::remove_dir_all 的区别：
/// 1. 失败时能定位具体是哪个文件/子目录删不掉
/// 2. 遇到 Junction 只删除链接点，**绝不跟入目标**（防止误删其他位置数据）
///
/// 部分删除是安全的：数据已在 tmp 中（Copied 状态），源不完整不影响恢复。
fn remove_dir_all_with_detail(dir: &Path) -> Result<(), (String, Option<PathBuf>)> {
    fn remove_recursive(path: &Path) -> Result<(), (String, Option<PathBuf>)> {
        // 关键：先检测 Junction！
        // Rust std 的 is_symlink() 在 Windows 上不识别 Junction，
        // 如果不先检测，Junction 会被当作普通目录跟入，误删目标数据。
        if path.is_dir() && win_util::is_junction(path) {
            // Junction：只删链接点，不跟入目标（remove_dir 对 Junction 安全）
            fs::remove_dir(path).map_err(|e| (format!("{}", e), Some(path.to_path_buf())))?;
        } else if path.is_symlink() {
            // 文件级符号链接：删除链接本身
            fs::remove_file(path).map_err(|e| (format!("{}", e), Some(path.to_path_buf())))?;
        } else if path.is_dir() {
            for entry in fs::read_dir(path).map_err(|e| (format!("{}", e), Some(path.to_path_buf())))? {
                let entry = entry.map_err(|e| (format!("{}", e), None))?;
                remove_recursive(&entry.path())?;
            }
            fs::remove_dir(path).map_err(|e| (format!("{}", e), Some(path.to_path_buf())))?;
        } else {
            fs::remove_file(path).map_err(|e| (format!("{}", e), Some(path.to_path_buf())))?;
        }
        Ok(())
    }
    remove_recursive(dir)
}

/// 格式化占用进程列表为字符串
fn format_locks(locks: &[(u32, String)]) -> String {
    let names: Vec<String> = locks.iter()
        .map(|(pid, name)| format!("{} (PID:{})", name, pid))
        .collect();
    format!("，占用进程: {}", names.join(", "))
}

/// 检测目录占用进程（带降级策略）
///
/// 检测顺序：
/// 1. 目录级 Restart Manager 检测（query_file_locks）
/// 2. 递归批量检测所有子文件（query_dir_locks_recursive）
/// 3. 若进程检测均失败，定位第一个被锁定的文件（find_locked_file）
///
/// 返回格式化字符串（如 "，占用进程: wps.exe (PID:1234)" 或 "，被占用的文件: xxx\yyy.exe"）。
fn detect_locks_with_fallback(dir: &Path) -> String {
    // 1. 尝试目录级进程检测
    let process_info = match win_util::query_file_locks(dir) {
        Ok(locks) if !locks.is_empty() => Some(format_locks(&locks)),
        Ok(_) => {
            // 目录级未检测到，递归批量检测子文件
            match win_util::query_dir_locks_recursive(dir) {
                Ok(locks) if !locks.is_empty() => Some(format_locks(&locks)),
                _ => None,
            }
        }
        Err(_) => {
            // 目录级检测失败，尝试递归
            match win_util::query_dir_locks_recursive(dir) {
                Ok(locks) if !locks.is_empty() => Some(format_locks(&locks)),
                _ => None,
            }
        }
    };

    if let Some(info) = process_info {
        return info;
    }

    // 2. 进程检测失败，定位被锁定的文件（输出相对路径）
    if let Some(locked_file) = win_util::find_locked_file(dir) {
        let rel = locked_file.strip_prefix(dir)
            .unwrap_or(&locked_file)
            .to_string_lossy();
        return format!("，被占用的文件: {}", rel);
    }

    "，未检测到占用进程或锁定文件".to_string()
}

/// 递归物理文件拷贝核心逻辑，实时发送结构化进度（带节流）
/// base_src 为源根目录，用于计算文件的相对路径以在日志中显示完整层级
fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    base_src: &Path,
    copied_bytes: &mut u64,
    copied_files: &mut usize,
    total_bytes: u64,
    total_files: usize,
    tx: &Sender<MigrationProgress>,
) -> Result<(), String> {
    if !dst.exists() {
        fs::create_dir_all(dst)
            .map_err(|e| format!("无法在目标位置创建子目录: {}", e))?;
    }

    use std::time::Instant;
    let mut last_send = Instant::now();

    if let Ok(entries) = fs::read_dir(src) {
        for entry in entries.flatten() {
            let s_path = entry.path();
            let d_path = dst.join(entry.file_name());

            if let Ok(metadata) = entry.metadata() {
                // 检测子目录是否为 Junction（重解析点）
                // 关键：必须先检测再判断 is_dir，否则 Junction 会被当作普通目录递归进入
                if metadata.is_dir() && win_util::is_junction(&s_path) {
                    // 子目录是 Junction：读取其目标路径，在目标位置重建同指向的 Junction
                    let rel = s_path.strip_prefix(base_src)
                        .unwrap_or(&s_path)
                        .to_string_lossy();
                    match win_util::read_junction_target(&s_path) {
                        Ok(junction_target) => {
                            // 在目标位置创建同指向的 Junction
                            if let Err(e) = win_util::create_junction(&d_path, &junction_target) {
                                return Err(format!("重建子目录 Junction 失败 [{} -> {:?}]: {}", rel, junction_target, e));
                            }
                            let _ = tx.send(MigrationProgress {
                                stage: "Copying".to_string(),
                                progress: 0.0, // 保持上一条进度
                                total_files,
                                copied_files: *copied_files,
                                total_size: total_bytes,
                                copied_size: *copied_bytes,
                                current_file: String::new(),
                                detail: format!("重建联接: {} -> {:?}", rel, junction_target),
                                source_path: None,
                                renamed_source_path: None,
                                final_target_path: None,
                            });
                        }
                        Err(e) => {
                            return Err(format!("读取子目录 Junction 目标失败 [{}]: {}", rel, e));
                        }
                    }
                    continue; // Junction 已重建，跳过递归
                }

                if metadata.file_type().is_symlink() {
                    // 文件级符号链接：跳过（罕见，一般只出现在开发者环境）
                    continue;
                }

                if metadata.is_dir() {
                    copy_dir_recursive(&s_path, &d_path, base_src, copied_bytes, copied_files, total_bytes, total_files, tx)?;
                } else {
                    // 计算相对于源根目录的相对路径，日志中显示完整层级
                    let file_name = s_path.strip_prefix(base_src)
                        .unwrap_or(&s_path)
                        .to_string_lossy()
                        .to_string();

                    let mut src_file = File::open(&s_path)
                        .map_err(|e| format!("无法打开源文件 {:?}: {}", s_path, e))?;
                    let mut dst_file = File::create(&d_path)
                        .map_err(|e| format!("无法创建目标文件 {:?}: {}", d_path, e))?;

                    let mut buffer = [0u8; 64 * 1024]; // 64KB 拷贝块缓冲
                    loop {
                        let bytes_read = src_file.read(&mut buffer)
                            .map_err(|e| format!("读取源文件失败: {}", e))?;
                        if bytes_read == 0 {
                            break;
                        }
                        dst_file.write_all(&buffer[..bytes_read])
                            .map_err(|e| format!("写入目标文件失败: {}", e))?;

                        *copied_bytes += bytes_read as u64;

                        // 节流：至少 100ms 发一次，或文件读完时发一次
                        let should_send = last_send.elapsed().as_millis() >= 100;
                        if should_send {
                            let progress = if total_bytes > 0 {
                                (*copied_bytes as f32 / total_bytes as f32) * 80.0 + 10.0
                            } else {
                                90.0
                            };

                            let _ = tx.send(MigrationProgress {
                                stage: "Copying".to_string(),
                                progress,
                                total_files,
                                copied_files: *copied_files,
                                total_size: total_bytes,
                                copied_size: *copied_bytes,
                                current_file: file_name.clone(),
                                detail: format!("复制中: {} ({}/{})", file_name, *copied_files + 1, total_files),
                                source_path: None,
                                renamed_source_path: None,
                                final_target_path: None,
                            });
                            last_send = Instant::now();
                        }
                    }
                    dst_file.flush().map_err(|e| format!("数据落盘同步失败: {}", e))?;
                    *copied_files += 1;

                    // 每个文件复制完成后也发一次（确保小文件有日志）
                    let progress = if total_bytes > 0 {
                        (*copied_bytes as f32 / total_bytes as f32) * 80.0 + 10.0
                    } else {
                        90.0
                    };
                    let _ = tx.send(MigrationProgress {
                        stage: "Copying".to_string(),
                        progress,
                        total_files,
                        copied_files: *copied_files,
                        total_size: total_bytes,
                        copied_size: *copied_bytes,
                        current_file: file_name.clone(),
                        detail: format!("已复制: {} ({}/{})", file_name, *copied_files, total_files),
                        source_path: None,
                        renamed_source_path: None,
                        final_target_path: None,
                    });
                    last_send = Instant::now();
                }
            }
        }
    }

    Ok(())
}

/// 构造源目录的 _cdisklinker_old 重命名路径
/// 例如 C:\Steam → C:\Steam._cdisklinker_old
fn make_old_path(source_dir: &Path) -> PathBuf {
    PathBuf::from(format!("{}.{}", source_dir.display(), "_cdisklinker_old"))
}

/// 执行双阶段物理迁移（V2：先重命名源→建链接→用户确认→删旧源）
pub fn execute_migration(
    entry: &ScanEntry,
    target_drive: &str,
    tx: Sender<MigrationProgress>,
) -> Result<(), String> {
    let source_dir = &entry.path;

    // 0. 检测同源 Copied/Finalized 状态恢复
    //    如果存在同源的 Copied 日志，说明拷贝+校验已完成，
    //    跳过拷贝直接从后续步骤继续，避免重复拷贝
    if let Ok(Some(existing_job)) = journal::read_job() {
        if existing_job.source_path == *source_dir {
            match existing_job.stage {
                MigrationStage::Copied | MigrationStage::Finalized => {
                    let _ = tx.send(MigrationProgress::new("Resuming", 90.0,
                        "检测到上次拷贝已完成，正在恢复迁移（跳过拷贝，从重命名步骤继续）..."));
                    return resume_migration_from_copied(existing_job, &tx);
                }
                _ => {}
            }
        }
    }

    // 1. 输入校验
    // 1a. 源目录必须存在
    if !source_dir.exists() {
        return Err(format!("源目录不存在: {:?}", source_dir));
    }
    // 1b. 源路径必须是目录
    if !source_dir.is_dir() {
        return Err(format!("源路径不是目录: {:?}", source_dir));
    }
    // 1c. 源目录不能是 Junction（已迁移过的目录）
    if win_util::is_junction(source_dir) {
        return Err(format!("源目录已是 NTFS 联接（可能已迁移过）: {:?}", source_dir));
    }
    // 1d. 目标路径不能与源路径在同一盘
    let source_drive = source_dir.to_str()
        .and_then(|s| s.get(..2))
        .unwrap_or("");
    let target_drive_letter = target_drive.get(..2).unwrap_or("");
    if source_drive.eq_ignore_ascii_case(target_drive_letter) {
        return Err("目标路径与源路径在同一磁盘，无需跨盘迁移".to_string());
    }
    // 1e. 源路径不能是盘符根目录（file_name() 为空会导致目标目录名异常）
    if source_dir.file_name().is_none() {
        return Err(format!("源路径不能是盘符根目录: {:?}", source_dir));
    }
    // 1f. 目标盘文件系统必须是 NTFS（Junction 仅 NTFS 支持，FAT32/exFAT/网络盘会失败）
    let target_drive_root = if target_drive.len() >= 3 {
        format!("{}\\", &target_drive[..2])
    } else {
        format!("{}\\", target_drive)
    };
    match win_util::is_ntfs(&target_drive_root) {
        Ok(true) => {}
        Ok(false) => {
            return Err(format!("目标盘 {} 不是 NTFS 文件系统，无法创建 Junction 联接（FAT32/exFAT/网络盘不支持）", target_drive_letter));
        }
        Err(e) => {
            return Err(format!("无法获取目标盘文件系统信息: {}", e));
        }
    }

    // 1g. _cdisklinker_old 路径不能已存在（避免重命名冲突）
    let old_source_path = make_old_path(source_dir);
    if old_source_path.exists() {
        return Err(format!(
            "源目录的旧备份路径已存在 {:?}，请先手动删除或重命名后重试",
            old_source_path
        ));
    }

    // 2. 获取目标磁盘盘符的可用空间，校验是否满足业务不变量 INV-002
    let drive_root = if target_drive.len() >= 3 {
        &target_drive[..3]
    } else {
        target_drive
    };

    let free_space = get_disk_free_space(drive_root)
        .map_err(|e| format!("无法获取目标盘空间信息（盘符可能无效）: {}", e))?;

    // 3. 【预统计】递归统计源目录文件数量和总大小
    let _ = tx.send(MigrationProgress::new("PreScanning", 1.0, "正在预统计源目录文件..."));
    let (total_files, total_size) = pre_scan_migration(source_dir)?;

    if total_files == 0 {
        return Err("源目录为空，没有可迁移的文件".to_string());
    }

    let _ = tx.send(MigrationProgress {
        stage: "PreScanned".to_string(),
        progress: 5.0,
        total_files,
        copied_files: 0,
        total_size,
        copied_size: 0,
        current_file: String::new(),
        detail: format!("预统计完成: {} 个文件, 总计 {}", total_files, crate::scanner::format_size(total_size)),
        source_path: None,
        renamed_source_path: None,
        final_target_path: None,
    });

    let required_space = total_size;
    let safe_margin = (required_space as f64 * 1.1) as u64 + 1024 * 1024 * 1024; // 1.1倍 + 1GB

    if free_space < safe_margin {
        return Err(format!(
            "目标盘空间不足！可用空间：{}，安全余量要求：{}，请清理后再试。",
            crate::scanner::format_size(free_space),
            crate::scanner::format_size(safe_margin)
        ));
    }

    // 4. 构造目标路径和临时路径
    let target_base_dir = Path::new(target_drive);
    let final_target_path = target_base_dir.join(&entry.name);

    let tmp_folder_name = format!(".tmp_{}", entry.name);
    let target_tmp_path = target_base_dir.join(&tmp_folder_name);

    if final_target_path.exists() {
        return Err(format!("目标路径已存在同名正式文件夹 {:?}，迁移失败", final_target_path));
    }
    if target_tmp_path.exists() {
        let _ = remove_dir_all_with_detail(&target_tmp_path);
    }

    // 5. 【生成源端 Manifest 并持久化】删除源之前的权威清单
    //    崩溃恢复时用此清单校验目标完整性（即使源已删也能校验）
    let _ = tx.send(MigrationProgress::new("PreScanning", 7.0, "正在生成源端文件清单(SHA256)..."));
    let source_manifest = Manifest::generate(source_dir)?;

    // Manifest 文件路径与 pending_jobs.json 同目录
    let manifest_path = journal::get_journal_dir()?.join(format!("manifest_{}.json", uuid_v4_like()));
    source_manifest.save_to_file(&manifest_path)?;

    // 6. 注册并写入待办事务日志 (Stage = Initiated，含 manifest 路径)
    //    原子顺序：先写 Manifest → 再写日志。崩溃时若日志有 Initiated 但无 Manifest，视为步骤1前崩溃
    let job = PendingJob {
        job_id: uuid_v4_like(),
        source_path: source_dir.clone(),
        target_path: target_tmp_path.clone(),
        final_target_path: final_target_path.clone(),
        stage: MigrationStage::Initiated,
        manifest_path: Some(manifest_path.clone()),
        renamed_source_path: None,
    };
    journal::write_job(&job)?;

    let _ = tx.send(MigrationProgress {
        stage: "Copying".to_string(),
        progress: 10.0,
        total_files,
        copied_files: 0,
        total_size,
        copied_size: 0,
        current_file: String::new(),
        detail: "开始物理复制文件流...".to_string(),
        source_path: None,
        renamed_source_path: None,
        final_target_path: None,
    });

    // 7. 执行第一阶段：物理复制数据流并发送进度
    let mut copied_bytes = 0u64;
    let mut copied_files = 0usize;
    let copy_result = copy_dir_recursive(
        source_dir,
        &target_tmp_path,
        source_dir,
        &mut copied_bytes,
        &mut copied_files,
        total_size,
        total_files,
        &tx,
    );

    if let Err(e) = copy_result {
        let _ = remove_dir_all_with_detail(&target_tmp_path);
        let _ = journal::clear_job();
        let _ = fs::remove_file(&manifest_path);
        return Err(format!("文件拷贝流遇到异常终止: {}", e));
    }

    // 8. 【Manifest 逐项校验】生成目标端 Manifest，与源端逐项比对
    //    这是删除源之前的最终防线：每个文件的路径+大小+SHA256 必须完全一致
    let _ = tx.send(MigrationProgress {
        stage: "Verifying".to_string(),
        progress: 90.0,
        total_files,
        copied_files: total_files,
        total_size,
        copied_size: total_size,
        current_file: String::new(),
        detail: "正在校验物理数据完整性与哈希值...".to_string(),
        source_path: None,
        renamed_source_path: None,
        final_target_path: None,
    });

    let target_manifest = Manifest::generate(&target_tmp_path)?;
    if let Err(e) = source_manifest.verify_against(&target_manifest) {
        // 校验不一致：可能是源在复制期间被活跃进程修改（竞态条件）
        // 解决方案：用源当前最新内容重新覆盖差异文件，然后重新校验
        // 最多重试 2 次，防止源持续被修改导致无限循环
        let _ = tx.send(MigrationProgress::new("Verifying", 91.0,
            &format!("检测到源目录在复制期间被修改（{}），正在同步差异文件...", e)));

        let mut retry_count = 0;
        let max_retries = 2;

        loop {
            // 找出差异文件并重新复制
            let fresh_source_manifest = Manifest::generate(source_dir)?;
            let diff_files = find_manifest_diff_files(&fresh_source_manifest, &target_manifest);

            if diff_files.is_empty() {
                // 源和目标完全一致（不太可能到这里，但防御性编程）
                fresh_source_manifest.save_to_file(&manifest_path)?;
                break;
            }

            let _ = tx.send(MigrationProgress::new("Verifying", 92.0,
                &format!("正在重新复制 {} 个差异文件（第 {} 次重试）...", diff_files.len(), retry_count + 1)));

            // 重新复制差异文件
            for rel_path in &diff_files {
                let src_file = source_dir.join(rel_path);
                let tgt_file = target_tmp_path.join(rel_path);
                if src_file.exists() {
                    fs::copy(&src_file, &tgt_file)
                        .map_err(|e2| format!("重新复制差异文件失败 {:?}: {}", rel_path, e2))?;
                }
            }

            // 重新生成目标 Manifest 并校验
            let new_target_manifest = Manifest::generate(&target_tmp_path)?;
            match fresh_source_manifest.verify_against(&new_target_manifest) {
                Ok(()) => {
                    // 一致了，更新持久化 Manifest
                    fresh_source_manifest.save_to_file(&manifest_path)?;
                    let _ = tx.send(MigrationProgress::new("Verifying", 93.0,
                        "差异文件已同步，校验通过"));
                    break;
                }
                Err(e2) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        let _ = remove_dir_all_with_detail(&target_tmp_path);
                        let _ = journal::clear_job();
                        let _ = fs::remove_file(&manifest_path);
                        return Err(format!(
                            "完整性校验失败：源目录在迁移期间持续被修改（{}），请关闭占用程序后重试",
                            e2
                        ));
                    }
                    // 重试
                    continue;
                }
            }
        }
    }

    let _ = tx.send(MigrationProgress {
        stage: "Verifying".to_string(),
        progress: 95.0,
        total_files,
        copied_files: total_files,
        total_size,
        copied_size: total_size,
        current_file: String::new(),
        detail: "Manifest 校验通过，准备激活目标目录...".to_string(),
        source_path: None,
        renamed_source_path: None,
        final_target_path: None,
    });

    // 9. 标记状态至 Copied（拷贝+校验通过，源还在，tmp 完整）
    let mut job = job;
    job.stage = MigrationStage::Copied;
    journal::write_job(&job)?;

    // 10. 将临时文件夹重命名为正式目标目录（tmp → final）
    let _ = tx.send(MigrationProgress {
        stage: "Renaming".to_string(),
        progress: 96.0,
        total_files,
        copied_files: total_files,
        total_size,
        copied_size: total_size,
        current_file: String::new(),
        detail: "激活重命名目标文件夹目录结构...".to_string(),
        source_path: None,
        renamed_source_path: None,
        final_target_path: None,
    });

    if let Err(e) = fs::rename(&target_tmp_path, &final_target_path) {
        // rename 失败：源完好，tmp 完整 → 保留 Copied 状态，用户可重试
        return Err(format!(
            "重命名目标文件夹失败: {}。源目录完好，数据完整保存在临时目录 {:?}，请重试迁移。",
            e, target_tmp_path
        ));
    }

    // 10.1 标记状态至 Finalized（tmp 已改名 final，源仍在原位置）
    job.stage = MigrationStage::Finalized;
    journal::write_job(&job)?;

    // 11. 将源目录重命名为 _cdisklinker_old（腾出原路径用于 Junction）
    let _ = tx.send(MigrationProgress {
        stage: "Renaming".to_string(),
        progress: 97.0,
        total_files,
        copied_files: total_files,
        total_size,
        copied_size: total_size,
        current_file: String::new(),
        detail: "正在重命名源目录以腾出原路径...".to_string(),
        source_path: None,
        renamed_source_path: None,
        final_target_path: None,
    });

    if let Err(e) = fs::rename(source_dir, &old_source_path) {
        // 源重命名失败：回滚 - 将 final rename 回 tmp，保持源完整
        let _ = fs::rename(&final_target_path, &target_tmp_path);
        // 回退到 Copied 状态
        job.stage = MigrationStage::Copied;
        job.target_path = target_tmp_path.clone();
        let _ = journal::write_job(&job);
        let lock_info = detect_locks_with_fallback(source_dir);
        return Err(format!(
            "重命名源目录失败: {}{}。源目录完好，请关闭占用程序后重试。",
            e, lock_info
        ));
    }

    // 11.1 标记状态至 SourceRenamed（源已重命名，原路径已腾出，final 是权威副本）
    job.stage = MigrationStage::SourceRenamed;
    job.renamed_source_path = Some(old_source_path.clone());
    journal::write_job(&job)?;

    // 12. 建立 NTFS Junction 联接重定向
    let _ = tx.send(MigrationProgress {
        stage: "Linking".to_string(),
        progress: 98.0,
        total_files,
        copied_files: total_files,
        total_size,
        copied_size: total_size,
        current_file: String::new(),
        detail: "正在原位置建立重解析点 (NTFS Junction) 重定向...".to_string(),
        source_path: None,
        renamed_source_path: None,
        final_target_path: None,
    });

    if let Err(e) = win_util::create_junction(source_dir, &final_target_path) {
        // Junction 创建失败：回滚 - 删除 Junction 占位（如果已创建部分），rename _old 回原路径
        // 先尝试把 _old rename 回源路径
        if fs::rename(&old_source_path, source_dir).is_ok() {
            // 源恢复成功，final 还在，回退到 Finalized 状态
            job.stage = MigrationStage::Finalized;
            job.renamed_source_path = None;
            let _ = journal::write_job(&job);
        }
        return Err(format!(
            "建立 NTFS 目录联接失败: {}。数据已安全迁移至 {:?}，源目录已恢复原位，请手动执行 mklink /J \"{:?}\" \"{:?}\" 创建联接后重试。",
            e, final_target_path, source_dir, final_target_path
        ));
    }

    // 12.1 标记状态至 Linked（Junction 已建，迁移功能上完成，等待用户确认）
    job.stage = MigrationStage::Linked;
    journal::write_job(&job)?;

    // 13. 通知前端：迁移完成，等待用户确认
    //     不 clear_job！不删除 _old！保持 Linked 状态，等用户确认后再清理
    let _ = tx.send(MigrationProgress {
        stage: "PendingConfirmation".to_string(),
        progress: 100.0,
        total_files,
        copied_files: total_files,
        total_size,
        copied_size: total_size,
        current_file: String::new(),
        detail: "迁移已完成！请测试软件是否正常使用".to_string(),
        source_path: Some(source_dir.to_string_lossy().to_string()),
        renamed_source_path: old_source_path.to_str().map(|s| s.to_string()),
        final_target_path: Some(final_target_path.to_string_lossy().to_string()),
    });

    Ok(())
}

/// 从 Copied/Finalized 状态恢复迁移（重试时跳过拷贝+校验）
///
/// 当删除源目录失败或后续步骤中断时，保留 Copied/Finalized 状态日志和 tmp/final 目录。
/// 用户重新点击迁移，execute_migration 入口检测到同源 Copied/Finalized 日志，
/// 调用本函数跳过拷贝+校验，直接从后续步骤继续。
fn resume_migration_from_copied(
    mut job: PendingJob,
    tx: &Sender<MigrationProgress>,
) -> Result<(), String> {
    let source_dir = &job.source_path;
    let final_target_path = &job.final_target_path;

    // 验证源还在
    if !source_dir.exists() {
        let _ = journal::clear_job();
        return Err("恢复失败：源目录不存在，无法继续迁移".to_string());
    }

    // 根据当前阶段确定 tmp 和 final 的位置
    let target_tmp_path = &job.target_path;

    // 如果处于 Copied 状态，先验证 tmp 并 rename 到 final
    if job.stage == MigrationStage::Copied {
        // 验证 tmp 还在
        if !target_tmp_path.exists() {
            let _ = journal::clear_job();
            return Err("恢复失败：临时目录不存在，请重新迁移".to_string());
        }

        // 用持久化的源端 Manifest 校验目标 tmp 完整性
        let _ = tx.send(MigrationProgress::new("Verifying", 92.0, "恢复迁移：正在用 Manifest 校验已拷贝数据完整性..."));

        let source_manifest = match &job.manifest_path {
            Some(mp) if mp.exists() => {
                Manifest::load_from_file(mp).map_err(|e| format!("加载 Manifest 失败: {}", e))?
            }
            _ => {
                // Manifest 丢失，用源实时生成（降级方案）
                Manifest::generate(source_dir)?
            }
        };

        let target_manifest = Manifest::generate(target_tmp_path)?;
        if let Err(e) = source_manifest.verify_against(&target_manifest) {
            // tmp 数据不完整或被篡改，清理后要求重新迁移
            let _ = remove_dir_all_with_detail(target_tmp_path);
            let _ = journal::clear_job();
            if let Some(mp) = &job.manifest_path {
                let _ = fs::remove_file(mp);
            }
            return Err(format!("恢复失败：临时目录数据不完整（{}），已清理，请重新迁移", e));
        }

        let _ = tx.send(MigrationProgress::new("Renaming", 96.0, "恢复迁移：正在重命名临时目录为正式目录..."));

        // 步骤10: rename tmp → final
        if let Err(e) = fs::rename(target_tmp_path, final_target_path) {
            return Err(format!(
                "重命名目标文件夹失败: {}。数据完整保存在 {:?}，请手动重命名为 {:?}。",
                e, target_tmp_path, final_target_path
            ));
        }

        // 标记 Finalized
        job.stage = MigrationStage::Finalized;
        journal::write_job(&job)?;
    }

    // 从 Finalized 状态继续：rename 源 → _old → 建 Junction → Linked
    if job.stage == MigrationStage::Finalized {
        // 验证 final 存在
        if !final_target_path.exists() {
            let _ = journal::clear_job();
            return Err(format!("恢复失败：目标目录 {:?} 不存在，请重新迁移", final_target_path));
        }

        // 验证源还在且不是 Junction
        if win_util::is_junction(source_dir) {
            let _ = journal::clear_job();
            return Err("恢复失败：源路径已是 Junction，可能迁移已完成".to_string());
        }

        let old_source_path = make_old_path(source_dir);

        // 检查 _old 路径是否已存在
        if old_source_path.exists() {
            let _ = journal::clear_job();
            return Err(format!(
                "源目录的旧备份路径已存在 {:?}，请先手动删除后重试",
                old_source_path
            ));
        }

        let total_files = 0;
        let total_size = 0u64;

        // 步骤11: rename 源 → _old
        let _ = tx.send(MigrationProgress::new("Renaming", 97.0, "恢复迁移：正在重命名源目录以腾出原路径..."));

        if let Err(e) = fs::rename(source_dir, &old_source_path) {
            let lock_info = detect_locks_with_fallback(source_dir);
            return Err(format!(
                "重命名源目录失败: {}{}。请关闭占用进程后重试。",
                e, lock_info
            ));
        }

        // 标记 SourceRenamed
        job.stage = MigrationStage::SourceRenamed;
        job.renamed_source_path = Some(old_source_path.clone());
        journal::write_job(&job)?;

        // 步骤12: 建 Junction
        let _ = tx.send(MigrationProgress::new("Linking", 98.0, "恢复迁移：正在建立 NTFS Junction..."));

        if let Err(e) = win_util::create_junction(source_dir, final_target_path) {
            // Junction 失败，尝试回滚 rename
            if fs::rename(&old_source_path, source_dir).is_ok() {
                job.stage = MigrationStage::Finalized;
                job.renamed_source_path = None;
                let _ = journal::write_job(&job);
            }
            return Err(format!(
                "建立 NTFS 目录联接失败: {}。请手动执行 mklink /J \"{:?}\" \"{:?}\"。",
                e, source_dir, final_target_path
            ));
        }

        // 标记 Linked
        job.stage = MigrationStage::Linked;
        journal::write_job(&job)?;

        let _ = tx.send(MigrationProgress {
            stage: "PendingConfirmation".to_string(),
            progress: 100.0,
            total_files,
            copied_files: total_files,
            total_size,
            copied_size: total_size,
            current_file: String::new(),
            detail: "迁移已完成！请测试软件是否正常使用".to_string(),
            source_path: Some(job.source_path.to_string_lossy().to_string()),
            renamed_source_path: job.renamed_source_path.as_ref().and_then(|p| p.to_str().map(|s| s.to_string())),
            final_target_path: Some(job.final_target_path.to_string_lossy().to_string()),
        });

        return Ok(());
    }

    // 不应到达此处
    Err(format!("恢复失败：不支持从 {:?} 状态恢复", job.stage))
}

/// 程序启动自检恢复或回滚异常中断的事务
///
/// 恢复策略核心原则：**绝不可删除可能是唯一数据副本的目录**。
/// 每个状态依据"日志状态 + 文件系统实际状态"推导恢复动作：
/// - 源重命名前（Initiated/Copied/Finalized）：源是权威，tmp/final 是冗余副本
/// - 源重命名后（SourceRenamed/Linked）：final 是权威，_old 是冗余副本（可删）
/// - 用户确认后（Completed）：final 是唯一权威
pub fn handle_crash_recovery() -> Result<Option<String>, String> {
    if let Some(job) = journal::read_job()? {
        match job.stage {
            MigrationStage::Initiated => {
                // 拷贝中途崩溃：tmp 可能不完整，源应完好
                if job.target_path.exists() {
                    let _ = remove_dir_all_with_detail(&job.target_path);
                }
                if job.source_path.exists() {
                    let _ = journal::clear_job();
                    if let Some(mp) = &job.manifest_path { let _ = fs::remove_file(mp); }
                    Ok(Some(format!(
                        "检测到上次拷贝中途故障中断的事务 {:?}，已安全回滚（删除不完整的临时目录），C盘源数据完好无损。",
                        job.source_path
                    )))
                } else {
                    // 异常：源不在但 tmp 在（Initiated 阶段源不该被删）
                    Ok(Some(format!(
                        "⚠️ 异常状态：事务 {:?} 处于拷贝阶段但源目录不存在，临时目录 {:?} 可能含部分数据，已保留待人工确认。",
                        job.source_path, job.target_path
                    )))
                }
            }
            MigrationStage::Copied => {
                // 拷贝+校验通过，源应还在，tmp 完整
                // 不删 tmp！保留 Copied 状态，用户重新迁移同目录可从后续步骤恢复（无需重新拷贝）
                if job.source_path.exists() && !win_util::is_junction(&job.source_path) {
                    Ok(Some(format!(
                        "检测到上次拷贝完成但后续步骤中断的迁移任务 {:?}。请重新迁移该目录，将从重命名步骤继续（无需重新拷贝）。",
                        job.source_path
                    )))
                } else {
                    // 源不在或已是 Junction → 源可能已被处理，tmp 是唯一完整副本，保留待用户处理
                    Ok(Some(format!(
                        "⚠️ 事务 {:?} 处于已校验状态但源目录不存在。完整数据保存在临时目录 {:?}，请手动重命名为 {:?} 并创建 Junction，或联系技术支持。",
                        job.source_path, job.target_path, job.final_target_path
                    )))
                }
            }
            MigrationStage::Finalized => {
                // tmp 已改名 final，源应仍在原位置
                // 检查 final 和源状态，继续从 rename 源步骤开始
                if !job.final_target_path.exists() {
                    // final 不存在，检查 tmp 是否还在（rename 可能未完成）
                    if job.target_path.exists() {
                        Ok(Some(format!(
                            "⚠️ 事务 {:?} 处于 Finalized 状态但正式目标目录 {:?} 不存在，临时目录 {:?} 仍在。请手动重命名临时目录为正式目录后重试。",
                            job.source_path, job.final_target_path, job.target_path
                        )))
                    } else {
                        let _ = journal::clear_job();
                        if let Some(mp) = &job.manifest_path { let _ = fs::remove_file(mp); }
                        Ok(Some(format!(
                            "❌ 事务 {:?} 处于 Finalized 状态但目标目录均不存在，数据可能丢失，请检查。",
                            job.source_path
                        )))
                    }
                } else if job.source_path.exists() && !win_util::is_junction(&job.source_path) {
                    // final 在 + 源在 → 自动继续：rename 源→_old → 建 Junction
                    let old_source_path = make_old_path(&job.source_path);
                    if old_source_path.exists() {
                        Ok(Some(format!(
                            "检测到未完成的迁移 {:?}，但旧备份路径 {:?} 已存在。请手动处理后重试。",
                            job.source_path, old_source_path
                        )))
                    } else if let Err(e) = fs::rename(&job.source_path, &old_source_path) {
                        Ok(Some(format!(
                            "⚠️ 自动恢复：重命名源目录失败: {}。请关闭占用程序后重试。",
                            e
                        )))
                    } else {
                        // 源重命名成功，更新日志
                        let mut job = job;
                        job.stage = MigrationStage::SourceRenamed;
                        job.renamed_source_path = Some(old_source_path.clone());
                        let _ = journal::write_job(&job);

                        // 自动建 Junction
                        if let Err(e) = win_util::create_junction(&job.source_path, &job.final_target_path) {
                            // Junction 失败，回滚 rename
                            let _ = fs::rename(&old_source_path, &job.source_path);
                            job.stage = MigrationStage::Finalized;
                            job.renamed_source_path = None;
                            let _ = journal::write_job(&job);
                            return Ok(Some(format!(
                                "⚠️ 自动恢复：重命名源成功但创建 Junction 失败: {}。请手动执行 mklink /J \"{:?}\" \"{:?}\"。",
                                e, job.source_path, job.final_target_path
                            )));
                        }

                        // 完成，进入 Linked 状态
                        job.stage = MigrationStage::Linked;
                        let _ = journal::write_job(&job);
                        Ok(Some(format!(
                            "✅ 自动恢复完成：事务 {:?} 已自动完成源重命名与 Junction 创建，迁移成功。请测试软件是否正常使用。",
                            job.source_path
                        )))
                    }
                } else {
                    // 源不在或已是 Junction → 源可能已被其他方式处理
                    Ok(Some(format!(
                        "⚠️ 事务 {:?} 处于 Finalized 状态但源目录不在原位或已是 Junction。数据安全保存在 {:?}，请手动检查。",
                        job.source_path, job.final_target_path
                    )))
                }
            }
            MigrationStage::SourceRenamed => {
                // 源已重命名为 _old，final 应在，需创建 Junction
                // 获取 _old 路径（如果日志中没有 renamed_source_path，则推导）
                let deduced_old_path = job.renamed_source_path.clone()
                    .unwrap_or_else(|| {
                        eprintln!("警告：SourceRenamed 状态缺少 renamed_source_path，尝试推导");
                        make_old_path(&job.source_path)
                    });

                if !deduced_old_path.exists() {
                    // _old 不存在 → 源可能未重命名成功或已被删除
                    if job.source_path.exists() && !win_util::is_junction(&job.source_path) {
                        // 源还在原位 → 回退到 Finalized，让用户重试
                        let mut job = job;
                        job.stage = MigrationStage::Finalized;
                        job.renamed_source_path = None;
                        let _ = journal::write_job(&job);
                        return Ok(Some(format!(
                            "检测到事务 {:?} 处于 SourceRenamed 状态但旧源目录不存在，源目录仍在原位。已回退至 Finalized 状态，请重试。",
                            job.source_path
                        )));
                    }
                    let _ = journal::clear_job();
                    return Ok(Some(format!(
                        "❌ 事务 {:?} 处于 SourceRenamed 状态但旧源目录 {:?} 不存在，源也不在原位，数据可能丢失，请检查。",
                        job.source_path, deduced_old_path
                    )));
                }

                if !job.final_target_path.exists() {
                    let _ = journal::clear_job();
                    return Ok(Some(format!(
                        "❌ 事务 {:?} 处于 SourceRenamed 状态但目标目录 {:?} 不存在，数据可能丢失，请检查。",
                        job.source_path, job.final_target_path
                    )));
                }

                // _old 在 + final 在 → 自动建 Junction
                if let Err(e) = win_util::create_junction(&job.source_path, &job.final_target_path) {
                    return Ok(Some(format!(
                        "⚠️ 自动恢复：数据完整但创建 Junction 失败: {}。请手动执行 mklink /J \"{:?}\" \"{:?}\"。",
                        e, job.source_path, job.final_target_path
                    )));
                }

                let mut job = job;
                job.stage = MigrationStage::Linked;
                if job.renamed_source_path.is_none() {
                    job.renamed_source_path = Some(deduced_old_path);
                }
                let _ = journal::write_job(&job);
                Ok(Some(format!(
                    "✅ 自动恢复完成：事务 {:?} 已自动创建 Junction，迁移成功。请测试软件是否正常使用。",
                    job.source_path
                )))
            }
            MigrationStage::Linked => {
                // Junction 已建，_old 仍存在 → 提示用户确认删除或回滚
                Ok(Some(format!(
                    "检测到已完成的迁移事务 {:?}，等待确认。请确认软件正常使用后点击确认删除旧数据，或如需回滚请执行即时回滚。",
                    job.source_path
                )))
            }
            MigrationStage::Completed => {
                // 完全完成 → 清理日志
                let _ = journal::clear_job();
                if let Some(mp) = &job.manifest_path { let _ = fs::remove_file(mp); }
                Ok(Some(format!(
                    "检测到已完成的迁移事务 {:?}，已清理残留日志。",
                    job.source_path
                )))
            }
        }
    } else {
        Ok(None)
    }
}

/// 用户确认迁移正常，删除旧源目录（_cdisklinker_old）
///
/// 仅在 Linked 状态下可调用。
/// 删除 _old 目录后，迁移进入 Completed 状态，不可再即时回滚。
pub fn confirm_delete_source(source_path: &Path) -> Result<(), String> {
    // 读取当前任务
    let mut job = journal::read_job()?
        .ok_or_else(|| "没有进行中的迁移任务".to_string())?;

    // 验证阶段为 Linked
    if job.stage != MigrationStage::Linked {
        return Err(format!(
            "当前迁移状态为 {:?}，仅在 Linked 状态下可确认删除旧源",
            job.stage
        ));
    }

    // 验证源路径匹配
    if job.source_path != *source_path {
        return Err(format!(
            "任务源路径 {:?} 与请求路径 {:?} 不匹配",
            job.source_path, source_path
        ));
    }

    // 获取 _old 路径（如果日志中没有 renamed_source_path，则推导）
    let deduced_old_path = job.renamed_source_path.clone()
        .unwrap_or_else(|| {
            eprintln!("警告：Linked 状态缺少 renamed_source_path，尝试推导");
            make_old_path(source_path)
        });

    // 删除 _old 目录（Junction 安全删除）
    if deduced_old_path.exists() {
        remove_dir_all_with_detail(&deduced_old_path)
            .map_err(|(e, failed_path)| {
                let file_info = failed_path.map(|p| {
                    let rel = p.strip_prefix(&deduced_old_path).unwrap_or(&p).to_string_lossy();
                    format!("，失败的文件: {}", rel)
                }).unwrap_or_default();
                format!("删除旧源目录失败: {}{}{}", e, file_info,
                    detect_locks_with_fallback(&deduced_old_path))
            })?;
    }

    // 标记 Completed
    job.stage = MigrationStage::Completed;
    journal::write_job(&job)?;

    // 清理日志和 Manifest
    let _ = journal::clear_job();
    if let Some(mp) = &job.manifest_path {
        let _ = fs::remove_file(mp);
    }

    Ok(())
}

/// 即时回滚：从 Linked/SourceRenamed/Finalized 状态回滚
///
/// 无需数据拷贝，仅通过重命名/删除操作即可恢复。
/// - Linked: 删除 Junction → rename _old 回原路径
/// - SourceRenamed: rename _old 回原路径（可选：rename final→tmp）
/// - Finalized: rename final→tmp（源仍在原位，只需清理 final）
pub fn rollback_migration_instant(source_path: &Path) -> Result<(), String> {
    // 读取当前任务
    let job = journal::read_job()?
        .ok_or_else(|| "没有进行中的迁移任务".to_string())?;

    // 验证源路径匹配
    if job.source_path != *source_path {
        return Err(format!(
            "任务源路径 {:?} 与请求路径 {:?} 不匹配",
            job.source_path, source_path
        ));
    }

    match job.stage {
        MigrationStage::Linked => {
            // 1. 删除 Junction
            if win_util::is_junction(source_path) {
                win_util::delete_junction(source_path)?;
            }

            // 2. rename _old 回原路径
            let old_path = job.renamed_source_path.as_ref()
                .cloned()
                .unwrap_or_else(|| make_old_path(source_path));

            if old_path.exists() {
                fs::rename(&old_path, source_path)
                    .map_err(|e| format!(
                        "回滚失败：将 {:?} 重命名回 {:?} 失败: {}",
                        old_path, source_path, e
                    ))?;
            } else {
                return Err(format!(
                    "回滚失败：旧源目录 {:?} 不存在，无法恢复原目录",
                    old_path
                ));
            }

            // 3. 清理 final 目录（数据已回到源位置，final 不再需要）
            if job.final_target_path.exists() {
                let _ = remove_dir_all_with_detail(&job.final_target_path);
            }

            // 清理
            let _ = journal::clear_job();
            if let Some(mp) = &job.manifest_path {
                let _ = fs::remove_file(mp);
            }
            Ok(())
        }
        MigrationStage::SourceRenamed => {
            // 1. rename _old 回原路径
            let old_path = job.renamed_source_path.as_ref()
                .cloned()
                .unwrap_or_else(|| make_old_path(source_path));

            if old_path.exists() {
                fs::rename(&old_path, source_path)
                    .map_err(|e| format!(
                        "回滚失败：将 {:?} 重命名回 {:?} 失败: {}",
                        old_path, source_path, e
                    ))?;
            } else {
                return Err(format!(
                    "回滚失败：旧源目录 {:?} 不存在，无法恢复原目录",
                    old_path
                ));
            }

            // 2. 清理 final 目录（源已恢复，final 不再需要）
            if job.final_target_path.exists() {
                let _ = remove_dir_all_with_detail(&job.final_target_path);
            }

            // 清理
            let _ = journal::clear_job();
            if let Some(mp) = &job.manifest_path {
                let _ = fs::remove_file(mp);
            }
            Ok(())
        }
        MigrationStage::Finalized => {
            // 源仍在原位，只需清理 final 目录
            if job.final_target_path.exists() {
                let _ = remove_dir_all_with_detail(&job.final_target_path);
            }

            // 清理
            let _ = journal::clear_job();
            if let Some(mp) = &job.manifest_path {
                let _ = fs::remove_file(mp);
            }
            Ok(())
        }
        other => Err(format!(
            "即时回滚不支持 {:?} 状态，仅支持 Linked/SourceRenamed/Finalized",
            other
        ))
    }
}

/// 执行已完成迁移的撤销恢复（需二次确认，需数据拷贝）
///
/// 这是从 Completed 状态回滚的罕见场景：用户在确认删除旧源后
/// 又想把数据搬回 C 盘。需要物理拷贝数据。
pub fn rollback_completed_migration(
    source_junction: &Path,
    real_target: &Path,
    tx: Sender<MigrationProgress>,
) -> Result<(), String> {
    if !source_junction.exists() {
        return Err("源位置链接路径不存在，无法回滚".to_string());
    }
    if !real_target.exists() {
        return Err("迁移目标地物理路径不存在，无法回滚".to_string());
    }

    let _ = tx.send(MigrationProgress::new("RollingBack", 10.0, "开始清除 C 盘的 Junction 重解析点链接..."));

    // 1. 删除 C 盘的 Junction 占位符
    win_util::delete_junction(source_junction)?;

    let _ = tx.send(MigrationProgress::new("RollingBack", 20.0, "正在计算 D 盘实际占用大小..."));
    let total_bytes = crate::scanner::calculate_dir_size(real_target, 1);
    let (total_files, _) = pre_scan_migration(real_target).unwrap_or((0, total_bytes));

    let c_free = get_disk_free_space("C:\\")?;
    if c_free < total_bytes + 1024 * 1024 * 1024 {
        let _ = win_util::create_junction(source_junction, real_target);
        return Err("C 盘剩余可用容量不足以塞回已迁移的文件数据！".to_string());
    }

    let _ = tx.send(MigrationProgress::new("RollingBack", 25.0, "正在生成 D 盘文件清单(SHA256)..."));

    // 拷贝前生成 D 盘 Manifest（作为回滚的源端清单）
    let source_manifest = Manifest::generate(real_target)?;

    let _ = tx.send(MigrationProgress::new("RollingBack", 30.0, "正在将文件搬运回 C 盘原位置..."));

    // 2. 拷贝数据回原位置
    let mut copied_bytes = 0u64;
    let mut copied_files = 0usize;
    let copy_result = copy_dir_recursive(
        real_target,
        source_junction,
        real_target,
        &mut copied_bytes,
        &mut copied_files,
        total_bytes,
        total_files,
        &tx,
    );

    if let Err(e) = copy_result {
        let _ = remove_dir_all_with_detail(source_junction);
        let _ = win_util::create_junction(source_junction, real_target);
        return Err(format!("将文件拷贝回 C 盘时发生故障: {}", e));
    }

    let _ = tx.send(MigrationProgress::new("RollingBack", 90.0, "正在用 Manifest 校验回滚数据完整性..."));

    // 用 Manifest 逐项校验：D 盘清单 vs C 盘回滚后清单
    let target_manifest = Manifest::generate(source_junction)?;
    if let Err(e) = source_manifest.verify_against(&target_manifest) {
        // 回滚数据不完整，保留 C 盘数据（可能是部分拷贝），重建 Junction 指向 D 盘原始数据
        let _ = remove_dir_all_with_detail(source_junction);
        let _ = win_util::create_junction(source_junction, real_target);
        return Err(format!("数据回滚完整性校验失败: {}。已恢复 Junction 指向 D 盘原始数据", e));
    }

    let _ = tx.send(MigrationProgress::new("RollingBack", 95.0, "校验通过，安全删除目标盘残留副本文件..."));

    if let Err((e, _)) = remove_dir_all_with_detail(real_target) {
        return Err(format!("清理 D 盘残留大文件时失败: {}", e));
    }

    let _ = tx.send(MigrationProgress::new("Done", 100.0, "撤销回滚已成功完成！文件数据已完全还原至 C 盘原位置。"));
    Ok(())
}

/// 生成简单的 UUID 代替符号，作为事务的 job_id
fn uuid_v4_like() -> String {
    let mut data = [0u8; 16];
    // 填充随机字节数据
    for item in &mut data {
        *item = (rand_simple() % 256) as u8;
    }
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        data[0], data[1], data[2], data[3],
        data[4], data[5],
        data[6], data[7],
        data[8], data[9],
        data[10], data[11], data[12], data[13], data[14], data[15]
    )
}

/// 编写无外部复杂加密伪随机生成器以减少体积
fn rand_simple() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    (start & 0xFFFFFFFF) as u32
}
