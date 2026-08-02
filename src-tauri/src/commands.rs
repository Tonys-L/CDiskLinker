use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use tauri::{AppHandle, Emitter};

use crate::scanner::{self, DirectoryRating, ScanEntry};
use crate::engine::{self, MigrationProgress};
use crate::win_util;

// === 数据模型 ===

#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    pub id: i32,
    pub path: String,
    pub name: String,
    pub size_text: String,
    pub actual_size_bytes: i64,
    pub rating: String,
    pub level: i32,
    pub is_expanded: bool,
    pub has_children: bool,
    pub is_selected: bool,
    pub is_junction: bool,
    pub is_visible: bool,
    pub children_count: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfo {
    pub drive: String,
    pub total: String,
    pub free: String,
    pub used_percent: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrashRecoveryResult {
    pub found: bool,
    pub message: String,
    /// 当前 journal 的迁移阶段（前端据此决定显示哪些恢复按钮）
    /// 值为 "Initiated" / "Copied" / "Finalized" / "SourceRenamed" / "Linked" / "Completed"
    /// 仅当 found=true 时有意义
    pub stage: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LockProcess {
    pub pid: u32,
    pub name: String,
}

// === 全局 ID 计数器 ===
static GLOBAL_ID_COUNTER: AtomicI32 = AtomicI32::new(0);

// === 辅助函数 ===

fn scan_entry_to_tree_node(entry: &ScanEntry, id: i32) -> TreeNode {
    TreeNode {
        id,
        path: entry.path.to_string_lossy().into_owned(),
        name: entry.name.clone(),
        size_text: entry.size_on_disk_formatted.clone(),
        actual_size_bytes: entry.size_in_bytes as i64,
        rating: match entry.rating {
            DirectoryRating::Safe => "Safe".to_string(),
            DirectoryRating::Warning => "Warning".to_string(),
            DirectoryRating::Forbidden => "Forbidden".to_string(),
        },
        level: entry.depth,
        is_expanded: entry.expanded,
        has_children: entry.has_children,
        is_selected: false,
        is_junction: win_util::is_junction(&entry.path),
        is_visible: true,
        children_count: 0,
    }
}

// === Tauri Commands ===

#[tauri::command]
pub fn check_admin() -> bool {
    win_util::check_administrator_privileges()
}

#[tauri::command]
pub fn elevate_self() -> Result<(), String> {
    win_util::elevate_self()
}

#[tauri::command]
pub fn get_disk_info(drive: &str) -> Result<DiskInfo, String> {
    let drive_path = format!("{}:\\", drive);
    let (total, free) = win_util::get_disk_space_info(&drive_path)?;
    let used = total.saturating_sub(free);
    let ratio = if total > 0 { used as f32 / total as f32 } else { 0.0 };
    Ok(DiskInfo {
        drive: drive.to_string(),
        total: scanner::format_size(total),
        free: scanner::format_size(free),
        used_percent: ratio,
    })
}

/// 异步扫描C盘根目录，通过事件流式推送进度和结果，避免 UI 卡死
#[tauri::command]
pub async fn scan_disk(app: AppHandle) -> Result<(), String> {
    GLOBAL_ID_COUNTER.store(0, Ordering::SeqCst);

    let app_handle = app.clone();
    // 在后台线程执行耗时的扫描
    tokio::task::spawn_blocking(move || {
        let scan_path = PathBuf::from("C:\\");

        // 通知前端开始扫描
        let _ = app_handle.emit("scan-progress", serde_json::json!({
            "status": "Scanning",
            "detail": "正在扫描C盘根目录...",
            "detail_key": "log.scanRootDetail",
        }));

        let results = scanner::scan_subdirectories(&scan_path, 0);
        let nodes: Vec<TreeNode> = results.iter().map(|entry| {
            let id = GLOBAL_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
            scan_entry_to_tree_node(entry, id)
        }).collect();

        // 推送扫描结果（size=0，先快速展示目录树）
        let _ = app_handle.emit("scan-result", serde_json::json!({
            "status": "Done",
            "nodes": nodes,
            "count": nodes.len(),
        }));

        // 异步计算每个节点的大小，算完逐个推送更新（不阻塞 UI）
        let app_handle2 = app_handle.clone();
        std::thread::spawn(move || {
            for entry in &results {
                if entry.rating != DirectoryRating::Forbidden {
                    let size = scanner::calculate_dir_size(&entry.path, 1);
                    let _ = app_handle2.emit("node-size-update", serde_json::json!({
                        "path": entry.path.to_string_lossy(),
                        "size_bytes": size,
                        "size_text": scanner::format_size(size),
                    }));
                }
            }
        });
    });

    Ok(())
}

/// 异步扫描子目录，通过事件流式推送结果
#[tauri::command]
pub async fn scan_subdirectory(app: AppHandle, path: String, level: i32, parent_id: i32) -> Result<(), String> {
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        let parent_path = PathBuf::from(&path);
        let results = scanner::scan_subdirectories(&parent_path, level);
        let nodes: Vec<TreeNode> = results.iter().map(|entry| {
            let id = GLOBAL_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
            scan_entry_to_tree_node(entry, id)
        }).collect();

        // 推送子目录扫描结果，附带父节点 ID 以便前端插入（size=0，先展示）
        let _ = app_handle.emit("subdir-result", serde_json::json!({
            "parent_id": parent_id,
            "nodes": nodes,
        }));

        // 异步计算每个节点的大小，算完逐个推送更新
        let app_handle2 = app_handle.clone();
        std::thread::spawn(move || {
            for entry in &results {
                if entry.rating != DirectoryRating::Forbidden {
                    let size = scanner::calculate_dir_size(&entry.path, 1);
                    let _ = app_handle2.emit("node-size-update", serde_json::json!({
                        "path": entry.path.to_string_lossy(),
                        "size_bytes": size,
                        "size_text": scanner::format_size(size),
                    }));
                }
            }
        });
    });

    Ok(())
}

