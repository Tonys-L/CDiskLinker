<template>
  <div class="history-panel">
    <!-- 顶部：标题 + 刷新按钮 -->
    <div class="panel-header">
      <span class="panel-title">{{ t('history.title') }}</span>
      <div class="panel-actions">
        <n-button
          size="small"
          :loading="store.historyLoading"
          :disabled="store.historyLoading"
          @click="store.loadMigrationHistory()"
        >
          {{ t('common.refresh') }}
        </n-button>
      </div>
    </div>

    <!-- 列表区 -->
    <div class="panel-body">
      <n-spin :show="store.historyLoading">
        <!-- 空状态 -->
        <div
          v-if="!store.historyLoading && store.migrationHistory.length === 0"
          class="empty-state"
        >
          <n-empty :description="t('history.empty')" />
        </div>

        <!-- 档案列表 -->
        <n-list v-else hoverable clickable>
          <n-list-item v-for="item in store.migrationHistory" :key="item.archive_id">
            <n-thing>
              <template #header>
                <div class="item-header">
                  <span class="item-source" :title="item.source_path">{{ item.source_path }}</span>
                  <n-tooltip v-if="!item.meta_file_exists" trigger="hover">
                    <template #trigger>
                      <n-tag size="small" type="warning" :bordered="false">
                        {{ t('history.metaMissing') }}
                      </n-tag>
                    </template>
                    {{ t('history.metaMissingTip') }}
                  </n-tooltip>
                  <n-tooltip v-if="item.meta_file_exists && !item.junction_exists" trigger="hover">
                    <template #trigger>
                      <n-tag size="small" type="warning" :bordered="false">
                        {{ t('history.junctionMissing') }}
                      </n-tag>
                    </template>
                    {{ t('history.junctionMissingTip') }}
                  </n-tooltip>
                </div>
              </template>
              <template #description>
                <div class="item-desc">
                  <div class="desc-row">
                    <span class="desc-label">{{ t('history.target') }}:</span>
                    <span class="desc-value" :title="item.target_path">{{ item.target_path }}</span>
                  </div>
                  <div class="desc-row">
                    <span class="desc-label">{{ t('history.migratedAt') }}:</span>
                    <span class="desc-value">{{ formatTime(item.created_at) }}</span>
                  </div>
                  <div class="desc-row">
                    <span class="desc-label">{{ t('history.totalSize') }}:</span>
                    <span class="desc-value">{{ store.formatSize(item.total_size) }}</span>
                    <span class="desc-sep">·</span>
                    <span class="desc-label">{{ t('history.totalFiles') }}:</span>
                    <span class="desc-value">{{ item.total_files }} {{ t('history.files') }}</span>
                  </div>
                </div>
              </template>
              <template #action>
                <div class="item-actions">
                  <n-button
                    size="small"
                    type="primary"
                    :disabled="store.migrationStatus !== 'Idle'"
                    @click="onRestoreClick(item)"
                  >
                    {{ t('history.restore') }}
                  </n-button>
                  <n-button
                    v-if="!item.meta_file_exists"
                    size="small"
                    :disabled="store.migrationStatus !== 'Idle'"
                    @click="onRebuildClick(item)"
                  >
                    {{ t('history.rebuild') }}
                  </n-button>
                  <n-button
                    v-if="item.meta_file_exists && !item.junction_exists"
                    size="small"
                    type="info"
                    :disabled="store.migrationStatus !== 'Idle'"
                    @click="onRebuildJunctionClick(item)"
                  >
                    {{ t('history.rebuildJunction') }}
                  </n-button>
                </div>
              </template>
            </n-thing>
          </n-list-item>
        </n-list>
      </n-spin>
    </div>

    <!-- 恢复确认对话框 -->
    <n-modal
      v-model:show="showRestoreConfirm"
      preset="dialog"
      :title="t('history.restoreConfirmTitle')"
      type="warning"
      :positive-text="t('common.confirm')"
      :negative-text="t('common.cancel')"
      @positive-click="confirmRestore"
    >
      <p>{{ t('history.restoreConfirm') }}</p>
      <ul class="confirm-warnings">
        <li>{{ t('history.restoreWarning1') }}</li>
        <li>{{ t('history.restoreWarning2') }}</li>
        <li>{{ t('history.restoreWarning3') }}</li>
      </ul>
      <div class="confirm-info">
        <div class="confirm-row">
          <span class="confirm-label">{{ t('history.source') }}:</span>
          <span class="confirm-value">{{ pendingRestore?.source_path }}</span>
        </div>
        <div class="confirm-row">
          <span class="confirm-label">{{ t('history.target') }}:</span>
          <span class="confirm-value">{{ pendingRestore?.target_path }}</span>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore, type ArchiveListItem } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()

// 待恢复的档案项（确认对话框中暂存）
const pendingRestore = ref<ArchiveListItem | null>(null)
const showRestoreConfirm = ref(false)

// Unix 秒 → 本地日期字符串
function formatTime(unixSeconds: number): string {
  if (!unixSeconds || unixSeconds <= 0) return '-'
  return new Date(unixSeconds * 1000).toLocaleString()
}

function onRestoreClick(item: ArchiveListItem) {
  pendingRestore.value = item
  showRestoreConfirm.value = true
}

async function confirmRestore() {
  const item = pendingRestore.value
  if (!item) return
  const archiveId = item.archive_id
  pendingRestore.value = null
  await store.restoreFromArchive(archiveId)
}

async function onRebuildClick(item: ArchiveListItem) {
  await store.rebuildArchiveMeta(item.archive_id)
}

async function onRebuildJunctionClick(item: ArchiveListItem) {
  await store.rebuildJunction(item.archive_id)
}

// 组件挂载时自动加载迁移历史
onMounted(() => {
  store.loadMigrationHistory()
})
</script>

<style scoped>
.history-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.panel-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 240px;
}

.item-header {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.item-source {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.item-desc {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin-top: 4px;
}

.desc-row {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  min-width: 0;
}

.desc-label {
  flex-shrink: 0;
  opacity: 0.8;
}

.desc-value {
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desc-sep {
  margin: 0 4px;
  opacity: 0.5;
}

.item-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.confirm-info {
  margin-top: 10px;
  padding: 10px;
  background: var(--bg-tertiary, #243447);
  border-radius: 6px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.confirm-warnings {
  margin: 8px 0 4px;
  padding-left: 20px;
  font-size: 12px;
  color: var(--text-secondary);
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.confirm-warnings li {
  line-height: 1.5;
}

.confirm-row {
  display: flex;
  font-size: 12px;
  gap: 8px;
}

.confirm-label {
  color: var(--text-secondary);
  white-space: nowrap;
}

.confirm-value {
  color: var(--text-primary);
  font-weight: 600;
  word-break: break-all;
}
</style>
