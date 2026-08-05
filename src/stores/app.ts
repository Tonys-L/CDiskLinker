import { defineStore } from 'pinia'
import { ref, computed, shallowRef } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import i18n from '../i18n'

// 帮助弹窗的 localStorage 持久化 key：标记用户是否已看过首次帮助
const HELP_DISMISSED_KEY = 'help-dismissed'
// 更新检查最小间隔（毫秒）：避免频繁手动触发请求 GitHub
const UPDATE_CHECK_MIN_INTERVAL = 60 * 1000

// 后端 best_effort 删除结果（对应 Rust DeleteResult）
interface DeleteResult {
  fully_deleted: boolean
  failed_files: string[]
}

// 后端返回的字符串可能是 i18n key（如 "err.xxx" / "log.xxx"），命中则按当前语言翻译，否则原样返回。
// 兼容底层 OS 错误等不可枚举的动态文本。
function translateError(e: unknown): string {
  const msg = String(e)
  if (/^(err|log)\./.test(msg)) {
    return i18n.global.t(msg)
  }
  return msg
}

// 后端 invoke 返回的 Ok(String) 可能是 i18n key，命中则翻译；否则视为动态文本原样返回。
function translateResult(msg: string): string {
  if (/^(err|log)\./.test(msg)) {
    return i18n.global.t(msg)
  }
  return msg
}

// === 类型定义 ===
export interface TreeNode {
  id: number
  path: string
  name: string
  size_text: string
  actual_size_bytes: number
  rating: 'Safe' | 'Warning' | 'Forbidden'
  level: number
  is_expanded: boolean
  has_children: boolean
  is_selected: boolean
  is_junction: boolean
  is_visible: boolean
  children_count: number
  is_migrated_by_us: boolean
}

export interface DiskInfo {
  drive: string
  total: string
  free: string
  used_percent: number
}

export interface LogEntry {
  timestamp: string
  level: 'info' | 'warn' | 'error' | 'success'
  // i18n key：命中时由 LogConsole 渲染时翻译为当前语言
  i18nKey?: string
  // i18n 插值参数
  params?: Record<string, unknown>
  // 原始消息：i18n key 未命中或无 key 时的回退显示（也用于后端透传 detail）
  message: string
}

export interface LockProcess {
  pid: number
  name: string
}

// 大目录排行榜条目（与后端 LargeDirEntry 对应）
export interface LargeDirEntry {
  path: string
  name: string
  size_bytes: number
  size_text: string
  rating: string  // "Safe" / "Warning" / "Forbidden"
  depth: number
}

// 迁移档案列表项（与后端 ArchiveListItem 对应）
// 后端通过 #[serde(flatten)] 展开了 MigrationArchive 字段 + meta_file_exists 标记
export interface ArchiveListItem {
  version: number
  archive_id: string
  source_path: string
  target_path: string
  created_at: number  // Unix 秒
  manifest_self_hash: string
  total_files: number
  total_size: number
  software_version: string
  archive_self_hash: string
  meta_file_exists: boolean
  junction_exists: boolean
}

export type MigrationStatus = 'Idle' | 'Scanning' | 'Copying' | 'RollingBack' | 'Completed' | 'PendingConfirmation'

// 将字节数格式化为人类可读大小（KB/MB/GB）
function formatSize(bytes: number): string {
  if (!bytes || bytes < 0) return '0 B'
  if (bytes >= 1024 * 1024 * 1024) {
    return (bytes / (1024 * 1024 * 1024)).toFixed(2) + ' GB'
  } else if (bytes >= 1024 * 1024) {
    return (bytes / (1024 * 1024)).toFixed(2) + ' MB'
  } else if (bytes >= 1024) {
    return (bytes / 1024).toFixed(2) + ' KB'
  }
  return bytes + ' B'
}

