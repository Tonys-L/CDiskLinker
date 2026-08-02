use std::path::{Path, PathBuf};
use std::fs;
use crate::win_util;

/// 目录的危险评级划分
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DirectoryRating {
    /// 安全：可放心迁移的第三方程序数据
    Safe,
    /// 警告：如 AppData 等配置数据区，迁移后可能造成某些软件配置丢失，需弹窗警示
    Warning,
    /// 禁用：系统核心文件夹或根目录，绝对禁止勾选
    Forbidden,
}

/// 扫描所得的候选目录数据实体
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanEntry {
    pub path: PathBuf,
    pub name: String,
    pub size_in_bytes: u64,
    pub size_on_disk_formatted: String,
    pub rating: DirectoryRating,
    pub depth: i32,
    pub expanded: bool,
    pub has_children: bool,
}

/// 检查路径是否属于绝对禁止的系统黑名单目录
pub fn is_forbidden_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    
    // 1. 绝对禁止对 C 盘根目录自身进行迁移
    if path_str == "c:\\" || path_str == "c:" {
        return true;
    }
    
    // 2. 黑名单物理目录精确匹配或前缀匹配
    let forbidden_list = [
        "c:\\windows",
        "c:\\program files",
        "c:\\program files (x86)",
        "c:\\system volume information",
        "c:\\boot",
        "c:\\recovery",
        "c:\\$recycle.bin", // 回收站
        "c:\\documents and settings"
    ];

    for &forbidden in &forbidden_list {
        if path_str == forbidden || path_str.starts_with(&format!("{}\\", forbidden)) {
            return true;
        }
    }

    false
}

/// 检查路径是否属于需要警告的 AppData 配置文件区
pub fn is_warning_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    path_str.contains("\\appdata")
}

/// 递归计算指定文件夹的实际磁盘大小 (排除已有的软链接/Junction点，防止死循环和大小暴增)
/// 为防止栈溢出，设置最大递归深度为 32 层
pub fn calculate_dir_size(path: &Path, depth: u32) -> u64 {
    if depth > 32 {
        return 0; // 达到安全深度上限，拦截返回
    }

    let mut total_size = 0;

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                let file_type = metadata.file_type();
                
                // 如果是软链接、目录联接（Junction点）或其他重解析点，跳过，不计入大小，不递归
                if file_type.is_symlink() {
                    continue;
                }

                if metadata.is_dir() {
                    // 递归计算子目录大小
                    total_size += calculate_dir_size(&entry.path(), depth + 1);
                } else {
                    total_size += metadata.len();
                }
            }
        }
    }

    total_size
}

/// 格式化字节大小为可读的字符串形式 (如 GB/MB)
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} Bytes", bytes)
    }
}

/// 快速检测一个目录是否含有子目录（不遍历，只读一个且是文件夹即可，速度飞快）
pub fn has_subdirectories(path: &Path) -> bool {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_dir() {
                    // 跳过重解析点
                    if metadata.file_type().is_symlink() {
                        continue;
                    }
                    return true;
                }
            }
        }
    }
    false
}

/// 扫描指定父目录下的第一级子目录，进行安全评级归类，并传入初始深度
///
/// 注意：本函数**不计算目录大小**（size_in_bytes 设为 0），以保证展开速度。
/// 目录大小由调用方（commands.rs）在返回节点后异步计算并推送更新，避免阻塞 UI。
pub fn scan_subdirectories(parent_path: &Path, depth: i32) -> Vec<ScanEntry> {
    let mut results = Vec::new();

    if let Ok(entries) = fs::read_dir(parent_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                // 检测是否为 Junction/重解析点（已迁移的目录）
                // 注意：Windows 上 metadata.is_dir() 对 Junction 返回 false
                // （因为 is_symlink() 为 true 时 is_dir() 返回 false）
                // 所以需要单独检测 is_junction，与 is_dir 取或集
                let is_junction = win_util::is_junction(&path);

                // 普通目录 or Junction 都包含在扫描结果中
                if metadata.is_dir() || is_junction {
                    let name = path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();

                    // 判断评级
                    let rating = if is_forbidden_path(&path) {
                        DirectoryRating::Forbidden
                    } else if is_warning_path(&path) {
                        DirectoryRating::Warning
                    } else {
                        DirectoryRating::Safe
                    };

                    // Junction 不展开；普通目录正常检测子目录
                    let has_children = if is_junction { false } else { has_subdirectories(&path) };

                    results.push(ScanEntry {
                        path,
                        name,
                        size_in_bytes: 0, // 大小由调用方异步计算
                        size_on_disk_formatted: String::new(),
                        rating,
                        depth,
                        expanded: false,
                        has_children,
                    });
                }
            }
        }
    }

    // 按目录名排序（大小异步计算，无法按大小排序）
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    results
}

