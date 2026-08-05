// 迁移档案管理模块
//
// 设计目标：迁移完成后在目标目录写入自包含指纹文件 (.cdisklinker_meta.json)，
// 并在软件运行目录维护全局索引 (migration_history.json)，让软件能识别自己迁移过的目录，
// 并提供软件内一键恢复入口。
//
// 双副本设计：
// - 自包含档案（目标目录/.cdisklinker_meta.json）：跟随数据走，身份校验权威
// - 全局索引（软件目录/migration_history.json）：快速查询
//
// 一致性原则：以自包含档案为权威，全局索引可重建。
// 写入顺序：先写自包含档案 → 再更新全局索引（顺序不可反）。
//
// 误删处理：
// - 误删自包含档案：list_archives 返回 meta_file_exists: false，UI 警告；可 rebuild 重建
// - 误删全局索引：本次不实现自动重建（需扫描所有盘符），作为后续扩展点

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::journal;

/// 自包含档案文件名（写入目标目录根下）
const META_FILE_NAME: &str = ".cdisklinker_meta.json";

/// 全局索引文件名（写入软件运行目录，与 pending_jobs.json 同目录）
const HISTORY_FILE_NAME: &str = "migration_history.json";

/// 档案格式版本
const ARCHIVE_VERSION: u32 = 1;

/// 迁移档案：记录一次成功迁移的元数据
///
/// 双副本存储：
/// 1. 自包含档案：写入目标目录根下的 `.cdisklinker_meta.json`（隐藏属性）
/// 2. 全局索引：聚合到软件运行目录的 `migration_history.json`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MigrationArchive {
    /// 档案格式版本，当前 = 1
    pub version: u32,
    /// 档案唯一 ID（UUID v4 格式）
    pub archive_id: String,
    /// 原路径（C 盘 Junction 所在路径，如 "C:\\Steam"）
    pub source_path: String,
    /// 真实数据路径（D 盘实际数据位置，如 "D:\\Games\\Steam"）
    pub target_path: String,
    /// 迁移完成时间戳（Unix 秒）
    pub created_at: u64,
    /// 迁移完成时 Manifest 的 self_hash（用于校验目标数据未被替换）
    /// 若迁移时 Manifest 已删除，此字段为空字符串（降级，无法验证数据身份）
    pub manifest_self_hash: String,
    /// 文件总数（冗余字段，快速校验）
    pub total_files: usize,
    /// 总大小（冗余字段，快速校验）
    pub total_size: u64,
    /// 创建档案时的软件版本
    pub software_version: String,
    /// 档案自身的哈希（防篡改，基于不含本字段的内容计算）
    /// 读取时若不匹配则视为档案被篡改或损坏
    #[serde(default)]
    pub archive_self_hash: String,
}

impl MigrationArchive {
    /// 计算自身内容哈希（不含 archive_self_hash 字段）
    ///
    /// 复用 Manifest self_hash 的设计：克隆自身 → 清空 archive_self_hash → 序列化 → SHA256
    fn compute_self_hash(&self) -> String {
        let mut copy = self.clone();
        copy.archive_self_hash = String::new();
        let json = serde_json::to_string(&copy).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// 填充 archive_self_hash 字段（持久化前调用）
    fn fill_self_hash(&mut self) {
        self.archive_self_hash = self.compute_self_hash();
    }

    /// 校验 archive_self_hash 是否匹配
    ///
    /// 匹配返回 Ok(())，不匹配返回 Err
    /// 空字符串的 archive_self_hash 视为"未设置"，返回 Ok（降级场景）
    pub fn verify_self_hash(&self) -> Result<(), String> {
        if self.archive_self_hash.is_empty() {
            return Ok(());
        }
        let expected = self.compute_self_hash();
        if self.archive_self_hash != expected {
            return Err(format!(
                "档案自哈希不匹配（可能被篡改或损坏）。期望 {}，实际 {}",
                expected, self.archive_self_hash
            ));
        }
        Ok(())
    }

    /// 持久化到目标目录下的 `.cdisklinker_meta.json`
    ///
    /// 调用前需已 fill_self_hash。写入后设置隐藏属性。
    pub fn save_to_target_dir(&self, target_dir: &Path) -> Result<PathBuf, String> {
        let meta_path = target_dir.join(META_FILE_NAME);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("档案序列化失败: {}", e))?;
        fs::write(&meta_path, json.as_bytes())
            .map_err(|e| format!("写入档案文件失败 {:?}: {}", meta_path, e))?;

        // 设置隐藏属性，避免在资源管理器中暴露
        let _ = set_file_hidden(&meta_path);

        Ok(meta_path)
    }

