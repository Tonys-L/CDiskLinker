use std::path::Path;
use std::os::windows::process::CommandExt;
use widestring::U16CString;
use windows::core::{PCWSTR, HSTRING};
use windows::Win32::Foundation::{HANDLE, CloseHandle};
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;
use windows::Win32::System::RestartManager::{
    RmStartSession, RmRegisterResources, RmGetList, RmShutdown, RmEndSession,
    RM_PROCESS_INFO, RmForceShutdown,
};

/// 检查当前进程是否具备系统管理员权限
pub fn check_administrator_privileges() -> bool {
    unsafe {
        let mut token: HANDLE = HANDLE::default();
        // 打开当前进程的安全令牌
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;

        // 查询令牌是否已提权
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size,
            &mut size,
        );

        let _ = CloseHandle(token);

        if result.is_ok() {
            elevation.TokenIsElevated != 0
        } else {
            false
        }
    }
}

/// 请求 UAC 提权并以管理员身份重新拉起当前进程，拉起成功后旧进程退出
pub fn elevate_self() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("获取当前可执行文件路径失败: {}", e))?;
    
    // 转换为 Windows UTF-16 宽字符并加上 Null 终止符
    let file_path = HSTRING::from(current_exe.as_os_str());
    let operation = HSTRING::from("runas");

    unsafe {
        // 调用 ShellExecuteW，以管理员权限 (runas) 运行新的实例
        let h_instance = ShellExecuteW(
            None,
            &operation,
            &file_path,
            None,
            None,
            SW_SHOW,
        );

        // 如果返回值大于 32 则表示成功启动，否则为启动失败的系统错误码
        if h_instance.0 as usize > 32 {
            std::process::exit(0);
        } else {
            return Err(format!("提权拉起失败，Windows 错误码: {:?}", h_instance));
        }
    }
}

/// 在 source 路径处创建一个指向 target 的 NTFS 目录联接 (Directory Junction)
/// 
/// # 参数
/// - `source`: 源路径 (e.g. C:\Games\Steam)
/// - `target`: 迁移后的真实目录路径 (e.g. D:\Games\Steam)
pub fn create_junction(source: &Path, target: &Path) -> Result<(), String> {
    // 1. 如果源路径已存在，先删除（mklink /J 要求源路径不存在）
    // 关键安全检查：如果源路径是 Junction，必须只删链接点，绝不能跟入！
    // std::fs::remove_dir_all 会跟随 Junction 删除目标真实数据，造成不可逆损失。
    if source.exists() {
        if is_junction(source) {
            // Junction：只删链接点（remove_dir 对 Junction 安全，不跟入）
            std::fs::remove_dir(source)
                .map_err(|e| format!("删除已存在的 Junction 链接点失败: {}", e))?;
        } else if source.is_dir() {
            // 普通目录：递归删除，但遇到子 Junction 只删链接点不跟入
            remove_dir_all_safe(source)?;
        } else {
            // 普通文件
            std::fs::remove_file(source)
                .map_err(|e| format!("删除已存在的源文件失败: {}", e))?;
        }
    }

    // 2. 确保源路径的父目录存在
    if let Some(parent) = source.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建源路径的父目录: {}", e))?;
        }
    }

    // 3. 使用 cmd /c mklink /J 创建 Junction（mklink 会自动创建 Junction 目录）
    // 注意：路径可能含空格，使用 /c 后接完整命令字符串
    let source_str = source.to_str()
        .ok_or_else(|| "源路径包含无效的 UTF-8 字符".to_string())?;
    let target_str = target.to_str()
        .ok_or_else(|| "目标路径包含无效的 UTF-8 字符".to_string())?;

    let output = std::process::Command::new("cmd")
        .raw_arg(format!("/c mklink /J \"{}\" \"{}\"", source_str, target_str))
        .output()
        .map_err(|e| format!("执行 mklink 命令失败: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        // mklink 输出是 GBK 编码，用 GBK 解码
        let stdout = encoding_rs::GBK.decode(&output.stdout).0;
        let stderr = encoding_rs::GBK.decode(&output.stderr).0;
        Err(format!("mklink /J 失败: {} {}", stdout.trim(), stderr.trim()))
    }
}

