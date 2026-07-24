<template>
  <div class="status-panel">
    <div class="status-header">
      <span class="status-title">{{ t('migration.title') }}</span>
    </div>
    <div class="status-body">
      <!-- 手动源路径输入 -->
      <div class="source-input-section">
        <div class="input-label">{{ t('migration.sourceLabel') }}</div>
        <n-input
          v-model:value="store.manualSourcePath"
          :placeholder="t('source.placeholder')"
          size="small"
          clearable
        />
      </div>

      <!-- 已选目录摘要 -->
      <div v-if="store.selectedNodes.length > 0" class="selection-summary">
        <div class="summary-row">
          <span>{{ t('source.title') }}</span>
          <span class="summary-value">{{ t('migration.selectedCount', { count: store.selectedNodes.length }) }}</span>
        </div>
        <div class="summary-row">
          <span>{{ t('migration.canFree') }}</span>
          <span class="summary-value highlight">{{ formatSize(store.totalSelectedSize) }}</span>
        </div>
      </div>

      <!-- 当前迁移目标 -->
      <div class="target-summary">
        <div class="summary-row">
          <span>{{ t('target.path') }}</span>
          <span class="summary-value">{{ store.targetPath || t('target.notSet') }}</span>
        </div>
      </div>

      <!-- 迁移进度 -->
      <div v-if="store.migrationStatus !== 'Idle'" class="progress-section">
        <div class="progress-header">
          <span class="progress-stage">{{ stageLabel }}</span>
          <span v-if="store.migrationTotalItems > 1" class="progress-items">
            {{ t('migration.items', { current: store.migrationCurrentItem, total: store.migrationTotalItems }) }}
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
            {{ t('migration.files', { copied: store.migrationCopiedFiles, total: store.migrationTotalFiles }) }}
          </span>
          <span class="stat-item">
            {{ t('migration.size', { copied: formatSize(store.migrationCopiedSize), total: formatSize(store.migrationTotalSize) }) }}
          </span>
        </div>
        <div v-if="store.migrationCurrentFile" class="progress-current-file" :title="store.migrationCurrentFile">
          {{ t('migration.current', { file: store.migrationCurrentFile }) }}
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
          {{ store.migrationStatus === 'Copying' ? t('migration.migrating') : t('migration.start') }}
        </n-button>
      </div>
    </div>

    <!-- 文件占用检测对话框 -->
    <n-modal
      :show="store.showLockDialog"
      preset="card"
      :title="t('migration.lockTitle')"
      style="width: 480px; max-width: 90vw;"
      :mask-closable="false"
      :close-on-esc="false"
    >
      <div class="lock-dialog-body">
        <p class="lock-tip">{{ t('migration.lockTip') }}</p>
        <div class="lock-list">
          <div v-for="p in store.lockingProcesses" :key="p.pid" class="lock-item">
            <span class="lock-name">{{ p.name }}</span>
            <span class="lock-pid">{{ t('migration.lockPid', { pid: p.pid }) }}</span>
          </div>
          <div v-if="store.lockingProcesses.length === 0" class="lock-empty">{{ t('migration.lockEmpty') }}</div>
        </div>
      </div>
      <template #footer>
        <div class="lock-dialog-footer">
          <n-button @click="store.cancelMigrationDueToLocks()">{{ t('migration.cancelLock') }}</n-button>
          <n-button type="error" @click="store.killLockingProcessesAndContinue()">{{ t('migration.killLock') }}</n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()

const canStartMigration = computed(() => {
  const hasSource = store.selectedNodes.length > 0 || store.manualSourcePath.trim().length > 0
  const hasTarget = store.targetPath.trim().length > 0
  return hasSource && hasTarget && store.migrationStatus === 'Idle'
})

// 后端 stage -> i18n key 映射
const STAGE_I18N_KEYS: Record<string, string> = {
  Starting: 'migration.stages.Starting',
  PreScanning: 'migration.stages.PreScanning',
  PreScanned: 'migration.stages.PreScanned',
  Copying: 'migration.stages.Copying',
  Verifying: 'migration.stages.Verifying',
  Deleting: 'migration.stages.Deleting',
  Renaming: 'migration.stages.Renaming',
  Linking: 'migration.stages.Linking',
  Done: 'migration.stages.Done',
  RollingBack: 'migration.stages.RollingBack',
  Idle: 'migration.stages.Idle',
}

const stageLabel = computed(() => {
  const s = store.migrationStage
  if (!s) {
    // 没有 stage 时退化为旧状态文案
    return store.migrationStatus === 'RollingBack' ? t('migration.fallbackRollback') : t('migration.fallbackMigrating')
  }
  const key = STAGE_I18N_KEYS[s]
  return key ? t(key) : s
})

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return (bytes / (1024 * 1024 * 1024)).toFixed(2) + ' GB'
  } else if (bytes >= 1024 * 1024) {
    return (bytes / (1024 * 1024)).toFixed(2) + ' MB'
  } else if (bytes >= 1024) {
    return (bytes / (1024)).toFixed(2) + ' KB'
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