    /// 从目标目录读取自包含档案并校验自哈希
    ///
    /// 若文件不存在返回 Err；若自哈希不匹配返回 Err
    pub fn load_from_target_dir(target_dir: &Path) -> Result<Self, String> {
        let meta_path = target_dir.join(META_FILE_NAME);
        let data = fs::read_to_string(&meta_path)
            .map_err(|e| format!("读取档案文件失败 {:?}: {}", meta_path, e))?;
        let archive: MigrationArchive = serde_json::from_str(&data)
            .map_err(|e| format!("解析档案失败: {}", e))?;
        archive.verify_self_hash()?;
        Ok(archive)
    }
}

/// 检查目标目录是否存在自包含档案文件
pub fn meta_file_exists(target_dir: &Path) -> bool {
    target_dir.join(META_FILE_NAME).exists()
}

/// 全局索引：聚合所有迁移档案
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MigrationHistory {
    pub archives: Vec<MigrationArchive>,
    pub updated_at: u64,
}

/// 列表项：含档案本身 + 目标目录自包含档案的存在性标记
///
/// meta_file_exists = false 表示用户误删了自包含档案，UI 应警告
/// junction_exists = false 表示源位置的 Junction 已丢失，UI 应显示"重建链接"按钮
#[derive(Serialize, Debug, Clone)]
pub struct ArchiveListItem {
    #[serde(flatten)]
    pub archive: MigrationArchive,
    /// 目标目录的 .cdisklinker_meta.json 是否存在
    pub meta_file_exists: bool,
    /// 源路径是否仍为 Junction（false 表示链接丢失，可重建）
    #[serde(default)]
    pub junction_exists: bool,
}

/// 获取全局索引文件路径（软件运行目录）
///
/// 测试时可通过 set_test_history_dir 注入临时目录，避免污染真实运行目录
fn get_history_path() -> Result<PathBuf, String> {
    #[cfg(test)]
    {
        if let Some(dir) = TEST_HISTORY_DIR.with(|d| d.borrow().clone()) {
            return Ok(dir.join(HISTORY_FILE_NAME));
        }
    }
    let dir = journal::get_journal_dir()?;
    Ok(dir.join(HISTORY_FILE_NAME))
}

#[cfg(test)]
thread_local! {
    static TEST_HISTORY_DIR: std::cell::RefCell<Option<PathBuf>> = std::cell::RefCell::new(None);
}

/// 测试专用：注入全局索引目录（None 表示使用真实 journal 目录）
#[cfg(test)]
pub fn set_test_history_dir(dir: Option<PathBuf>) {
    TEST_HISTORY_DIR.with(|d| *d.borrow_mut() = dir);
}

/// 读取全局索引（文件不存在时返回空索引）
fn load_history() -> Result<MigrationHistory, String> {
    let path = get_history_path()?;
    if !path.exists() {
        return Ok(MigrationHistory::default());
    }
    let data = fs::read_to_string(&path)
        .map_err(|e| format!("读取全局索引失败 {:?}: {}", path, e))?;
    let history: MigrationHistory = serde_json::from_str(&data)
        .map_err(|e| format!("解析全局索引失败: {}", e))?;
    Ok(history)
}