/// 检测指定路径的文件占用情况
#[tauri::command]
pub fn check_file_locks(path: String) -> Result<Vec<LockProcess>, String> {
    let path = PathBuf::from(&path);
    let locks = win_util::query_file_locks(&path)?;
    Ok(locks.iter().map(|(pid, name)| LockProcess {
        pid: *pid,
        name: name.clone(),
    }).collect())
}

/// 强制终止占用指定路径的进程
#[tauri::command]
pub fn kill_locking_processes(path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    win_util::force_release_locks(&path)
}

/// target_dir 可以是盘符路径（如 "D:\"），也可以是完整的目标目录路径（如 "D:\CDiskLinker_Moved"）。
/// 迁移后文件将位于 target_dir\目录名 下。
/// fast_mode: true 时跳过目标端 SHA256 校验，仅校验文件大小（更快但不防磁盘静默错误）
#[tauri::command]
pub fn migrate_selected(
    app: AppHandle,
    paths: Vec<String>,
    target_dir: String,
    fast_mode: bool,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("err.noSourceSelected".to_string());
    }

    let app_handle = app.clone();
    std::thread::spawn(move || {
        let total = paths.len();
        // 跟踪是否有任何路径到达了 PendingConfirmation 状态
        // 如果有，不应发出 migration-done（前端由 PendingConfirmation 事件接管状态）
        let mut any_pending_confirmation = false;

        for (idx, src_path_str) in paths.iter().enumerate() {
            let src_path = PathBuf::from(src_path_str);
            let folder_name = src_path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            // 发送当前迁移项信息
            let _ = app_handle.emit("migration-progress", serde_json::json!({
                "stage": "Starting",
                "progress": 0.0,
                "total_files": 0,
                "copied_files": 0,
                "total_size": 0,
                "copied_size": 0,
                "current_file": "",
                "detail": format!("({}/{}) 正在移出 {}...", idx + 1, total, folder_name),
                "detail_key": "log.migratingItem",
                "detail_params": {
                    "current": idx + 1,
                    "total": total,
                    "folder": folder_name,
                },
                "current_item": idx + 1,
                "total_items": total,
                "folder": folder_name,
            }));

            let rating_rule = if scanner::is_forbidden_path(&src_path) {
                DirectoryRating::Forbidden
            } else if scanner::is_warning_path(&src_path) {
                DirectoryRating::Warning
            } else {
                DirectoryRating::Safe
            };

            let entry = ScanEntry {
                path: src_path.clone(),
                name: folder_name.clone(),
                size_in_bytes: scanner::calculate_dir_size(&src_path, 1),
                size_on_disk_formatted: String::new(),
                rating: rating_rule,
                depth: 0,
                expanded: false,
                has_children: false,
            };

            let (tx, rx) = std::sync::mpsc::channel::<MigrationProgress>();
            let (result_tx, result_rx) = std::sync::mpsc::channel();
            let target_dir_clone = target_dir.clone();

            // 使用 8MB 栈空间（默认 2MB 不足以应对深层目录递归拷贝）
            // 0xC00000FD (STATUS_STACK_OVERFLOW) 会在深层目录树递归时触发
            let migration_thread = std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let res = engine::execute_migration(&entry, &target_dir_clone, tx, fast_mode);
                    let _ = result_tx.send(res);
                });

            if let Err(e) = migration_thread {
                let _ = app_handle.emit("migration-error", serde_json::json!({
                    "path": src_path_str,
                    "error": format!("无法启动迁移线程: {}", e),
                }));
                let _ = app_handle.emit("migration-done", serde_json::json!({
                    "status": "Failed",
                    "detail": format!("迁移失败: 无法启动迁移线程: {}", e),
                    "detail_key": "log.migrateFailedThreadStart",
                    "detail_params": { "error": format!("{}", e) },
                }));
                return;
            }

            // 转发结构化进度事件到前端，同时跟踪是否到达 PendingConfirmation
            while let Ok(prog) = rx.recv() {
                // 检测 PendingConfirmation 阶段：迁移完成，等待用户确认
                if prog.stage == "PendingConfirmation" {
                    any_pending_confirmation = true;
                }
                let _ = app_handle.emit("migration-progress", serde_json::to_value(&prog).unwrap_or(serde_json::json!({})));
            }

            let item_result = result_rx.recv().unwrap_or(Err("err.noMigrationResult".to_string()));
            if let Err(e) = item_result {
                let _ = app_handle.emit("migration-error", serde_json::json!({
                    "path": src_path_str,
                    "error": e,
                }));
                let _ = app_handle.emit("migration-done", serde_json::json!({
                    "status": "Failed",
                    "detail": format!("迁移失败: {}", e),
                    "detail_key": "log.migrateFailed",
                    "detail_params": { "error": e },
                }));
                return;
            }

            let _ = app_handle.emit("migration-item-done", serde_json::json!({
                "path": src_path_str,
                "status": "Completed",
            }));
        }

        // 仅当没有任何路径到达 PendingConfirmation 时才发出 migration-done
        // PendingConfirmation 状态下，前端已由该事件接管（显示确认对话框），
        // 此时发出 migration-done 会将 migrationStatus 重置为 Idle，导致：
        // 1. 确认对话框显示的同时按钮可再次点击
        // 2. 用户可能误启第二次迁移，造成 journal 覆盖和状态混乱
        if !any_pending_confirmation {
            let _ = app_handle.emit("migration-done", serde_json::json!({
                "status": "Completed",
                "detail": "全部迁移完成。",
                "detail_key": "log.allDone",
            }));
        }
    });

    Ok(())
}