/// 安全递归删除目录（Junction 安全版）
///
/// 与 std::fs::remove_dir_all 的区别：
/// 遇到子目录中的 Junction 时只删除链接点，绝不跟入目标。
/// std::fs::remove_dir_all 会跟随 Junction 删除目标真实数据，造成不可逆损失。
fn remove_dir_all_safe(dir: &Path) -> Result<(), String> {
    fn remove_recursive(path: &Path) -> Result<(), String> {
        // Junction：只删链接点，不跟入（remove_dir 对 Junction 安全）
        if is_junction(path) {
            std::fs::remove_dir(path)
                .map_err(|e| format!("删除 Junction 链接点失败 {:?}: {}", path, e))?;
        } else if path.is_symlink() {
            // 文件级符号链接：删除链接本身
            std::fs::remove_file(path)
                .map_err(|e| format!("删除符号链接失败 {:?}: {}", path, e))?;
        } else if path.is_dir() {
            for entry in std::fs::read_dir(path)
                .map_err(|e| format!("读取目录失败 {:?}: {}", path, e))?
            {
                let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
                remove_recursive(&entry.path())?;
            }
            std::fs::remove_dir(path)
                .map_err(|e| format!("删除目录失败 {:?}: {}", path, e))?;
        } else {
            std::fs::remove_file(path)
                .map_err(|e| format!("删除文件失败 {:?}: {}", path, e))?;
        }
        Ok(())
    }
    remove_recursive(dir)
}

/// 删除一个目录联接 (Junction) 点占位符
/// 
/// # 说明
/// 删除 Junction 占位符目录时，Windows NTFS 只会切断联接关系并移除占位符文件夹，
/// 绝对**不会**误删 D 盘真实的目标文件夹。
pub fn delete_junction(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    
    // 在 Windows 下安全删除重解析点目录，使用标准库的 remove_dir 即可
    // 它是对 RemoveDirectoryW 的安全包装，对于 Junction 会自动剥离重解析点，安全无害
    std::fs::remove_dir(path)
        .map_err(|e| format!("清理联接点失败: {}", e))
}

