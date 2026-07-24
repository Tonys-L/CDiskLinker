// src/ui_bridge.rs - Rust 后端与 Slint UI 的数据桥接
// 此文件展示如何将文档中的核心数据结构映射到 Slint UI

use slint::{ModelRc, VecModel, SharedString};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

// 引入 Slint 生成的 Rust 代码
slint::include_modules!();

use crate::scanner::ScanEntry;
use crate::engine::MigrationStatus;

/// UI 状态管理器
pub struct UiBridge {
    app_window: AppWindow,
    scan_results: Arc<Mutex<Vec<ScanEntry>>>,
}

impl UiBridge {
    pub fn new() -> Self {
        let app_window = AppWindow::new().unwrap();

        // 初始化空数据
        app_window.set_scan_results(ModelRc::new(VecModel::from(vec![])));
        app_window.set_log_entries(ModelRc::new(VecModel::from(vec![])));

        Self {
            app_window,
            scan_results: Arc::new(Mutex::new(vec![])),
        }
    }

    /// 将扫描结果转换为 Slint TreeNode 模型
    pub fn update_scan_results(&self, entries: Vec<ScanEntry>) {
        let mut tree_nodes = vec![];
        let mut id_counter = 0i32;

        // 按路径层级构建树形结构
        for entry in &entries {
            let node = self.entry_to_tree_node(entry, 0, &mut id_counter);
            tree_nodes.push(node);

            // 如果有子目录，递归添加
            // 注意：实际实现中 ScanEntry 需要包含 children 字段
        }

        let model = ModelRc::new(VecModel::from(tree_nodes));
        self.app_window.set_scan_results(model);

        *self.scan_results.lock().unwrap() = entries;
    }

    fn entry_to_tree_node(
        &self, 
        entry: &ScanEntry, 
        level: i32,
        id_counter: &mut i32
    ) -> TreeNode {
        let id = *id_counter;
        *id_counter += 1;

        TreeNode {
            id,
            path: SharedString::from(entry.path.to_string_lossy().as_ref()),
            name: SharedString::from(entry.path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref()),
            size_text: SharedString::from(format_size(entry.actual_size_on_disk)),
            actual_size_bytes: entry.actual_size_on_disk as i32,
            rating: match entry.rating {
                crate::scanner::DirectoryRating::Safe => DirectoryRating::Safe,
                crate::scanner::DirectoryRating::Warning => DirectoryRating::Warning,
                crate::scanner::DirectoryRating::Forbidden => DirectoryRating::Forbidden,
            },
            level,
            is_expanded: level < 1, // 默认展开第一层
            has_children: false,     // 实际根据子目录判断
            is_selected: false,
            is_junction: false,      // 实际检测是否为已有 Junction
            is_visible: level == 0, // 根节点默认可见
            children_count: 0,
        }
    }

    /// 更新迁移状态
    pub fn update_status(&self, status: MigrationStatus, progress: f32, detail: &str) {
        self.app_window.set_current_status(match status {
            MigrationStatus::Idle => ui::MigrationStatus::Idle,
            MigrationStatus::Scanning => ui::MigrationStatus::Scanning,
            MigrationStatus::Initiated => ui::MigrationStatus::Initiated,
            MigrationStatus::Copying => ui::MigrationStatus::Copying,
            MigrationStatus::Validating => ui::MigrationStatus::Validating,
            MigrationStatus::Linking => ui::MigrationStatus::Linking,
            MigrationStatus::Completed => ui::MigrationStatus::Completed,
            MigrationStatus::RollingBack => ui::MigrationStatus::RollingBack,
            MigrationStatus::Error => ui::MigrationStatus::Error,
        });

        self.app_window.set_progress_percent(progress);
        self.app_window.set_progress_detail(SharedString::from(detail));
    }

    /// 添加日志条目
    pub fn add_log(&self, level: &str, message: &str) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();

        let entry = LogEntry {
            timestamp: SharedString::from(timestamp),
            level: SharedString::from(level),
            message: SharedString::from(message),
        };

        // 获取现有日志并追加
        // 实际实现中需要维护一个 VecModel 引用
    }

    /// 设置事务日志状态
    pub fn update_journal(&self, stage: &str, has_pending: bool) {
        self.app_window.set_journal_stage(SharedString::from(stage));
        self.app_window.set_has_pending_journal(has_pending);
    }

    /// 连接 UI 回调到 Rust 后端处理函数
    pub fn setup_callbacks(&self, engine: Arc<Mutex<crate::engine::MigrationEngine>>) {
        let app_weak = self.app_window.as_weak();

        // 扫描按钮
        self.app_window.on_scan_disk({
            let engine = engine.clone();
            move || {
                // 启动后台扫描线程
                std::thread::spawn(move || {
                    // engine.lock().unwrap().scan_and_update();
                });
            }
        });

        // 迁移按钮
        self.app_window.on_migrate_selected({
            let engine = engine.clone();
            move || {
                // 获取选中的节点并执行迁移
            }
        });

        // 展开/折叠节点
        self.app_window.on_toggle_node_expanded({
            let app_weak = app_weak.clone();
            move |node_id| {
                // 更新节点的 is_expanded 和子节点的 is_visible
                if let Some(app) = app_weak.upgrade() {
                    // 遍历模型更新可见性
                }
            }
        });

        // 选择/取消选择节点
        self.app_window.on_toggle_node_selected({
            let app_weak = app_weak.clone();
            move |node_id| {
                // 更新选中状态并重新计算汇总
                if let Some(app) = app_weak.upgrade() {
                    // 检查是否为 Warning 级别，若是则弹出确认对话框
                }
            }
        });

        // 回滚按钮
        self.app_window.on_rollback_journal({
            let engine = engine.clone();
            move || {
                // 执行事务回滚
            }
        });

        // 警告对话框确认
        self.app_window.on_confirm_warning({
            move || {
                // 继续迁移流程
            }
        });
    }

    pub fn run(&self) {
        self.app_window.run().unwrap();
    }
}

/// 格式化文件大小
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_idx])
}