export const useAppStore = defineStore('app', () => {
  // === 状态 ===
  const isAdmin = ref(false)
  const diskInfo = ref<DiskInfo | null>(null)
  const treeNodes = ref<TreeNode[]>([])
  const targetPath = ref('D:\\CDiskLinker_Moved')
  const manualSourcePath = ref('')
  const fastMode = ref(false)
  const migrationStatus = ref<MigrationStatus>('Idle')
  const migrationPercent = ref(0)
  const migrationDetail = ref('')
  const migrationCurrentItem = ref(1)
  const migrationTotalItems = ref(1)
  const migrationFolder = ref('')
  // 后端结构化进度字段（与 migration-progress 事件 payload 对应）
  const migrationStage = ref<string>('')
  const migrationTotalFiles = ref(0)
  const migrationCopiedFiles = ref(0)
  const migrationTotalSize = ref(0)
  const migrationCopiedSize = ref(0)
  const migrationCurrentFile = ref('')
  // 文件占用检测相关
  const lockingProcesses = ref<LockProcess[]>([])
  const showLockDialog = ref(false)
  const logs = ref<LogEntry[]>([])
  const showWarningDialog = ref(false)
  const warningPaths = ref<string[]>([])
  const crashRecoveryMsg = ref('')
  const crashRecoveryStage = ref('')  // journal stage: "Linked" / "Copied" / "Finalized" 等
  const scanDetail = ref('')
  // 迁移确认对话框：Linked 状态下提示用户测试软件
  const showConfirmDialog = ref(false)
  const confirmSourcePath = ref('')
  const confirmOldPath = ref('')
  const confirmTargetPath = ref('')
  // 帮助对话框：首次启动自动弹出，或用户点击右上角 "?" 按钮触发
  const helpVisible = ref(false)
  // 大目录排行榜：递归扫描 C 盘，按大小降序排列的 Top 20 目录
  const largeDirs = ref<LargeDirEntry[]>([])
  const largeDirsScanning = ref(false)
  // 迁移历史：已迁移目录的档案列表（含目标目录自包含档案的存在性标记）
  const migrationHistory = ref<ArchiveListItem[]>([])
  const historyLoading = ref(false)
  // 更新对话框：发现新版本时弹出，包含版本号、更新日志、下载安装进度
  const updateVisible = ref(false)
  // Update 对象继承 Tauri Resource（含私有字段），必须用 shallowRef 避免 Vue 深度 Proxy 代理
// 否则访问私有字段时报 "Cannot read private member from an object whose class did not declare it"
const updateInfo = shallowRef<Update | null>(null)
  const updateChecking = ref(false)
  const updateDownloading = ref(false)
  const updateProgress = ref(0)
  const updateProgressText = ref('')
  const updateErrorMsg = ref('')
  // 上次更新检查时间戳，用于节流（避免用户狂点导致频繁请求 GitHub）
  let lastUpdateCheckAt = 0
  // 内部暂存：占用检测通过后需要继续迁移的路径，以及被占用的路径列表
  let pendingMigrationPaths: string[] = []
  let lockedPaths: string[] = []

  // === 计算属性 ===
  const selectedNodes = computed(() => treeNodes.value.filter(n => n.is_selected))
  const selectedSafeNodes = computed(() => selectedNodes.value.filter(n => n.rating === 'Safe'))
  const selectedWarningNodes = computed(() => selectedNodes.value.filter(n => n.rating === 'Warning'))
  const totalSelectedSize = computed(() => {
    return selectedNodes.value.reduce((sum, n) => sum + n.actual_size_bytes, 0)
  })
  const canMigrate = computed(() => {
    const hasSource = selectedNodes.value.length > 0 || manualSourcePath.value.trim().length > 0
    return hasSource
      && selectedNodes.value.every(n => n.rating !== 'Forbidden')
      && !selectedNodes.value.some(n => n.is_junction)
      && targetPath.value.trim().length > 0
      && migrationStatus.value === 'Idle'
  })

  // === 方法 ===
  async function checkAdmin() {
    try {
      isAdmin.value = await invoke<boolean>('check_admin')
    } catch (e) {
      isAdmin.value = false
    }
  }

  async function elevateSelf() {
    try {
      await invoke('elevate_self')
    } catch (e) {
      addLog('error', `提权失败: ${e}`, 'log.elevateFailed', { error: translateError(e) })
    }
  }

  async function refreshDiskInfo() {
    try {
      diskInfo.value = await invoke<DiskInfo>('get_disk_info', { drive: 'C' })
    } catch (e) {
      addLog('error', `获取磁盘信息失败: ${e}`, 'log.diskInfoFailed', { error: translateError(e) })
    }
  }

  async function scanDisk() {
    if (migrationStatus.value === 'Scanning') return
    try {
      migrationStatus.value = 'Scanning'
      scanDetail.value = '正在扫描C盘根目录...'
      addLog('info', '开始扫描C盘...', 'log.scanStart')
      // 异步调用，结果通过事件返回
      await invoke('scan_disk')
    } catch (e) {
      migrationStatus.value = 'Idle'
      addLog('error', `扫描失败: ${e}`, 'log.scanFailed', { error: translateError(e) })
    }
  }

  // 大目录排行榜：递归扫描 C 盘，按大小降序返回 Top 20
  async function scanLargeDirs() {
    if (largeDirsScanning.value) return
    try {
      largeDirsScanning.value = true
      largeDirs.value = []
      // 异步调用，结果通过事件返回
      await invoke('scan_large_directories', { maxDepth: 4, topN: 20 })
    } catch (e) {
      largeDirsScanning.value = false
      addLog('error', `大目录扫描失败: ${e}`, 'log.scanFailed', { error: translateError(e) })
    }
  }

  // 将大目录排行榜中的目录设为源目录（填入手动源路径输入框）
  // 单选模式：清空树勾选，避免与 manualSourcePath 路径冲突
  function selectLargeDir(path: string) {
    treeNodes.value.forEach(n => { n.is_selected = false })
    manualSourcePath.value = path
  }

  // 加载迁移历史档案列表
  async function loadMigrationHistory() {
    historyLoading.value = true
    try {
      migrationHistory.value = await invoke<ArchiveListItem[]>('list_migration_history')
    } catch (e) {
      addLog('error', `加载迁移历史失败: ${e}`, 'log.historyLoadFailed', { error: translateError(e) })
    } finally {
      historyLoading.value = false
    }
  }

  // 从档案恢复（将数据搬回 C 盘原位置 + 重建源目录）
  // 恢复进度通过现有 migration-progress 事件推送，主界面进度条自动更新
  async function restoreFromArchive(archiveId: string) {
    try {
      migrationStatus.value = 'Copying'
      addLog('info', '开始从档案恢复...', 'log.restoreStart')
      await invoke('restore_from_archive', { archiveId })
      addLog('info', '恢复完成', 'log.restoreDone')
      // 恢复完成后刷新历史列表（恢复会清理档案，列表会减少一项）
      await loadMigrationHistory()
      refreshDiskInfo()
    } catch (e) {
      addLog('error', `恢复失败: ${e}`, 'log.restoreFailed', { error: translateError(e) })
    } finally {
      migrationStatus.value = 'Idle'
    }
  }

  // 修复档案元文件（用户误删目标目录的 .cdisklinker_meta.json 时从全局索引重建）
  async function rebuildArchiveMeta(archiveId: string) {
    try {
      await invoke('rebuild_archive_meta', { archiveId })
      addLog('info', '档案修复完成', 'log.rebuildDone')
      await loadMigrationHistory()
    } catch (e) {
      addLog('error', `档案修复失败: ${e}`, 'log.rebuildFailed', { error: translateError(e) })
    }
  }

  // 重建链接（用户误删了 Junction 但目标数据还在时，重建源位置的 Junction）
  async function rebuildJunction(archiveId: string) {
    try {
      await invoke('rebuild_junction', { archiveId })
      addLog('info', '链接重建完成', 'log.rebuildJunctionDone')
      await loadMigrationHistory()
    } catch (e) {
      addLog('error', `链接重建失败: ${e}`, 'log.rebuildJunctionFailed', { error: translateError(e) })
    }
  }

  function toggleNodeExpand(nodeId: number) {
    const idx = treeNodes.value.findIndex(n => n.id === nodeId)
    if (idx === -1) return

    const node = treeNodes.value[idx]
    if (node.is_expanded) {
      // 折叠：移除所有子级
      node.is_expanded = false
      const currentLevel = node.level
      treeNodes.value = treeNodes.value.filter((n, i) => {
        if (i <= idx) return true
        return n.level <= currentLevel
      })
    } else if (node.has_children) {
      // 展开标记（前端先显示展开状态）
      node.is_expanded = true
      // 异步扫描子目录，结果通过事件返回
      invoke('scan_subdirectory', {
        path: node.path,
        level: node.level + 1,
        parentId: node.id,
      }).catch((e) => {
        addLog('error', `展开目录失败: ${e}`, 'log.expandFailed', { error: translateError(e) })
        node.is_expanded = false
      })
    }
  }

  // 单选模式：一次只能勾选一个目录，勾选时自动填入源输入框
  // - 勾选新节点时，先取消所有其他勾选，再将路径填入 manualSourcePath
  // - 取消勾选时，清空 manualSourcePath
  // 这样左侧目录树与右侧源输入框保持同步，用户能直观看到选中了哪个路径
  function toggleNodeSelect(nodeId: number) {
    const node = treeNodes.value.find(n => n.id === nodeId)
    if (!node || node.rating === 'Forbidden' || node.is_junction) return

    if (node.is_selected) {
      // 取消勾选：清空源输入框
      node.is_selected = false
      manualSourcePath.value = ''
    } else {
      // 单选：先取消所有其他勾选
      treeNodes.value.forEach(n => { n.is_selected = false })
      // 勾选当前节点
      node.is_selected = true
      // 自动填入源输入框
      manualSourcePath.value = node.path
    }
  }

  async function startMigration() {
    if (!canMigrate.value) return

    if (selectedWarningNodes.value.length > 0) {
      warningPaths.value = selectedWarningNodes.value.map(n => n.name)
      showWarningDialog.value = true
      return
    }

    await doMigration()
  }

  async function doMigration() {
    showWarningDialog.value = false

    // 收集源路径：树选中的 + 手动输入的
    const paths: string[] = []

    // 树中选中的目录
    const selectedPaths = selectedNodes.value.map(n => n.path)
    paths.push(...selectedPaths)

    // 手动输入的路径（去重）
    const manual = manualSourcePath.value.trim()
    if (manual && !paths.includes(manual)) {
      paths.push(manual)
    }

    if (paths.length === 0) {
      addLog('error', '没有选择任何源目录', 'log.noSourceSelected')
      return
    }

    // 文件占用预检：落实 flows.md 中的 LockCheck 节点。
    // 必须在 UI 提示受影响进程列表，经用户确认后方可关闭（boundaries.md 文件独占解除能力约束）。
    try {
      const allLocks: LockProcess[] = []
      const locked: string[] = []
      for (const p of paths) {
        const locks = await invoke<LockProcess[]>('check_file_locks', { path: p })
        if (locks && locks.length > 0) {
          locked.push(p)
          // 去重合并进程（同一进程可能占用多个路径）
          for (const lp of locks) {
            if (!allLocks.some(x => x.pid === lp.pid)) {
              allLocks.push(lp)
            }
          }
        }
      }
      if (allLocks.length > 0) {
        pendingMigrationPaths = paths
        lockedPaths = locked
        lockingProcesses.value = allLocks
        showLockDialog.value = true
        addLog('warn', `检测到 ${allLocks.length} 个进程占用源目录，等待用户处理`, 'log.lockDetected', { count: allLocks.length })
        return
      }
    } catch (e) {
      addLog('warn', `文件占用检测不可用（可能权限不足），跳过检测继续迁移`, 'log.lockCheckUnavailable')
    }

    await _proceedMigrationAfterLockCheck(paths)
  }

  // 实际调用 migrate_selected 的内部方法
  async function _proceedMigrationAfterLockCheck(paths: string[]) {
    try {
      migrationStatus.value = 'Copying'
      await invoke('migrate_selected', {
        paths,
        targetDir: targetPath.value,
        fastMode: fastMode.value,
      })
    } catch (e) {
      migrationStatus.value = 'Idle'
      addLog('error', `迁移失败: ${e}`, 'log.migrateFailed', { error: translateError(e) })
    }
  }

  // 关闭占用进程并继续迁移
  async function killLockingProcessesAndContinue() {
    const pathsToKill = lockedPaths
    showLockDialog.value = false
    for (const p of pathsToKill) {
      try {
        await invoke('kill_locking_processes', { path: p })
      } catch (e) {
        addLog('error', `关闭占用进程失败 [${p}]: ${e}`, 'log.killLockFailed', { path: p, error: translateError(e) })
      }
    }
    addLog('info', '已尝试关闭所有占用进程，继续迁移', 'log.lockKilled')
    lockingProcesses.value = []
    lockedPaths = []
    const paths = pendingMigrationPaths
    pendingMigrationPaths = []
    if (paths.length === 0) {
      migrationStatus.value = 'Idle'
      return
    }
    await _proceedMigrationAfterLockCheck(paths)
  }

  // 取消迁移（用户拒绝关闭占用进程）
  function cancelMigrationDueToLocks() {
    showLockDialog.value = false
    lockingProcesses.value = []
    lockedPaths = []
    pendingMigrationPaths = []
    migrationStatus.value = 'Idle'
    addLog('warn', '用户取消迁移（存在文件占用）', 'log.userCancelLock')
  }

  async function checkCrashRecovery() {
    try {
      const result = await invoke<{ found: boolean; message: string; stage: string }>('check_crash_recovery')
      if (result.found) {
        crashRecoveryMsg.value = result.message
        crashRecoveryStage.value = result.stage || ''
        // 后端返回的 message 为动态内容，直接透传
        addLog('warn', result.message)
      }
    } catch (e) {
      addLog('error', `崩溃恢复检查失败: ${e}`, 'log.crashCheckFailed', { error: translateError(e) })
    }
  }

  async function rollbackJournal() {
    try {
      migrationStatus.value = 'RollingBack'
      const msg = await invoke<string>('rollback_journal')
      migrationStatus.value = 'Idle'
      crashRecoveryMsg.value = ''
      crashRecoveryStage.value = ''
      // 后端返回可能是 i18n key（如 log.noPendingJournal）或崩溃恢复动态消息，统一翻译
      addLog('success', translateResult(msg), /^(err|log)\./.test(msg) ? msg : undefined)
      refreshDiskInfo()
    } catch (e) {
      migrationStatus.value = 'Idle'
      addLog('error', `回滚失败: ${e}`, 'log.rollbackFailed', { error: translateError(e) })
    }
  }

  // 从 journal 恢复的 Linked 状态下，确认删除旧源（无参数版本）
  // 适用于应用重启后 JournalBar 显示的 Linked 状态恢复场景
  async function confirmJournalComplete() {
    try {
      migrationStatus.value = 'Copying'
      const result = await invoke<DeleteResult>('confirm_journal_complete')
      migrationStatus.value = 'Idle'
      crashRecoveryMsg.value = ''
      crashRecoveryStage.value = ''
      if (result.fully_deleted) {
        addLog('success', '旧源目录已删除，迁移完全完成！', 'log.oldSourceFullyDone')
      } else {
        addLog('warn', `迁移已完成，但部分旧源文件删除失败（共 ${result.failed_files.length} 个），请手动清理残留`, 'log.oldSourceDeletedWithResidue')
        // 逐条显示失败文件
        for (const f of result.failed_files) {
          addLog('warn', f)
        }
      }
      refreshDiskInfo()
    } catch (e) {
      migrationStatus.value = 'Idle'
      addLog('error', `删除旧源失败: ${e}`, 'log.confirmDeleteFailed', { error: translateError(e) })
    }
  }

  // 用户确认迁移正常，删除旧源目录
  async function confirmAndDeleteSource() {
    try {
      migrationStatus.value = 'Copying'
      const result = await invoke<DeleteResult>('confirm_delete_source', { path: confirmSourcePath.value })
      showConfirmDialog.value = false
      migrationStatus.value = 'Idle'
      if (result.fully_deleted) {
        addLog('success', '旧源目录已删除，迁移完全完成！', 'log.oldSourceFullyDone')
      } else {
        addLog('warn', `迁移已完成，但部分旧源文件删除失败（共 ${result.failed_files.length} 个），请手动清理残留`, 'log.oldSourceDeletedWithResidue')
        for (const f of result.failed_files) {
          addLog('warn', f)
        }
      }
      refreshDiskInfo()
    } catch (e) {
      migrationStatus.value = 'PendingConfirmation'
      addLog('error', `删除旧源失败: ${e}`, 'log.confirmDeleteFailed', { error: translateError(e) })
    }
  }

  // 即时回滚（秒级，无需数据拷贝）
  async function instantRollback() {
    try {
      migrationStatus.value = 'RollingBack'
      const result = await invoke<DeleteResult>('rollback_migration_instant', { path: confirmSourcePath.value })
      showConfirmDialog.value = false
      migrationStatus.value = 'Idle'
      if (result.fully_deleted) {
        addLog('success', '迁移已回滚，目录已恢复原状', 'log.instantRollbackDone')
      } else {
        addLog('warn', `迁移已回滚，目录已恢复原状，但部分目标文件删除失败（共 ${result.failed_files.length} 个），请手动清理残留`, 'log.instantRollbackDoneWithResidue')
        for (const f of result.failed_files) {
          addLog('warn', f)
        }
      }
      refreshDiskInfo()
    } catch (e) {
      migrationStatus.value = 'PendingConfirmation'
      addLog('error', `回滚失败: ${e}`, 'log.instantRollbackFailed', { error: translateError(e) })
    }
  }

  function addLog(
    level: LogEntry['level'],
    message: string,
    i18nKey?: string,
    params?: Record<string, unknown>,
  ) {
    const now = new Date()
    const timestamp = `${now.getHours().toString().padStart(2, '0')}:${now.getMinutes().toString().padStart(2, '0')}:${now.getSeconds().toString().padStart(2, '0')}`
    logs.value.push({ timestamp, level, message, i18nKey, params })
    if (logs.value.length > 500) {
      logs.value = logs.value.slice(-400)
    }
  }

  // === 帮助与更新 ===

  // 首次启动检测：若用户未看过帮助，则自动弹出。
  // 由 App.vue 在 onMounted 中调用，遵循"软件首次打开要弹出帮助框"约束。
  function initHelpOnFirstLaunch() {
    try {
      const dismissed = localStorage.getItem(HELP_DISMISSED_KEY)
      if (dismissed !== '1') {
        helpVisible.value = true
      }
    } catch {
      // localStorage 不可用时（如 Tauri 沙箱），保守弹出帮助
      helpVisible.value = true
    }
  }

  // 关闭帮助并标记已看过，后续启动不再自动弹出
  function dismissHelp() {
    helpVisible.value = false
    try {
      localStorage.setItem(HELP_DISMISSED_KEY, '1')
    } catch {
      // 忽略存储失败
    }
  }

  // 触发更新检查：打开更新对话框并立即检查
  // 节流：距上次检查不足 UPDATE_CHECK_MIN_INTERVAL 时仅打开对话框，不重复请求
  function triggerUpdateCheck() {
    updateVisible.value = true
    const now = Date.now()
    if (now - lastUpdateCheckAt < UPDATE_CHECK_MIN_INTERVAL && updateInfo.value === null && !updateChecking.value) {
      // 节流期内且无新版本信息：直接展示上次结果（无更新则提示最新）
      return
    }
    void checkForUpdate()
  }

  // 实际调用 updater 插件检查更新
  async function checkForUpdate() {
    updateChecking.value = true
    updateErrorMsg.value = ''
    try {
      const update = await check()
      updateInfo.value = update
      if (!update) {
        addLog('info', i18n.global.t('update.noUpdate'), 'update.noUpdate')
      }
    } catch (e) {
      updateErrorMsg.value = translateError(e)
      addLog('error', i18n.global.t('update.fail') + ': ' + translateError(e), 'update.fail', { error: translateError(e) })
    } finally {
      updateChecking.value = false
      lastUpdateCheckAt = Date.now()
    }
  }

  // 下载并安装更新，显示进度条
  async function downloadAndInstallUpdate() {
    if (!updateInfo.value) return
    updateDownloading.value = true
    updateProgress.value = 0
    updateProgressText.value = ''
    try {
      let total = 0
      let downloaded = 0
      await updateInfo.value.downloadAndInstall((event: { event: string; data?: { chunkLength?: number; contentLength?: number } }) => {
        switch (event.event) {
          case 'Started':
            total = event.data?.contentLength || 0
            break
          case 'Progress':
            downloaded += event.data?.chunkLength || 0
            if (total > 0) {
              const pct = Math.round((downloaded / total) * 100)
              updateProgress.value = pct
              updateProgressText.value = `${pct}% (${(downloaded / 1024 / 1024).toFixed(1)}MB / ${(total / 1024 / 1024).toFixed(1)}MB)`
            }
            break
          case 'Finished':
            updateProgress.value = 100
            updateProgressText.value = i18n.global.t('update.installing')
            break
        }
      })
      // 安装完成，重启应用
      await relaunch()
    } catch (e) {
      updateErrorMsg.value = translateError(e)
      addLog('error', i18n.global.t('update.installFail') + ': ' + translateError(e), 'update.installFail', { error: translateError(e) })
      updateDownloading.value = false
    }
  }

  // 关闭更新对话框（仅在未下载时允许，避免中断安装）
  function closeUpdateDialog() {
    if (updateDownloading.value) return
    updateVisible.value = false
  }

  // === 事件监听 ===
  async function setupListeners() {
    // 扫描进度
    await listen('scan-progress', (event: any) => {
      const data = event.payload as any
      scanDetail.value = data.detail_key
        ? i18n.global.t(data.detail_key)
        : (data.detail || '')
    })

    // 扫描结果（C盘根目录）
    await listen('scan-result', (event: any) => {
      const data = event.payload as any
      treeNodes.value = data.nodes || []
      migrationStatus.value = 'Idle'
      scanDetail.value = ''
      const foundCount = data.count || treeNodes.value.length
      addLog('success', `扫描完成，发现 ${foundCount} 个目录`, 'log.scanDone', { count: foundCount })
    })

    // 子目录扫描结果
    await listen('subdir-result', (event: any) => {
      const data = event.payload as any
      const parentId = data.parent_id as number
      const children: TreeNode[] = data.nodes || []

      // 找到父节点位置，在其后插入子节点
      const parentIdx = treeNodes.value.findIndex(n => n.id === parentId)
      if (parentIdx !== -1) {
        treeNodes.value.splice(parentIdx + 1, 0, ...children)
      }
    })

    // 节点大小异步更新（后端算完后逐个推送，不阻塞展开）
    await listen('node-size-update', (event: any) => {
      const data = event.payload as any
      const path = data.path as string
      const sizeBytes = data.size_bytes as number
      const sizeText = data.size_text as string
      // 按 path 匹配节点，更新大小
      const node = treeNodes.value.find(n => n.path === path)
      if (node) {
        node.actual_size_bytes = sizeBytes
        node.size_text = sizeText
      }
    })

    // 大目录排行榜扫描进度
    await listen('large-dirs-progress', (event: any) => {
      const data = event.payload as any
      if (data.detail_key) {
        addLog('info', i18n.global.t(data.detail_key), data.detail_key)
      }
    })

    // 大目录排行榜扫描结果
    await listen('large-dirs-result', (event: any) => {
      const data = event.payload as any
      largeDirs.value = (data.dirs || []) as LargeDirEntry[]
      largeDirsScanning.value = false
      addLog('success', `大目录扫描完成，发现 ${data.count || 0} 个大目录`, 'log.scanDone', { count: data.count || 0 })
    })

    // 迁移进度（结构化 MigrationProgress）
    let lastLogDetail = ''
    let lastLogStage = ''
    await listen('migration-progress', (event: any) => {
      const data = event.payload as any
      // 结构化字段
      if (typeof data.stage === 'string') {
        migrationStage.value = data.stage
      }
      if (typeof data.progress === 'number') {
        migrationPercent.value = data.progress
      }
      if (typeof data.total_files === 'number') {
        migrationTotalFiles.value = data.total_files
      }
      if (typeof data.copied_files === 'number') {
        migrationCopiedFiles.value = data.copied_files
      }
      if (typeof data.total_size === 'number') {
        migrationTotalSize.value = data.total_size
      }
      if (typeof data.copied_size === 'number') {
        migrationCopiedSize.value = data.copied_size
      }
      if (typeof data.current_file === 'string') {
        migrationCurrentFile.value = data.current_file
      }
      if (typeof data.detail === 'string') {
        // 优先用 i18n key 翻译（进度条标签），无 key 时回退到后端原文
        migrationDetail.value = data.detail_key
          ? i18n.global.t(data.detail_key, data.detail_params || {})
          : data.detail
      }
      // 兼容字段：后端在 Starting 阶段会额外附带 current_item/total_items/folder
      if (typeof data.current_item === 'number') {
        migrationCurrentItem.value = data.current_item
      }
      if (typeof data.total_items === 'number') {
        migrationTotalItems.value = data.total_items
      }
      if (typeof data.folder === 'string') {
        migrationFolder.value = data.folder
      }
      // 旧协议兼容：status 字段（若存在）
      if (data.status) {
        migrationStatus.value = data.status as MigrationStatus
      }
      // 只在 detail 或 stage 变化时写日志，避免重复刷屏
      const stage = data.stage || ''
      const detail = data.detail || ''
      if (detail && (detail !== lastLogDetail || stage !== lastLogStage)) {
        addLog('info', detail, data.detail_key, data.detail_params)
        lastLogDetail = detail
        lastLogStage = stage
      }
      // 检测 PendingConfirmation 阶段：迁移完成，等待用户确认
      if (stage === 'PendingConfirmation') {
        migrationStatus.value = 'PendingConfirmation'
        confirmSourcePath.value = data.source_path || ''
        confirmOldPath.value = data.renamed_source_path || ''
        confirmTargetPath.value = data.final_target_path || ''
        showConfirmDialog.value = true
      }
    })

    // 迁移完成
    await listen('migration-done', (event: any) => {
      const data = event.payload as any
      // 重置结构化进度字段
      migrationStage.value = ''
      migrationTotalFiles.value = 0
      migrationCopiedFiles.value = 0
      migrationTotalSize.value = 0
      migrationCopiedSize.value = 0
      migrationCurrentFile.value = ''
      if (data.status === 'Failed') {
        migrationStatus.value = 'Idle'
        migrationPercent.value = 0
        // 后端 detail 为动态内容，优先透传；无 detail 时用 i18n
        if (data.detail) {
          addLog('error', data.detail, data.detail_key, data.detail_params)
        } else {
          addLog('error', '迁移失败', 'log.migrateFailedShort')
        }
      } else {
        migrationStatus.value = 'Idle'
        migrationPercent.value = 0
        addLog('success', '全部迁移完成！', 'log.migrateAllDone')
        refreshDiskInfo()
      }
    })

    // 迁移单项错误
    await listen('migration-error', (event: any) => {
      const data = event.payload as any
      addLog('error', `迁移失败 [${data.path}]: ${data.error}`, 'log.migrateItemFailed', { path: data.path, error: translateError(data.error) })
    })
  }

  return {
    isAdmin, diskInfo, treeNodes, targetPath, manualSourcePath, fastMode,
    migrationStatus, migrationPercent, migrationDetail,
    migrationCurrentItem, migrationTotalItems, migrationFolder,
    migrationStage, migrationTotalFiles, migrationCopiedFiles,
    migrationTotalSize, migrationCopiedSize, migrationCurrentFile,
    lockingProcesses, showLockDialog,
    logs, showWarningDialog, warningPaths, crashRecoveryMsg, crashRecoveryStage,
    scanDetail,
    showConfirmDialog, confirmSourcePath, confirmOldPath, confirmTargetPath,
    helpVisible, updateVisible, updateInfo, updateChecking,
    updateDownloading, updateProgress, updateProgressText, updateErrorMsg,
    largeDirs, largeDirsScanning,
    migrationHistory, historyLoading,
    selectedNodes, selectedSafeNodes, selectedWarningNodes,
    totalSelectedSize, canMigrate,
    formatSize,
    checkAdmin, elevateSelf, refreshDiskInfo, scanDisk,
    scanLargeDirs, selectLargeDir,
    loadMigrationHistory, restoreFromArchive, rebuildArchiveMeta, rebuildJunction,
    toggleNodeExpand, toggleNodeSelect,
    startMigration, doMigration, checkCrashRecovery, rollbackJournal, confirmJournalComplete,
    killLockingProcessesAndContinue, cancelMigrationDueToLocks,
    confirmAndDeleteSource, instantRollback,
    initHelpOnFirstLaunch, dismissHelp,
    triggerUpdateCheck, checkForUpdate, downloadAndInstallUpdate, closeUpdateDialog,
    addLog, setupListeners,
  }
})