#[tauri::command]
pub fn rollback_journal(app: AppHandle) -> Result<String, String> {
    let app_handle = app.clone();
    let _ = app_handle.emit("migration-progress", serde_json::json!({
        "stage": "RollingBack",
        "progress": 0.0,
        "detail": "正在执行回滚...",
        "detail_key": "log.rollingBack",
    }));

    match engine::handle_crash_recovery() {
        Ok(Some(msg)) => {
            let _ = app_handle.emit("migration-progress", serde_json::json!({
                "stage": "Idle",
                "progress": 0.0,
                "detail": "",
            }));
            Ok(msg)
        }
        Ok(None) => {
            Ok("log.noPendingJournal".to_string())
        }
        Err(e) => Err(e),
    }
}

/// 用户确认迁移正常，删除旧源目录（_cdisklinker_old）
#[tauri::command]
pub fn confirm_delete_source(path: String) -> Result<(), String> {
    let source_path = PathBuf::from(&path);
    engine::confirm_delete_source(&source_path)
}

/// 从 journal 恢复的 Linked 状态下，确认删除旧源（无参数版本）
///
/// 与 confirm_delete_source 的区别：从 journal 读取 source_path，无需前端传参。
/// 适用于应用重启后 JournalBar 显示的 Linked 状态恢复场景。
#[tauri::command]
pub fn confirm_journal_complete(app: AppHandle) -> Result<String, String> {
    let app_handle = app.clone();
    let _ = app_handle.emit("migration-progress", serde_json::json!({
        "stage": "Copying",
        "progress": 95.0,
        "detail": "正在删除旧源目录...",
        "detail_key": "log.deletingOldSource",
    }));

    // 从 journal 读取 source_path
    let job = crate::journal::read_job()
        .map_err(|e| format!("读取迁移日志失败: {}", e))?
        .ok_or_else(|| "err.noJournalFound".to_string())?;

    let source_path = PathBuf::from(&job.source_path);
    engine::confirm_delete_source(&source_path)?;

    let _ = app_handle.emit("migration-progress", serde_json::json!({
        "stage": "Idle",
        "progress": 100.0,
        "detail": "旧源已删除，迁移完成！",
        "detail_key": "log.oldSourceDeleted",
    }));

    Ok("log.oldSourceFullyDone".to_string())
}

/// 即时回滚迁移（秒级，无需数据拷贝）
/// 适用于 Linked / SourceRenamed / Finalized 状态
#[tauri::command]
pub fn rollback_migration_instant(path: String) -> Result<(), String> {
    let source_path = PathBuf::from(&path);
    engine::rollback_migration_instant(&source_path)
}

#[tauri::command]
pub fn check_crash_recovery() -> Result<CrashRecoveryResult, String> {
    // 单独读取 journal 的 stage（handle_crash_recovery 可能修改 stage，这里读取的是最终状态）
    let stage = crate::journal::read_job()
        .ok()
        .flatten()
        .map(|job| format!("{:?}", job.stage))
        .unwrap_or_default();

    match engine::handle_crash_recovery() {
        Ok(Some(msg)) => {
            // handle_crash_recovery 可能修改了 stage（如 Finalized→Linked 自动恢复），重新读取
            let final_stage = crate::journal::read_job()
                .ok()
                .flatten()
                .map(|job| format!("{:?}", job.stage))
                .unwrap_or(stage);
            Ok(CrashRecoveryResult { found: true, message: msg, stage: final_stage })
        }
        Ok(None) => Ok(CrashRecoveryResult { found: false, message: String::new(), stage: String::new() }),
        Err(e) => Ok(CrashRecoveryResult { found: true, message: format!("自检异常: {}", e), stage }),
    }
}