/// 保存全局索引
fn save_history(history: &MigrationHistory) -> Result<(), String> {
    let path = get_history_path()?;
    let json = serde_json::to_string_pretty(history)
        .map_err(|e| format!("全局索引序列化失败: {}", e))?;
    fs::write(&path, json.as_bytes())
        .map_err(|e| format!("写入全局索引失败 {:?}: {}", path, e))?;
    Ok(())
}

/// 列出全部迁移档案（含目标目录自包含档案的存在性标记 + 源 Junction 状态）
///
/// 用于 UI 展示已迁移列表。
/// - meta_file_exists = false 表示用户误删了自包含档案
/// - junction_exists = false 表示源位置的 Junction 已丢失，UI 可提供"重建链接"
pub fn list_archives() -> Result<Vec<ArchiveListItem>, String> {
    let history = load_history()?;
    let mut items = Vec::with_capacity(history.archives.len());
    for archive in history.archives {
        let meta_file_exists = Path::new(&archive.target_path).exists()
            && meta_file_exists(Path::new(&archive.target_path));
        let junction_exists = crate::win_util::is_junction(Path::new(&archive.source_path));
        items.push(ArchiveListItem {
            archive,
            meta_file_exists,
            junction_exists,
        });
    }
    Ok(items)
}

/// 按源路径查询档案
///
/// 用于扫描时识别"这个 Junction 是不是本软件创建的"
pub fn find_by_source_path(source_path: &Path) -> Option<MigrationArchive> {
    let history = load_history().ok()?;
    let source_str = source_path.to_string_lossy().to_lowercase();
    history
        .archives
        .into_iter()
        .find(|a| a.source_path.to_lowercase() == source_str)
}

/// 写入迁移档案（双副本同步）
///
/// 顺序：先写目标目录自包含档案 → 再更新全局索引
/// 失败处理：任一步失败立即返回 Err，不回滚已写入的部分（调用方 best_effort 处理）
pub fn write_archive(mut archive: MigrationArchive) -> Result<(), String> {
    archive.fill_self_hash();

    // 1. 写目标目录自包含档案
    let target_dir = Path::new(&archive.target_path);
    if !target_dir.exists() {
        return Err(format!(
            "目标目录不存在，无法写入档案: {:?}",
            archive.target_path
        ));
    }
    archive.save_to_target_dir(target_dir)?;

    // 2. 更新全局索引
    let mut history = load_history()?;
    // 移除同 archive_id 的旧记录（幂等）
    history.archives.retain(|a| a.archive_id != archive.archive_id);
    history.archives.push(archive);
    history.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    save_history(&history)?;

    Ok(())
}

/// 移除迁移档案（双副本同步）
///
/// 顺序：先删全局索引中的记录 → 再删目标目录自包含档案
/// 用于恢复完成后的清理
pub fn remove_archive(archive_id: &str) -> Result<(), String> {
    // 1. 从全局索引移除
    let mut history = load_history()?;
    let removed = history.archives.iter()
        .find(|a| a.archive_id == archive_id)
        .cloned();
    if let Some(archive) = removed {
        history.archives.retain(|a| a.archive_id != archive_id);
        history.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        save_history(&history)?;

        // 2. 删目标目录自包含档案（best_effort，文件不存在不算错）
        let meta_path = Path::new(&archive.target_path).join(META_FILE_NAME);
        if meta_path.exists() {
            let _ = fs::remove_file(&meta_path);
        }
    }
    Ok(())
}