/// 扫描并获取占用指定目录中任何文件的进程列表 (PID, 进程名)
pub fn query_file_locks(path: &Path) -> Result<Vec<(u32, String)>, String> {
    let path_canonical = path.canonicalize()
        .map_err(|e| format!("路径规范化失败: {}", e))?;
    
    // 使用 widestring 安全转换，不依赖 std::os::windows 的特定函数报错
    let path_w = U16CString::from_os_str(&path_canonical)
        .map_err(|e| format!("路径宽字符转换失败: {}", e))?
        .as_slice_with_nul()
        .to_vec();

    unsafe {
        let mut session_handle = 0u32;
        let mut session_key = [0u16; 33]; // CCH_RM_SESSION_KEY 是 32 字符长

        // 1. 开启 Restart Manager 会话，API 成功时返回 Ok(())
        RmStartSession(
            &mut session_handle,
            0,
            windows::core::PWSTR(session_key.as_mut_ptr()),
        ).map_err(|e| format!("开启重启管理器会话失败: {}", e))?;

        // 2. 注册要扫描的资源路径
        let path_ptrs = [PCWSTR(path_w.as_ptr())];
        if let Err(e) = RmRegisterResources(
            session_handle,
            Some(&path_ptrs),
            None,
            None,
        ) {
            let _ = RmEndSession(session_handle);
            return Err(format!("向重启管理器注册路径失败: {}", e));
        }

        // 3. 探测占用列表
        let mut n_proc_needed = 0u32;
        let mut n_proc = 0u32;
        let mut process_info = vec![RM_PROCESS_INFO::default(); 1];
        let mut reason = 0u32;

        // 第一次调用获取所需的数组大小
        let mut res = RmGetList(
            session_handle,
            &mut n_proc_needed,
            &mut n_proc,
            Some(process_info.as_mut_ptr()),
            &mut reason,
        );

        if let Err(ref e) = res {
            // ERROR_MORE_DATA 的错误码是 234
            if (e.code().0 as u32) & 0xFFFF == 234 {
                process_info = vec![RM_PROCESS_INFO::default(); n_proc_needed as usize];
                n_proc = n_proc_needed;
                res = RmGetList(
                    session_handle,
                    &mut n_proc_needed,
                    &mut n_proc,
                    Some(process_info.as_mut_ptr()),
                    &mut reason,
                );
            }
        }

        let mut results = Vec::new();
        if res.is_ok() {
            for i in 0..(n_proc as usize) {
                let info = &process_info[i];
                let pid = info.Process.dwProcessId;
                
                // 将字符数组转换为 Rust String
                let len = info.strAppName.iter().position(|&x| x == 0).unwrap_or(32);
                let app_name = String::from_utf16_lossy(&info.strAppName[..len]);
                results.push((pid, app_name));
            }
        }

        let _ = RmEndSession(session_handle);
        
        // 校验最终结果
        res.map_err(|e| format!("获取占用进程列表失败: {}", e))?;
        
        Ok(results)
    }
}

/// 递归收集目录下所有文件路径（排除子目录和重解析点）
fn collect_file_paths_recursive(dir: &Path, list: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                if metadata.file_type().is_symlink() {
                    continue;
                }
                if metadata.is_dir() {
                    collect_file_paths_recursive(&path, list)?;
                } else {
                    list.push(path);
                }
            }
        }
    }
    Ok(())
}

/// 递归检测目录下所有文件的占用进程（目录级检测可能遗漏子文件占用）
///
/// 实现：递归收集所有文件路径 → 批量注册给 Restart Manager → 获取占用进程列表。
/// 适用于删除目录失败时定位是哪个进程锁住了子文件。
pub fn query_dir_locks_recursive(dir: &Path) -> Result<Vec<(u32, String)>, String> {
    // 1. 递归收集所有文件路径
    let mut file_paths = Vec::new();
    collect_file_paths_recursive(dir, &mut file_paths)?;

    if file_paths.is_empty() {
        return Ok(Vec::new());
    }

    // 2. 转换为宽字符串向量（保持生命周期）
    let path_w_vecs: Vec<Vec<u16>> = file_paths.iter()
        .filter_map(|p| {
            U16CString::from_os_str(p.as_os_str())
                .ok()
                .map(|s| s.as_slice_with_nul().to_vec())
        })
        .collect();

    if path_w_vecs.is_empty() {
        return Ok(Vec::new());
    }

    let path_ptrs: Vec<PCWSTR> = path_w_vecs.iter()
        .map(|v| PCWSTR(v.as_ptr()))
        .collect();

    unsafe {
        let mut session_handle = 0u32;
        let mut session_key = [0u16; 33];

        RmStartSession(
            &mut session_handle,
            0,
            windows::core::PWSTR(session_key.as_mut_ptr()),
        ).map_err(|e| format!("开启重启管理器会话失败: {}", e))?;

        // 批量注册所有文件路径
        if let Err(e) = RmRegisterResources(
            session_handle,
            Some(&path_ptrs),
            None,
            None,
        ) {
            let _ = RmEndSession(session_handle);
            return Err(format!("批量注册文件路径失败: {}", e));
        }

        // 探测占用列表
        let mut n_proc_needed = 0u32;
        let mut n_proc = 0u32;
        let mut process_info = vec![RM_PROCESS_INFO::default(); 1];
        let mut reason = 0u32;

        let mut res = RmGetList(
            session_handle,
            &mut n_proc_needed,
            &mut n_proc,
            Some(process_info.as_mut_ptr()),
            &mut reason,
        );

        if let Err(ref e) = res {
            if (e.code().0 as u32) & 0xFFFF == 234 {
                process_info = vec![RM_PROCESS_INFO::default(); n_proc_needed as usize];
                n_proc = n_proc_needed;
                res = RmGetList(
                    session_handle,
                    &mut n_proc_needed,
                    &mut n_proc,
                    Some(process_info.as_mut_ptr()),
                    &mut reason,
                );
            }
        }

        let mut results = Vec::new();
        if res.is_ok() {
            for i in 0..(n_proc as usize) {
                let info = &process_info[i];
                let pid = info.Process.dwProcessId;
                let len = info.strAppName.iter().position(|&x| x == 0).unwrap_or(32);
                let app_name = String::from_utf16_lossy(&info.strAppName[..len]);
                results.push((pid, app_name));
            }
        }

        let _ = RmEndSession(session_handle);

        res.map_err(|e| format!("递归获取占用进程列表失败: {}", e))?;

        Ok(results)
    }
}