// === 大目录排行榜 ===

/// 大目录扫描结果条目（推送给前端）
#[derive(Debug, Clone, serde::Serialize)]
pub struct LargeDirEntry {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub size_text: String,
    pub rating: String,
    pub depth: i32,
}

/// 内部目录树节点（自底向上累加大小，只需遍历一次文件系统）
struct DirTreeNode {
    path: PathBuf,
    name: String,
    direct_size: u64,   // 直接子文件大小之和
    total_size: u64,    // 含子目录的总大小
    rating: DirectoryRating,
    depth: i32,
    children: Vec<DirTreeNode>,
}

/// 递归扫描目录树，自底向上累加大小
/// - 跳过 Junction（is_dir 返回 false，不会被误入）
/// - 跳过 Forbidden 目录
/// - depth < max_depth: 递归展开子目录到树中
/// - depth >= max_depth: 不再展开子目录，但用 calculate_dir_size 完整计算大小
///   这样大小计算与目录树一致（完整递归），但目录列表不会过深
fn build_dir_tree(path: &Path, depth: i32, max_depth: i32) -> Option<DirTreeNode> {
    // Forbidden 目录直接跳过
    if is_forbidden_path(path) {
        return None;
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let rating = if is_warning_path(path) {
        DirectoryRating::Warning
    } else {
        DirectoryRating::Safe
    };

    let mut node = DirTreeNode {
        path: path.to_path_buf(),
        name,
        direct_size: 0,
        total_size: 0,
        rating,
        depth,
        children: Vec::new(),
    };

    // 达到最大深度：用 calculate_dir_size 完整计算大小，不再递归展开子目录到树中
    // 关键：大小计算必须完整，否则会出现与目录树大小不一致的问题
    if depth >= max_depth {
        node.total_size = calculate_dir_size(path, 1);
        return Some(node);
    }

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    // 文件：累加到直接大小
                    node.direct_size += metadata.len();
                } else if metadata.is_dir() {
                    // 普通目录：递归扫描（Junction 的 is_dir 返回 false，自动跳过）
                    if let Some(child) = build_dir_tree(&entry.path(), depth + 1, max_depth) {
                        node.children.push(child);
                    }
                }
                // Junction 既不是 file 也不是 dir，自动跳过
            }
        }
    }

    // 自底向上累加总大小
    node.total_size = node.direct_size + node.children.iter().map(|c| c.total_size).sum::<u64>();

    Some(node)
}

/// 从目录树中收集所有目录，返回扁平列表
fn collect_dirs_from_tree(node: &DirTreeNode, result: &mut Vec<LargeDirEntry>) {
    result.push(LargeDirEntry {
        path: node.path.to_string_lossy().into_owned(),
        name: node.name.clone(),
        size_bytes: node.total_size,
        size_text: format_size(node.total_size),
        rating: match node.rating {
            DirectoryRating::Safe => "Safe".to_string(),
            DirectoryRating::Warning => "Warning".to_string(),
            DirectoryRating::Forbidden => "Forbidden".to_string(),
        },
        depth: node.depth,
    });

    for child in &node.children {
        collect_dirs_from_tree(child, result);
    }
}

/// 扫描 C 盘大目录，返回按大小降序排列的 Top N 目录
/// 高效实现：递归构建目录树，自底向上累加大小，只遍历一次文件系统
pub fn scan_large_directories(root: &Path, max_depth: i32, top_n: usize) -> Vec<LargeDirEntry> {
    let mut all_dirs = Vec::new();

    // 扫描根目录下的子目录
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                // 只处理普通目录（Junction 的 is_dir 返回 false，会被跳过）
                if metadata.is_dir() {
                    if let Some(tree) = build_dir_tree(&path, 1, max_depth) {
                        collect_dirs_from_tree(&tree, &mut all_dirs);
                    }
                }
            }
        }
    }

    // 按大小降序排序
    all_dirs.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    // 取 Top N
    all_dirs.into_iter().take(top_n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_forbidden_and_warning_paths() {
        // 验证系统关键目录的绝对拦截判定
        assert!(is_forbidden_path(Path::new("C:\\Windows")));
        assert!(is_forbidden_path(Path::new("c:\\windows\\system32")));
        assert!(is_forbidden_path(Path::new("c:\\Program Files (x86)\\Common Files")));
        assert!(is_forbidden_path(Path::new("C:\\")));

        // 验证非系统关键目录的正常放行
        assert!(!is_forbidden_path(Path::new("C:\\Users\\Tony\\Downloads")));

        // 验证 AppData 危险警示区域的特征提取
        assert!(is_warning_path(Path::new("C:\\Users\\Tony\\AppData\\Local")));
        assert!(!is_warning_path(Path::new("C:\\Users\\Tony\\Desktop")));
    }
}