/// 校验档案完整性（用于恢复前的身份验证）
///
/// 校验项：
/// 1. 全局索引中存在该 archive_id
/// 2. 目标目录存在
/// 3. 目标目录的自包含档案存在
/// 4. 自包含档案的 archive_self_hash 匹配（防篡改）
/// 5. 自包含档案与全局索引记录一致（archive_id / source_path / target_path 三者匹配）
pub fn verify_archive(archive_id: &str) -> Result<MigrationArchive, String> {
    let history = load_history()?;
    let index_archive = history
        .archives
        .iter()
        .find(|a| a.archive_id == archive_id)
        .ok_or_else(|| format!("全局索引中不存在档案: {}", archive_id))?
        .clone();

    let target_dir = Path::new(&index_archive.target_path);
    if !target_dir.exists() {
        return Err(format!(
            "目标目录不存在: {:?}",
            index_archive.target_path
        ));
    }

    let meta_archive = MigrationArchive::load_from_target_dir(target_dir)?;

    // 自包含档案与全局索引一致性校验
    if meta_archive.archive_id != index_archive.archive_id {
        return Err(format!(
            "档案 ID 不一致：全局索引 {}，自包含档案 {}",
            index_archive.archive_id, meta_archive.archive_id
        ));
    }
    if meta_archive.source_path != index_archive.source_path {
        return Err(format!(
            "源路径不一致：全局索引 {:?}，自包含档案 {:?}",
            index_archive.source_path, meta_archive.source_path
        ));
    }
    if meta_archive.target_path != index_archive.target_path {
        return Err(format!(
            "目标路径不一致：全局索引 {:?}，自包含档案 {:?}",
            index_archive.target_path, meta_archive.target_path
        ));
    }

    Ok(meta_archive)
}

/// 从全局索引重建目标目录的自包含档案
///
/// 用于用户误删自包含档案但全局索引仍存在的场景。
/// 重建后自包含档案的 archive_self_hash 与全局索引一致。
pub fn rebuild_meta_from_index(archive_id: &str) -> Result<(), String> {
    let history = load_history()?;
    let archive = history
        .archives
        .iter()
        .find(|a| a.archive_id == archive_id)
        .ok_or_else(|| format!("全局索引中不存在档案: {}", archive_id))?
        .clone();

    let target_dir = Path::new(&archive.target_path);
    if !target_dir.exists() {
        return Err(format!(
            "目标目录不存在，无法重建档案: {:?}",
            archive.target_path
        ));
    }

    archive.save_to_target_dir(target_dir)?;
    Ok(())
}

/// 重建源位置的 Junction（链接丢失恢复）
///
/// 适用场景：用户误删了 Junction，但目标目录数据还在，希望恢复链接。
///
/// 前置条件：
/// 1. 档案存在且通过完整性校验
/// 2. 目标目录存在（数据还在）
/// 3. 源路径不存在（Junction 已丢失）—— 不覆盖已存在的目录/Junction
///
/// 成功后：源路径重新成为指向目标目录的 Junction
pub fn rebuild_junction(archive_id: &str) -> Result<(), String> {
    let archive = verify_archive(archive_id)?;

    let target_dir = Path::new(&archive.target_path);
    if !target_dir.exists() {
        return Err(format!(
            "目标目录不存在，无法重建链接: {:?}",
            archive.target_path
        ));
    }

    let source_path = Path::new(&archive.source_path);
    if source_path.exists() {
        return Err(format!(
            "源路径已存在，不可覆盖（可能是 Junction 仍存在或出现了同名目录）: {:?}",
            archive.source_path
        ));
    }

    crate::win_util::create_junction(source_path, target_dir)?;
    Ok(())
}

/// 设置文件为隐藏属性（Windows）
///
/// 失败时静默忽略（隐藏属性是体验优化，非功能必需）
fn set_file_hidden(path: &Path) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_FLAGS_AND_ATTRIBUTES,
    };

    let path_h = HSTRING::from(path.as_os_str());

    unsafe {
        let attrs = GetFileAttributesW(&path_h);
        // INVALID_FILE_ATTRIBUTES = 0xFFFFFFFF
        if attrs == 0xFFFFFFFF {
            return Ok(());
        }
        let new_attrs = FILE_FLAGS_AND_ATTRIBUTES(attrs | FILE_ATTRIBUTE_HIDDEN.0);
        let _ = SetFileAttributesW(&path_h, new_attrs);
    }
    Ok(())
}

