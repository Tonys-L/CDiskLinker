use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use std::fs::File;
use std::io::{Write, Read};

/// 迁移流程的核心原子状态阶段
///
/// 状态机细化原则：每个里程碑对应一次日志写入，崩溃后依据"日志状态 + 文件系统实际状态"
/// 推导恢复动作。核心安全约束：源删除前 tmp 是冗余副本（可删）；源删除后 tmp/final 是
/// 唯一副本（绝不可删）。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum MigrationStage {
    /// 第一阶段：日志已建立，物理拷贝准备开始或拷贝进行中
    /// 崩溃恢复：源完整 → 删 tmp；源不在 → 异常，保留 tmp 提示用户
    Initiated,
    /// 第二阶段：物理拷贝完成且 Hash/数量/大小校验通过，源目录仍完好，tmp 完整
    /// 崩溃恢复：源在 → 删 tmp 保源；源不在 → 保留 tmp 提示用户
    Copied,
    /// 第三阶段：源目录已安全删除，tmp 仍是唯一完整副本（rename 未做）
    /// 崩溃恢复：tmp 在 → 自动 rename + 建 Junction 完成迁移；tmp 不在 → 数据丢失报错
    SourceDeleted,
    /// 第四阶段：tmp 已重命名为 final，数据在 final，但 Junction 未建立
    /// 崩溃恢复：final 在 → 自动建 Junction 完成迁移；final 不在 → 异常报错
    Renamed,
    /// 第五阶段：原 C 盘路径已建立 Junction 重解析点，迁移实质完成
    /// 崩溃恢复：清理日志即可
    Linked,
}

/// 事务日志结构体，包含本次迁移任务的一切核心源和目标路径元数据
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PendingJob {
    pub job_id: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,      // 指向目标 D 盘的临时路径，如 D:\Games\.tmp_Steam
    pub final_target_path: PathBuf,// 指向最终的真实目标路径，如 D:\Games\Steam
    pub stage: MigrationStage,
    /// 源端 Manifest 文件路径（生成于删除源之前，用于崩溃恢复时校验目标完整性）
    /// 旧版日志无此字段，serde 默认 None 兼容
    #[serde(default)]
    pub manifest_path: Option<PathBuf>,
}

/// 获取日志文件在可执行程序同目录下的绝对路径
fn get_journal_path() -> Result<PathBuf, String> {
    let dir = get_journal_dir()?;
    Ok(dir.join("pending_jobs.json"))
}

/// 获取日志/Manifest 文件所在目录（可执行程序同目录）
pub fn get_journal_dir() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("获取当前可执行文件位置失败: {}", e))?;
    let exe_dir = current_exe.parent()
        .ok_or_else(|| "无法获取可执行文件父目录".to_string())?;
    Ok(exe_dir.to_path_buf())
}

/// 写入或更新未决的迁移任务状态至本地的事务日志文件
pub fn write_job(job: &PendingJob) -> Result<(), String> {
    let log_path = get_journal_path()?;
    
    // 序列化成 JSON 字符串
    let data = serde_json::to_string_pretty(job)
        .map_err(|e| format!("事务日志序列化 JSON 失败: {}", e))?;
    
    // 强制写入并执行 flush 落盘
    let mut file = File::create(&log_path)
        .map_err(|e| format!("无法创建事务日志文件 {:?}: {}", log_path, e))?;
    file.write_all(data.as_bytes())
        .map_err(|e| format!("写入事务日志失败: {}", e))?;
    file.flush()
        .map_err(|e| format!("事务日志落盘 flush 失败: {}", e))?;
        
    Ok(())
}

/// 彻底清除本地事务日志（用于任务完美完成或完美回滚后）
pub fn clear_job() -> Result<(), String> {
    let log_path = get_journal_path()?;
    if log_path.exists() {
        std::fs::remove_file(&log_path)
            .map_err(|e| format!("清除事务日志文件失败: {}", e))?;
    }
    Ok(())
}

/// 读取并解析当前的未决任务日志。如果不存在未决任务，返回 Ok(None)
pub fn read_job() -> Result<Option<PendingJob>, String> {
    let log_path = get_journal_path()?;
    if !log_path.exists() {
        return Ok(None);
    }

    let mut file = File::open(&log_path)
        .map_err(|e| format!("无法打开事务日志文件: {}", e))?;
    let mut data = String::new();
    file.read_to_string(&mut data)
        .map_err(|e| format!("读取事务日志文件内容失败: {}", e))?;

    let job: PendingJob = serde_json::from_str(&data)
        .map_err(|e| format!("反序列化解析事务日志失败: {}", e))?;

    Ok(Some(job))
}