/// 检测单个文件是否被锁定（无法获取删除句柄）
///
/// 以 DELETE 权限 + FILE_SHARE_READ|WRITE（不含 DELETE）尝试打开文件。
/// 若文件被其他进程以不含 FILE_SHARE_DELETE 的方式打开，此调用失败 → 文件被锁定。
fn is_file_locked(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .access_mode(0x00010000) // DELETE access
        .share_mode(0x00000003)  // FILE_SHARE_READ | FILE_SHARE_WRITE（不含 DELETE）
        .open(path)
        .is_err()
}

/// 递归遍历目录，找到第一个被锁定的文件（无法获取删除句柄）
///
/// 用于删除目录失败时定位具体是哪个文件被占用。
/// 返回被锁定文件的完整路径。
pub fn find_locked_file(dir: &Path) -> Option<std::path::PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(metadata) = entry.metadata() {
                    if metadata.file_type().is_symlink() {
                        continue;
                    }
                    if metadata.is_dir() {
                        stack.push(path);
                    } else if is_file_locked(&path) {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

/// 强制终止占用特定路径 of 进程列表 (通过 PID 强制终止)
pub fn force_release_locks(path: &Path) -> Result<(), String> {
    let path_canonical = path.canonicalize()
        .map_err(|e| format!("路径规范化失败: {}", e))?;
    
    let path_w = U16CString::from_os_str(&path_canonical)
        .map_err(|e| format!("路径宽字符转换失败: {}", e))?
        .as_slice_with_nul()
        .to_vec();

    unsafe {
        let mut session_handle = 0u32;
        let mut session_key = [0u16; 33];

        RmStartSession(
            &mut session_handle,
            0,
            windows::core::PWSTR(session_key.as_mut_ptr()),
        ).map_err(|e| format!("开启重启管理器会话失败: {}", e))?;

        let path_ptrs = [PCWSTR(path_w.as_ptr())];
        if let Err(e) = RmRegisterResources(session_handle, Some(&path_ptrs), None, None) {
            let _ = RmEndSession(session_handle);
            return Err(format!("向重启管理器注册路径失败: {}", e));
        }

        // 调用 RmShutdown 关闭占用该资源的所有进程
        let res = RmShutdown(session_handle, RmForceShutdown.0 as u32, None);

        let _ = RmEndSession(session_handle);

        res.map_err(|e| format!("强制解除文件锁失败: {}", e))
    }
}

/// 获取指定盘符（例如 "C:\"）的总容量与剩余可用字节数 (总容量, 剩余容量)
pub fn get_disk_space_info(drive: &str) -> Result<(u64, u64), String> {
    let drive_w = widestring::U16CString::from_str(drive)
        .map_err(|e| format!("盘符编码转换失败: {}", e))?;
    
    let mut free_bytes_available = 0u64;
    let mut total_number_of_bytes = 0u64;
    let mut total_number_of_free_bytes = 0u64;

    unsafe {
        let result = windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            windows::core::PCWSTR(drive_w.as_ptr()),
            Some(&mut free_bytes_available),
            Some(&mut total_number_of_bytes),
            Some(&mut total_number_of_free_bytes),
        );
        
        if result.is_ok() {
            Ok((total_number_of_bytes, free_bytes_available))
        } else {
            Err("调用 Windows API 查询磁盘空间失败".to_string())
        }
    }
}

/// 判断路径是否为 NTFS 目录联接 (Junction)
///
/// 精确检测：不仅检查 FILE_ATTRIBUTE_REPARSE_POINT 属性，
/// 还通过 DeviceIoControl + FSCTL_GET_REPARSE_POINT 读取重解析标签，
/// 仅当标签为 IO_REPARSE_TAG_MOUNT_POINT 时返回 true。
///
/// # 重解析标签值依据
///
/// 参考:
/// - 标签定义: <https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-tags>
/// - 协议规范: <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/c8e77b37-3909-4fe6-a4ea-2b9d423b1ee4>
/// - Junction 数据结构: <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/ca069dad-ed16-42aa-b057-b6b207f447cc>
///
/// | 标签值           | 常量名                          | 含义               | 来源 |
/// |-----------------|--------------------------------|--------------------|------|
/// | 0xA0000003      | IO_REPARSE_TAG_MOUNT_POINT     | NTFS Junction（本函数检测的目标）| MS-FSCC 2.1.2.5 |
/// | 0xA000000C      | IO_REPARSE_TAG_SYMLINK         | 符号链接            | MS-FSCC 2.1.2.1 |
/// | 0x80000014      | IO_REPARSE_TAG_NFS             | NFS 符号链接         | MS-FSCC 2.1.2.1 |
/// | 0x80000023      | （未公开）                       | 应用占位符（如 JetBrains Toolbox cache）| fsutil 实测 |
/// | 0x9000001A      | IO_REPARSE_TAG_CLOUD           | 云文件占位符           | WinNT.h |
///
/// 高 2 位含义：10 = Microsoft 定义，01 = 第三方定义；
/// Bit 29 = 1 表示名称代理（目标为另一个命名实体，如 Junction/Symlink）。
///
/// 仅 IO_REPARSE_TAG_MOUNT_POINT 是 Junction，其他重解析点不应按 Junction 处理。
pub fn is_junction(path: &std::path::Path) -> bool {
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, CreateFileW, FILE_FLAGS_AND_ATTRIBUTES,
        FILE_CREATION_DISPOSITION, FILE_SHARE_MODE,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
    const GENERIC_READ: u32 = 0x80000000;
    // FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT
    const OPEN_REPARSE_POINT_FLAGS: FILE_FLAGS_AND_ATTRIBUTES =
        FILE_FLAGS_AND_ATTRIBUTES(0x02000000 | 0x00200000);
    const OPEN_EXISTING: FILE_CREATION_DISPOSITION = FILE_CREATION_DISPOSITION(3);
    // FSCTL_GET_REPARSE_POINT CTL_CODE
    const FSCTL_GET_REPARSE_POINT: u32 = 0x000900A8;

    let path_w = match U16CString::from_os_str(path.as_os_str()) {
        Ok(s) => s,
        Err(_) => return false,
    };

    unsafe {
        // 快速预检：无 REPARSE_POINT 属性则直接返回 false
        let attrs = GetFileAttributesW(PCWSTR(path_w.as_ptr()));
        if attrs == u32::MAX || (attrs & FILE_ATTRIBUTE_REPARSE_POINT) == 0 {
            return false;
        }

        // 打开重解析点本身（不跟随目标）
        let handle = CreateFileW(
            PCWSTR(path_w.as_ptr()),
            GENERIC_READ,
            FILE_SHARE_MODE(0x01 | 0x02), // FILE_SHARE_READ | FILE_SHARE_WRITE
            None,
            OPEN_EXISTING,
            OPEN_REPARSE_POINT_FLAGS,
            HANDLE::default(),
        );

        let handle = match handle {
            Ok(h) => h,
            Err(_) => return false,
        };

        if handle.is_invalid() {
            return false;
        }

        // 分配足够大的缓冲区接收 REPARSE_DATA_BUFFER
        let mut buffer = [0u8; 1024];
        let mut bytes_returned = 0u32;

        let result = DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(buffer.as_mut_ptr() as *mut _),
            buffer.len() as u32,
            Some(&mut bytes_returned),
            None,
        );

        let _ = CloseHandle(handle);

        if result.is_err() {
            return false;
        }

        // REPARSE_DATA_BUFFER 前四个字节是 ReparseTag
        if bytes_returned < 4 {
            return false;
        }

        let reparse_tag = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        reparse_tag == IO_REPARSE_TAG_MOUNT_POINT
    }
}