// ============================================================================
// 单元测试
// ============================================================================
//
// 测试覆盖目标：
//   - MigrationArchive 序列化/反序列化
//   - archive_self_hash 自哈希计算与篡改检测
//   - save_to_target_dir / load_from_target_dir
//   - write_archive 双副本同步
//   - list_archives（含 meta_file_exists 标记）
//   - find_by_source_path
//   - remove_archive 双向清除
//   - verify_archive 完整性校验
//   - rebuild_meta_from_index 误删重建
//
// 测试隔离：
//   - 所有文件操作在 std::env::temp_dir() 下唯一子目录中进行
//   - 全局索引文件路径通过 journal::get_journal_dir() 获取（软件运行目录）
//   - 测试可能污染真实运行目录，但因为是测试环境（开发机），可接受
//   - 每个测试用唯一的 archive_id 避免互相干扰
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// 构造唯一测试根目录
    fn make_test_root(test_name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join("cdisklinker_history_tests")
            .join(format!("{}_{}", test_name, rand_simple()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// 简单伪随机数（与 engine.rs 的 rand_simple 一致）
    fn rand_simple() -> u32 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        (start & 0xFFFFFFFF) as u32
    }

    /// 构造测试用档案
    fn make_test_archive(source: &str, target: &str, archive_id: &str) -> MigrationArchive {
        MigrationArchive {
            version: ARCHIVE_VERSION,
            archive_id: archive_id.to_string(),
            source_path: source.to_string(),
            target_path: target.to_string(),
            created_at: 1700000000,
            manifest_self_hash: "abc123".to_string(),
            total_files: 100,
            total_size: 1024 * 1024 * 100,
            software_version: "1.5.0".to_string(),
            archive_self_hash: String::new(),
        }
    }

    // ===== T1: MigrationArchive 序列化/反序列化 =====
    #[test]
    fn test_archive_serialize_deserialize() {
        let archive = make_test_archive("C:\\Test", "D:\\Test", "id-1");
        let json = serde_json::to_string(&archive).unwrap();
        let deserialized: MigrationArchive = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.version, archive.version);
        assert_eq!(deserialized.archive_id, archive.archive_id);
        assert_eq!(deserialized.source_path, archive.source_path);
        assert_eq!(deserialized.target_path, archive.target_path);
        assert_eq!(deserialized.manifest_self_hash, archive.manifest_self_hash);
        assert_eq!(deserialized.total_files, archive.total_files);
        assert_eq!(deserialized.total_size, archive.total_size);
    }

    // ===== T2: archive_self_hash 篡改检测 =====
    #[test]
    fn test_archive_self_hash_tamper_detection() {
        let root = make_test_root("self_hash_tamper");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let mut archive = make_test_archive("C:\\Test", target_dir.to_str().unwrap(), "id-tamper");
        archive.fill_self_hash();

        // 保存并重新加载，自哈希应匹配
        let meta_path = archive.save_to_target_dir(&target_dir).unwrap();
        let loaded = MigrationArchive::load_from_target_dir(&target_dir).unwrap();
        assert_eq!(loaded.archive_self_hash, archive.archive_self_hash);
        assert!(loaded.verify_self_hash().is_ok());

        // 篡改档案内容（修改 source_path 但保留原 archive_self_hash）
        let content = fs::read_to_string(&meta_path).unwrap();
        let tampered = content.replace("C:\\\\Test", "C:\\\\Tampered");
        fs::write(&meta_path, tampered).unwrap();

        // 重新加载应失败（自哈希不匹配）
        let result = MigrationArchive::load_from_target_dir(&target_dir);
        assert!(result.is_err(), "篡改后加载应失败");
        let err = result.unwrap_err();
        assert!(
            err.contains("自哈希不匹配") || err.contains("篡改"),
            "错误信息应提示自哈希不匹配: {}",
            err
        );

        let _ = fs::remove_dir_all(&root);
    }

    // ===== T3: archive_self_hash 空字符串时跳过校验（降级场景） =====
    #[test]
    fn test_archive_self_hash_empty_skips_verification() {
        let archive = make_test_archive("C:\\Test", "D:\\Test", "id-empty");
        // 不调用 fill_self_hash，archive_self_hash 为空
        assert!(archive.archive_self_hash.is_empty());
        // 空字符串时 verify_self_hash 应返回 Ok（降级场景）
        assert!(archive.verify_self_hash().is_ok());
    }

    // ========================================================================
    // 以下测试使用 set_test_history_dir 注入临时目录，避免污染真实 journal 目录
    // ========================================================================

    /// 设置测试用全局索引目录，返回该目录
    fn setup_test_history_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("cdisklinker_history_index")
            .join(format!("{}_{}", test_name, rand_simple()));
        fs::create_dir_all(&dir).unwrap();
        set_test_history_dir(Some(dir.clone()));
        dir
    }

    /// 清理测试用全局索引目录
    fn teardown_test_history_dir() {
        set_test_history_dir(None);
    }

    // ===== T4: write_archive 写入目标目录元文件 + 同步全局索引 =====
    #[test]
    fn test_write_archive_creates_meta_file_and_updates_index() {
        let _history_dir = setup_test_history_dir("write_archive");
        let root = make_test_root("write_archive");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-write-1",
        );

        // 写入前目标目录无元文件
        assert!(!meta_file_exists(&target_dir));

        write_archive(archive.clone()).unwrap();

        // 1. 目标目录元文件存在
        assert!(meta_file_exists(&target_dir));

        // 2. 元文件可被 load_from_target_dir 正确读取
        let loaded = MigrationArchive::load_from_target_dir(&target_dir).unwrap();
        assert_eq!(loaded.archive_id, "id-write-1");
        assert_eq!(loaded.source_path, "C:\\Steam");

        // 3. 全局索引包含该档案
        let items = list_archives().unwrap();
        let found = items.iter().find(|i| i.archive.archive_id == "id-write-1");
        assert!(found.is_some(), "全局索引应包含刚写入的档案");
        assert!(found.unwrap().meta_file_exists, "meta_file_exists 应为 true");

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T5: write_archive 幂等性 - 同 archive_id 重复写入不产生多条 =====
    #[test]
    fn test_write_archive_idempotent() {
        let _history_dir = setup_test_history_dir("idempotent");
        let root = make_test_root("idempotent");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let mut archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-idempotent",
        );

        write_archive(archive.clone()).unwrap();
        // 修改 total_files 模拟再次写入（同 archive_id）
        archive.total_files = 200;
        write_archive(archive.clone()).unwrap();

        let items = list_archives().unwrap();
        let count = items.iter().filter(|i| i.archive.archive_id == "id-idempotent").count();
        assert_eq!(count, 1, "同 archive_id 重复写入应只保留 1 条");

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T6: list_archives 误删自包含档案时返回 meta_file_exists=false =====
    #[test]
    fn test_list_archives_meta_file_missing() {
        let _history_dir = setup_test_history_dir("meta_missing");
        let root = make_test_root("meta_missing");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-missing",
        );
        write_archive(archive).unwrap();

        // 模拟用户误删自包含档案
        let meta_path = target_dir.join(META_FILE_NAME);
        assert!(meta_path.exists());
        fs::remove_file(&meta_path).unwrap();

        // list_archives 应返回 meta_file_exists=false
        let items = list_archives().unwrap();
        let found = items.iter().find(|i| i.archive.archive_id == "id-missing");
        assert!(found.is_some());
        assert!(!found.unwrap().meta_file_exists, "误删后 meta_file_exists 应为 false");

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T6b: list_archives 返回 junction_exists 字段（source 不存在时为 false） =====
    #[test]
    fn test_list_archives_junction_exists_field() {
        let _history_dir = setup_test_history_dir("junction_exists");
        let root = make_test_root("junction_exists");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        // source_path 是一个不存在的路径（非 Junction）
        let archive = make_test_archive(
            "C:\\DefinitelyNotExists_abc123",
            target_dir.to_str().unwrap(),
            "id-junc-1",
        );
        write_archive(archive).unwrap();

        let items = list_archives().unwrap();
        let found = items.iter().find(|i| i.archive.archive_id == "id-junc-1");
        assert!(found.is_some());
        // source_path 不存在 → junction_exists 应为 false
        assert!(
            !found.unwrap().junction_exists,
            "source_path 不存在时 junction_exists 应为 false"
        );

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T7: find_by_source_path 按源路径查询（大小写不敏感） =====
    #[test]
    fn test_find_by_source_path_case_insensitive() {
        let _history_dir = setup_test_history_dir("find_by_source");
        let root = make_test_root("find_by_source");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-find",
        );
        write_archive(archive).unwrap();

        // 大写查询
        let found = find_by_source_path(std::path::Path::new("C:\\Steam"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().archive_id, "id-find");

        // 小写查询（大小写不敏感）
        let found = find_by_source_path(std::path::Path::new("c:\\steam"));
        assert!(found.is_some());

        // 不存在的路径
        let not_found = find_by_source_path(std::path::Path::new("C:\\NotExists"));
        assert!(not_found.is_none());

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T8: remove_archive 双向清除 =====
    #[test]
    fn test_remove_archive_clears_both() {
        let _history_dir = setup_test_history_dir("remove");
        let root = make_test_root("remove");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-remove",
        );
        write_archive(archive).unwrap();
        assert!(meta_file_exists(&target_dir));

        remove_archive("id-remove").unwrap();

        // 1. 全局索引不再包含
        let items = list_archives().unwrap();
        let found = items.iter().find(|i| i.archive.archive_id == "id-remove");
        assert!(found.is_none(), "移除后全局索引不应包含该档案");

        // 2. 目标目录元文件已删除
        assert!(!meta_file_exists(&target_dir), "移除后目标元文件应被删除");

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T9: remove_archive 移除不存在的 archive_id 不报错 =====
    #[test]
    fn test_remove_archive_nonexistent_ok() {
        let _history_dir = setup_test_history_dir("remove_nonexistent");
        // 移除不存在的 archive_id 应返回 Ok
        let result = remove_archive("id-not-exist");
        assert!(result.is_ok());
        teardown_test_history_dir();
    }

    // ===== T10: verify_archive 正常情况返回 Ok =====
    #[test]
    fn test_verify_archive_ok() {
        let _history_dir = setup_test_history_dir("verify_ok");
        let root = make_test_root("verify_ok");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-verify",
        );
        write_archive(archive).unwrap();

        let verified = verify_archive("id-verify").unwrap();
        assert_eq!(verified.archive_id, "id-verify");
        assert_eq!(verified.source_path, "C:\\Steam");

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T11: verify_archive 目标目录不存在返回 Err =====
    #[test]
    fn test_verify_archive_target_missing() {
        let _history_dir = setup_test_history_dir("verify_target_missing");
        let root = make_test_root("verify_target_missing");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-verify-missing",
        );
        write_archive(archive).unwrap();

        // 删除目标目录
        fs::remove_dir_all(&target_dir).unwrap();

        let result = verify_archive("id-verify-missing");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("目标目录不存在"));

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T12: verify_archive 自包含档案被删返回 Err =====
    #[test]
    fn test_verify_archive_meta_deleted() {
        let _history_dir = setup_test_history_dir("verify_meta_deleted");
        let root = make_test_root("verify_meta_deleted");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-verify-meta-deleted",
        );
        write_archive(archive).unwrap();

        // 删除自包含档案
        fs::remove_file(target_dir.join(META_FILE_NAME)).unwrap();

        let result = verify_archive("id-verify-meta-deleted");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("读取档案文件失败"));

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T13: verify_archive 自包含档案被篡改返回 Err =====
    #[test]
    fn test_verify_archive_tampered() {
        let _history_dir = setup_test_history_dir("verify_tampered");
        let root = make_test_root("verify_tampered");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-verify-tampered",
        );
        write_archive(archive).unwrap();

        // 篡改自包含档案
        let meta_path = target_dir.join(META_FILE_NAME);
        let content = fs::read_to_string(&meta_path).unwrap();
        let tampered = content.replace("C:\\\\Steam", "C:\\\\Tampered");
        fs::write(&meta_path, tampered).unwrap();

        let result = verify_archive("id-verify-tampered");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("自哈希不匹配"));

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T14: rebuild_meta_from_index 误删后重建 =====
    #[test]
    fn test_rebuild_meta_from_index() {
        let _history_dir = setup_test_history_dir("rebuild");
        let root = make_test_root("rebuild");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\Steam",
            target_dir.to_str().unwrap(),
            "id-rebuild",
        );
        write_archive(archive).unwrap();

        // 模拟用户误删自包含档案
        fs::remove_file(target_dir.join(META_FILE_NAME)).unwrap();
        assert!(!meta_file_exists(&target_dir));

        // 此时 verify_archive 应失败
        assert!(verify_archive("id-rebuild").is_err());

        // 从全局索引重建
        rebuild_meta_from_index("id-rebuild").unwrap();

        // 重建后元文件存在
        assert!(meta_file_exists(&target_dir));

        // 重建后 verify_archive 应通过
        let verified = verify_archive("id-rebuild").unwrap();
        assert_eq!(verified.archive_id, "id-rebuild");

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T15: rebuild_meta_from_index 不存在的 archive_id 返回 Err =====
    #[test]
    fn test_rebuild_meta_nonexistent() {
        let _history_dir = setup_test_history_dir("rebuild_nonexistent");
        let result = rebuild_meta_from_index("id-not-exist");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("全局索引中不存在档案"));
        teardown_test_history_dir();
    }

    // ===== T16: rebuild_junction - archive_id 不存在时报错 =====
    #[test]
    fn test_rebuild_junction_nonexistent() {
        let _history_dir = setup_test_history_dir("rebuild_junc_nonexist");
        let result = rebuild_junction("id-not-exist");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("全局索引中不存在档案"));
        teardown_test_history_dir();
    }

    // ===== T17: rebuild_junction - 目标目录不存在时报错 =====
    #[test]
    fn test_rebuild_junction_target_missing() {
        let _history_dir = setup_test_history_dir("rebuild_junc_target_missing");
        let root = make_test_root("rebuild_junc_target_missing");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        let archive = make_test_archive(
            "C:\\DefinitelyNotExists_xyz789",
            target_dir.to_str().unwrap(),
            "id-junc-target-missing",
        );
        write_archive(archive).unwrap();

        // 模拟目标目录被删除（数据丢失）
        fs::remove_dir_all(&target_dir).unwrap();

        let result = rebuild_junction("id-junc-target-missing");
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("目标目录不存在"),
            "目标目录不存在时应报错"
        );

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }

    // ===== T18: rebuild_junction - 源路径已存在普通目录时报错（不覆盖） =====
    #[test]
    fn test_rebuild_junction_source_exists_as_dir() {
        let _history_dir = setup_test_history_dir("rebuild_junc_source_dir");
        let root = make_test_root("rebuild_junc_source_dir");
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();

        // 源路径指向一个已存在的普通目录（非 Junction）
        let source_dir = root.join("fake_source");
        fs::create_dir_all(&source_dir).unwrap();

        let archive = make_test_archive(
            source_dir.to_str().unwrap(),
            target_dir.to_str().unwrap(),
            "id-junc-source-dir",
        );
        write_archive(archive).unwrap();

        let result = rebuild_junction("id-junc-source-dir");
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("已存在"),
            "源路径已存在普通目录时应报错，避免覆盖"
        );

        teardown_test_history_dir();
        let _ = fs::remove_dir_all(&root);
    }
}
