import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

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
  const scanDetail = ref('')
  // 迁移确认对话框：Linked 状态下提示用户测试软件
  const showConfirmDialog = ref(false)
  const confirmSourcePath = ref('')
  const confirmOldPath = ref('')
  const confirmTargetPath = ref('')
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
      addLog('error', `提权失败: ${e}`, 'log.elevateFailed', { error: String(e) })
    }
  }

  async function refreshDiskInfo() {
    try {
      diskInfo.value = await invoke<DiskInfo>('get_disk_info', { drive: 'C' })
    } catch (e) {
      addLog('error', `获取磁盘信息失败: ${e}`, 'log.diskInfoFailed', { error: String(e) })
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
      addLog('error', `扫描失败: ${e}`, 'log.scanFailed', { error: String(e) })
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
        addLog('error', `展开目录失败: ${e}`, 'log.expandFailed', { error: String(e) })
        node.is_expanded = false
      })
    }
  }

  function toggleNodeSelect(nodeId: number) {
    const node = treeNodes.value.find(n => n.id === nodeId)
    if (node && node.rating !== 'Forbidden' && !node.is_junction) {
      node.is_selected = !node.is_selected
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
      })
    } catch (e) {
      migrationStatus.value = 'Idle'
      addLog('error', `迁移失败: ${e}`, 'log.migrateFailed', { error: String(e) })
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
        addLog('error', `关闭占用进程失败 [${p}]: ${e}`, 'log.killLockFailed', { path: p, error: String(e) })
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
      const result = await invoke<{ found: boolean; message: string }>('check_crash_recovery')
      if (result.found) {
        crashRecoveryMsg.value = result.message
        // 后端返回的 message 为动态内容，直接透传
        addLog('warn', result.message)
      }
    } catch (e) {
      addLog('error', `崩溃恢复检查失败: ${e}`, 'log.crashCheckFailed', { error: String(e) })
    }
  }

  async function rollbackJournal() {
    try {
      migrationStatus.value = 'RollingBack'
      const msg = await invoke<string>('rollback_journal')
      migrationStatus.value = 'Idle'
      // 后端返回的回滚结果消息，直接透传
      addLog('success', msg)
    } catch (e) {
      migrationStatus.value = 'Idle'
      addLog('error', `回滚失败: ${e}`, 'log.rollbackFailed', { error: String(e) })
    }
  }

  // 用户确认迁移正常，删除旧源目录
  async function confirmAndDeleteSource() {
    try {
      migrationStatus.value = 'Copying'
      await invoke('confirm_delete_source', { path: confirmSourcePath.value })
      showConfirmDialog.value = false
      migrationStatus.value = 'Idle'
      addLog('success', '旧源目录已删除，迁移完全完成！')
      refreshDiskInfo()
    } catch (e) {
      migrationStatus.value = 'PendingConfirmation'
      addLog('error', `删除旧源失败: ${e}`, 'log.confirmDeleteFailed', { error: String(e) })
    }
  }

  // 即时回滚（秒级，无需数据拷贝）
  async function instantRollback() {
    try {
      migrationStatus.value = 'RollingBack'
      await invoke('rollback_migration_instant', { path: confirmSourcePath.value })
      showConfirmDialog.value = false
      migrationStatus.value = 'Idle'
      addLog('success', '迁移已回滚，目录已恢复原状')
      refreshDiskInfo()
    } catch (e) {
      migrationStatus.value = 'PendingConfirmation'
      addLog('error', `回滚失败: ${e}`, 'log.instantRollbackFailed', { error: String(e) })
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

  // === 事件监听 ===
  async function setupListeners() {
    // 扫描进度
    await listen('scan-progress', (event: any) => {
      const data = event.payload as any
      scanDetail.value = data.detail || ''
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
        migrationDetail.value = data.detail
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
        addLog('info', detail)
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
          addLog('error', data.detail)
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
      addLog('error', `迁移失败 [${data.path}]: ${data.error}`, 'log.migrateItemFailed', { path: data.path, error: String(data.error) })
    })
  }

  return {
    isAdmin, diskInfo, treeNodes, targetPath, manualSourcePath,
    migrationStatus, migrationPercent, migrationDetail,
    migrationCurrentItem, migrationTotalItems, migrationFolder,
    migrationStage, migrationTotalFiles, migrationCopiedFiles,
    migrationTotalSize, migrationCopiedSize, migrationCurrentFile,
    lockingProcesses, showLockDialog,
    logs, showWarningDialog, warningPaths, crashRecoveryMsg,
    scanDetail,
    showConfirmDialog, confirmSourcePath, confirmOldPath, confirmTargetPath,
    selectedNodes, selectedSafeNodes, selectedWarningNodes,
    totalSelectedSize, canMigrate,
    formatSize,
    checkAdmin, elevateSelf, refreshDiskInfo, scanDisk,
    toggleNodeExpand, toggleNodeSelect,
    startMigration, doMigration, checkCrashRecovery, rollbackJournal,
    killLockingProcessesAndContinue, cancelMigrationDueToLocks,
    confirmAndDeleteSource, instantRollback,
    addLog, setupListeners,
  }
})