/// 快速检测路径是否具有 REPARSE_POINT 属性（不区分重解析点类型）
///
/// 用途：在 is_junction() 返回 false 后，二次过滤非 Junction 重解析点。
/// 这些条目（如 JetBrains cache tag 0x80000023、云占位符等）无法通过 File::open 访问，
/// 必须跳过，否则会遇到 os error 1920 (ERROR_CANT_ACCESS_FILE)。
///
/// 性能：仅调用一次 GetFileAttributesW（内核直接读 MFT，微秒级），不打开文件句柄。
pub fn is_reparse_point(path: &std::path::Path) -> bool {
    use windows::Win32::Storage::FileSystem::GetFileAttributesW;
    use windows::core::PCWSTR;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    let path_w = match U16CString::from_os_str(path.as_os_str()) {
        Ok(s) => s,
        Err(_) => return false,
    };

    unsafe {
        let attrs = GetFileAttributesW(PCWSTR(path_w.as_ptr()));
        attrs != u32::MAX && (attrs & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    }
}

/// 读取 Junction 的目标路径
///
/// 使用 fsutil reparsepoint query 命令解析 Junction 目标。
/// 返回 Junction 指向的绝对路径（如 `C:\Users\xxx\AppData\Local\Kingsoft\cloud`）。
/// 如果路径不是 Junction 或解析失败，返回错误。
///
/// 支持中文 Windows（"打印名称:"）和英文 Windows（"Print Name:"）两种输出格式。
pub fn read_junction_target(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let path_str = path.to_str()
        .ok_or_else(|| "路径包含无效 UTF-8 字符".to_string())?;

    let output = std::process::Command::new("fsutil")
        .args(["reparsepoint", "query", path_str])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| format!("执行 fsutil 失败: {}", e))?;

    if !output.status.success() {
        return Err(format!("fsutil reparsepoint query 失败: 路径可能不是重解析点"));
    }

    // fsutil 输出格式（中文 Windows）：
    //   打印名称:              F:\.c_cache\Kingsoft
    // fsutil 输出格式（英文 Windows）：
    //   Print Name:            F:\.c_cache\Kingsoft
    // 两种格式均尝试解析
    let stdout = encoding_rs::GBK.decode(&output.stdout).0;

    // 定义可能的标签（中文 + 英文）
    let labels = ["打印名称:", "Print Name:"];

    for line in stdout.lines() {
        for label in &labels {
            if line.contains(label) {
                if let Some(idx) = line.find(label) {
                    let target = line[idx + label.len()..].trim();
                    if !target.is_empty() {
                        return Ok(std::path::PathBuf::from(target));
                    }
                }
            }
        }
    }

    Err("无法从 fsutil 输出中解析 Junction 目标路径（不支持当前系统语言格式）".to_string())
}

