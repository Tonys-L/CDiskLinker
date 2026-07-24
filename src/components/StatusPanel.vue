<template>
  <div class="status-panel">
    <div class="status-header">
      <span class="status-title">迁移操作</span>
    </div>
    <div class="status-body">
      <!-- 手动源路径输入 -->
      <div class="source-input-section">
        <div class="input-label">源目录路径（可手动输入）</div>
        <n-input
          v-model:value="store.manualSourcePath"
          placeholder="例如 C:\Users\你的用户名\Documents\大文件夹"
          size="small"
          clearable
        />
      </div>

      <!-- 已选目录摘要 -->
      <div v-if="store.selectedNodes.length > 0" class="selection-summary">
        <div class="summary-row">
          <span>树中已选</span>
          <span class="summary-value">{{ store.selectedNodes.length }} 个目录</span>
        </div>
        <div class="summary-row">
          <span>可释放空间</span>
          <span class="summary-value highlight">{{ formatSize(store.totalSelectedSize) }}</span>
        </div>
      </div>

      <!-- 当前迁移目标 -->
      <div class="target-summary">
        <div class="summary-row">
          <span>目标路径</span>
          <span class="summary-value">{{ store.targetPath || '未设置' }}</span>
        </div>
      </div>

      <!-- 迁移进度 -->
      <div v-if="store.migrationStatus !== 'Idle'" class="progress-section">
        <div class="progress-header">
          <span class="progress-stage">{{ stageLabel }}</span>
          <span v-if="store.migrationTotalItems > 1" class="progress-items">
            项目 {{ store.migrationCurrentItem }} / {{ store.migrationTotalItems }}
          </span>
        </div>
        <n-progress
          type="line"
          :percentage="Number(store.migrationPercent.toFixed(1))"
          :color="store.migrationStatus === 'RollingBack' ? '#ffd43b' : '#4dabf7'"
          :rail-color="'#1a2738'"
          :height="10"
          :border-radius="5"
          indicator-placement="inside"
        />
        <div v-if="store.migrationDetail" class="progress-detail">{{ store.migrationDetail }}</div>
        <div v-if="store.migrationTotalFiles > 0" class="progress-stats">
          <span class="stat-item">
            文件: {{ store.migrationCopiedFiles }} / {{ store.migrationTotalFiles }}
          </span>
          <span class="stat-item">
            大小: {{ formatSize(store.migrationCopiedSize) }} / {{ formatSize(store.migrationTotalSize) }}
          </span>
        </div>
        <div v-if="store.migrationCurrentFile" class="progress-current-file" :title="store.migrationCurrentFile">
          当前: {{ store.migrationCurrentFile }}
        </div>
      </div>

      <!-- 操作按钮 -->
      <div class="action-buttons">
        <n-button
          type="primary"
          :disabled="!canStartMigration"
          :loading="store.migrationStatus === 'Copying'"
          @click="store.startMigration()"
          block
        >
          {{ store.migrationStatus === 'Copying' ? '迁移中...' : '开始迁移' }}
        </n-button>
      </div>
    </div>

    <!-- 文件占用检测对话框 -->
    <n-modal
      :show="store.showLockDialog"
      preset="card"
      title="检测到文件占用"
      style="width: 480px; max-width: 90vw;"
      :mask-closable="false"
      :close-on-esc="false"
    >
      <div class="lock-dialog-body">
        <p class="lock-tip">以下进程正在占用待迁移的源目录，需关闭后才能继续迁移：</p>
        <div class="lock-list">
          <div v-for="p in store.lockingProcesses" :key="p.pid" class="lock-item">
            <span class="lock-name">{{ p.name }}</span>
            <span class="lock-pid">PID: {{ p.pid }}</span>
          </div>
          <div v-if="store.lockingProcesses.length === 0" class="lock-empty">未检测到占用进程</div>
        </div>
      </div>
      <template #footer>
        <div class="lock-dialog-footer">
          <n-button @click="store.cancelMigrationDueToLocks()">取消迁移</n-button>
          <n-button type="error" @click="store.killLockingProcessesAndContinue()">关闭进程继续</n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useAppStore } from '../stores/app'

const store = useAppStore()

const canStartMigration = computed(() => {
  const hasSource = store.selectedNodes.length > 0 || store.manualSourcePath.trim().length > 0
  const hasTarget = store.targetPath.trim().length > 0
  return hasSource && hasTarget && store.migrationStatus === 'Idle'
})

// 后端 stage -> 中文标签
const STAGE_LABELS: Record<string, string> = {
  Starting: '开始',
  PreScanning: '预统计',
  PreScanned: '统计完成',
  Copying: '复制中',
  Verifying: '校验中',
  Deleting: '删除源目录',
  Renaming: '重命名',
  Linking: '创建链接',
  Done: '完成',
  RollingBack: '回滚中',
  Idle: '待机',
}

const stageLabel = computed(() => {
  const s = store.migrationStage
  if (!s) {
    // 没有 stage 时退化为旧状态文案
    return store.migrationStatus === 'RollingBack' ? '回滚中' : '迁移中'
  }
  return STAGE_LABELS[s] || s
})

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return (bytes / (1024 * 1024 * 1024)).toFixed(2) + ' GB'
  } else if (bytes >= 1024 * 1024) {
    return (bytes / (1024 * 1024)).toFixed(2) + ' MB'
  } else if (bytes >= 1024) {
    return (bytes / 1024).toFixed(2) + ' KB'
  }
  return bytes + ' Bytes'
}
</script>

<style scoped>
.status-panel {
  background: var(--bg-secondary);
  border-radius: 8px;
  padding: 14px 16px;
  flex: 1;
  min-width: 220px;
}

.status-header {
  margin-bottom: 10px;
}

.status-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.status-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.source-input-section {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.input-label {
  font-size: 11px;
  color: var(--text-secondary);
}

.selection-summary, .target-summary {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px;
  background: var(--bg-tertiary, #243447);
  border-radius: 6px;
}

.summary-row {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-secondary);
}

.summary-value {
  font-weight: 600;
  color: var(--text-primary);
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.summary-value.highlight {
  color: var(--success, #51cf66);
}

.progress-section {
  margin-top: 4px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.progress-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 2px;
}

.progress-stage {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
}

.progress-items {
  font-size: 11px;
  color: var(--text-secondary);
}

.progress-detail {
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.progress-stats {
  display: flex;
  gap: 12px;
  font-size: 11px;
  color: var(--text-secondary);
}

.stat-item {
  white-space: nowrap;
}

.progress-current-file {
  font-size: 11px;
  color: var(--text-tertiary, #8a94a6);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.action-buttons {
  display: flex;
  gap: 8px;
}

.lock-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.lock-tip {
  margin: 0;
  font-size: 13px;
  color: var(--text-primary);
}

.lock-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 240px;
  overflow-y: auto;
  padding: 6px 8px;
  background: var(--bg-tertiary, #243447);
  border-radius: 6px;
}

.lock-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 12px;
  color: var(--text-primary);
  padding: 4px 6px;
  background: var(--bg-secondary, #1a2738);
  border-radius: 4px;
}

.lock-name {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-right: 8px;
}

.lock-pid {
  font-size: 11px;
  color: var(--text-secondary);
  flex-shrink: 0;
}

.lock-empty {
  font-size: 12px;
  color: var(--text-secondary);
  text-align: center;
  padding: 12px 0;
}

.lock-dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