/// 检测指定盘根路径（如 "D:\\"）的文件系统是否为 NTFS
///
/// Junction 仅在 NTFS 上受支持，FAT32/exFAT/网络盘会创建失败。
pub fn is_ntfs(drive_root: &str) -> Result<bool, String> {
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;

    let drive_w = U16CString::from_str(drive_root)
        .map_err(|e| format!("盘符编码转换失败: {}", e))?;

    let mut file_system_name = [0u16; 256];
    unsafe {
        let res = GetVolumeInformationW(
            PCWSTR(drive_w.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut file_system_name),
        );
        if res.is_err() {
            return Err("调用 GetVolumeInformationW 获取文件系统信息失败".to_string());
        }
        let len = file_system_name.iter().position(|&x| x == 0).unwrap_or(0);
        let fs_name = String::from_utf16_lossy(&file_system_name[..len]);
        Ok(fs_name.eq_ignore_ascii_case("NTFS"))
    }
}

/// 读取 Windows 系统代理设置（从注册表 Internet Settings）
/// 返回格式如 "http://127.0.0.1:7897"，未配置代理时返回 None
/// 用于让 reqwest（updater 插件）能通过系统代理访问 GitHub Releases
pub fn get_system_proxy() -> Option<String> {
    // ProxyEnable 为 0x1（REG_DWORD）表示启用系统代理
    let proxy_enable = read_registry_value(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        "ProxyEnable",
    )?;
    if !proxy_enable.contains("0x1") {
        return None;
    }

    let proxy_server = read_registry_value(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        "ProxyServer",
    )?;
    let proxy_server = proxy_server.trim();
    if proxy_server.is_empty() {
        return None;
    }

    // ProxyServer 格式可能是:
    //   "127.0.0.1:7897"                 (统一代理)
    //   "http=...;https=..."             (分协议代理)
    // 优先取 https，其次 http，最后整体
    let proxy_addr = if proxy_server.contains('=') {
        proxy_server
            .split(';')
            .find_map(|s| {
                let s = s.trim();
                s.strip_prefix("https=").or_else(|| s.strip_prefix("http="))
            })
            .unwrap_or("")
    } else {
        proxy_server
    };

    if proxy_addr.is_empty() {
        return None;
    }

    // reqwest 需要 URL 格式的代理地址
    let proxy_url = if proxy_addr.starts_with("http://") || proxy_addr.starts_with("https://") {
        proxy_addr.to_string()
    } else {
        format!("http://{}", proxy_addr)
    };
    Some(proxy_url)
}

/// 调用 reg query 读取注册表值（技术层 win_util 允许调用 OS 命令）
fn read_registry_value(key: &str, value_name: &str) -> Option<String> {
    let output = std::process::Command::new("reg")
        .args(["query", key, "/v", value_name])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // reg query 输出格式:
    //     ProxyEnable    REG_DWORD    0x1
    // split_whitespace 后取最后一字段
    stdout
        .lines()
        .find(|line| line.contains(value_name))
        .and_then(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.last().map(|s| s.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_junction_basic_create_and_delete() {
        let temp_dir = std::env::temp_dir();
        let test_root = temp_dir.join("cdisklinker_test_basic");
        let _ = fs::remove_dir_all(&test_root);
        fs::create_dir_all(&test_root).unwrap();

        let source = test_root.join("source_link");
        let target = test_root.join("target_real");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("test.txt"), "hello junction").unwrap();

        // 1. 创建 Junction（源路径不存在）
        let result = create_junction(&source, &target);
        assert!(result.is_ok(), "创建 Junction 失败: {:?}", result.err());

        // 2. 通过源路径读取目标文件
        let content = fs::read_to_string(source.join("test.txt")).unwrap();
        assert_eq!(content, "hello junction");

        // 3. 删除 Junction
        let del_result = delete_junction(&source);
        assert!(del_result.is_ok(), "删除 Junction 失败: {:?}", del_result.err());

        // 4. 源路径已移除，目标数据完好
        assert!(!source.exists());
        assert!(target.join("test.txt").exists());

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn test_junction_source_already_exists_as_dir() {
        let temp_dir = std::env::temp_dir();
        let test_root = temp_dir.join("cdisklinker_test_exists");
        let _ = fs::remove_dir_all(&test_root);
        fs::create_dir_all(&test_root).unwrap();

        let source = test_root.join("source_link");
        let target = test_root.join("target_real");
        
        // 源路径已存在（空目录）
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("data.bin"), b"test").unwrap();

        // create_junction 应该先删除空目录再创建 Junction
        let result = create_junction(&source, &target);
        assert!(result.is_ok(), "源目录已存在时创建 Junction 失败: {:?}", result.err());

        // 验证 Junction 生效
        assert!(source.join("data.bin").exists());

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn test_junction_source_non_empty_dir() {
        let temp_dir = std::env::temp_dir();
        let test_root = temp_dir.join("cdisklinker_test_nonempty");
        let _ = fs::remove_dir_all(&test_root);
        fs::create_dir_all(&test_root).unwrap();

        let source = test_root.join("source_link");
        let target = test_root.join("target_real");
        
        // 源路径已存在且有文件
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("old_file.txt"), "old content").unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("new_file.txt"), "new content").unwrap();

        // create_junction 应该先 remove_dir_all 再创建 Junction
        let result = create_junction(&source, &target);
        assert!(result.is_ok(), "源目录非空时创建 Junction 失败: {:?}", result.err());

        // 验证 Junction 指向目标（能看到目标的内容）
        assert!(source.join("new_file.txt").exists());

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn test_full_migration_flow() {
        // 模拟完整迁移流程：复制 → 删源 → 重命名 → 建 Junction
        let temp_dir = std::env::temp_dir();
        let test_root = temp_dir.join("cdisklinker_test_migration");
        let _ = fs::remove_dir_all(&test_root);
        fs::create_dir_all(&test_root).unwrap();

        // 模拟源目录 C:\AAA
        let source_dir = test_root.join("AAA");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("file1.txt"), "content1").unwrap();
        fs::create_dir_all(source_dir.join("subdir")).unwrap();
        fs::write(source_dir.join("subdir/file2.txt"), "content2").unwrap();

        // 模拟目标路径 D:\DDD
        let target_base = test_root.join("DDD");
        fs::create_dir_all(&target_base).unwrap();

        let final_target = target_base.join("AAA");
        let tmp_target = target_base.join(".tmp_AAA");

        // Step 1: 复制源 → 临时目标
        copy_dir_recursive_test(&source_dir, &tmp_target);

        // Step 2: 删除源
        fs::remove_dir_all(&source_dir).unwrap();
        assert!(!source_dir.exists());

        // Step 3: 重命名临时目标 → 正式目标
        fs::rename(&tmp_target, &final_target).unwrap();
        assert!(final_target.join("file1.txt").exists());

        // Step 4: 创建 Junction
        let result = create_junction(&source_dir, &final_target);
        assert!(result.is_ok(), "迁移流程中创建 Junction 失败: {:?}", result.err());

        // Step 5: 验证 Junction 生效
        assert!(source_dir.join("file1.txt").exists());
        assert!(source_dir.join("subdir/file2.txt").exists());
        let content = fs::read_to_string(source_dir.join("subdir/file2.txt")).unwrap();
        assert_eq!(content, "content2");

        let _ = fs::remove_dir_all(&test_root);
    }

    /// 辅助：递归复制目录（测试用）
    fn copy_dir_recursive_test(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                copy_dir_recursive_test(&src_path, &dst_path);
            } else {
                fs::copy(&src_path, &dst_path).unwrap();
            }
        }
    }
}
