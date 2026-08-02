<template>
  <div class="app-root">
    <!-- 语言切换按钮 -->
    <n-button
      class="lang-toggle"
      size="small"
      secondary
      @click="toggleLocale"
      :title="locale === 'zh-CN' ? t('lang.en') : t('lang.zh')"
    >
      {{ t('lang.switch') }}
    </n-button>

    <!-- 左侧：目录树 -->
    <aside class="left-panel">
      <TreeView />
    </aside>

    <!-- 右侧：操作面板 -->
    <main class="right-panel">
      <!-- 顶部：C盘概览 -->
      <header class="top-bar">
        <DiskOverview />
      </header>

      <!-- 中间：源→目标→操作 流程 -->
      <div class="right-content">
        <!-- 源目录 -->
        <section class="panel-section">
          <div class="section-title">
            <span class="section-icon">📂</span> {{ t('source.title') }}
          </div>
          <div class="section-body">
            <n-input
              v-model:value="store.manualSourcePath"
              :placeholder="t('source.placeholder')"
              size="small"
              clearable
            />
            <div v-if="store.selectedNodes.length > 0" class="source-summary">
              <span>{{ t('source.selectedPrefix') }}<strong>{{ store.selectedNodes.length }}</strong>{{ t('source.selectedSuffix') }}</span>
              <span class="sep">·</span>
              <span class="highlight">{{ t('source.canFree', { size: formatSize(store.totalSelectedSize) }) }}</span>
            </div>
            <div v-else class="source-hint">{{ t('source.hint') }}</div>
          </div>
        </section>

        <!-- 迁移目标 -->
        <section class="panel-section">
          <div class="section-title">
            <span class="section-icon">📥</span> {{ t('target.title') }}
          </div>
          <div class="section-body">
            <div class="target-input-row">
              <n-input
                v-model:value="store.targetPath"
                :placeholder="t('target.placeholder')"
                size="small"
                clearable
              />
              <n-button size="small" @click="browseFolder">
                {{ t('common.browse') }}
              </n-button>
            </div>
            <div v-if="targetInfo" class="target-info">
              <span>{{ t('target.freePrefix') }}<strong class="highlight">{{ targetInfo.free }}</strong>{{ t('target.freeSuffix', { total: targetInfo.total }) }}</span>
            </div>
          </div>
        </section>

        <!-- 操作区 -->
        <section class="panel-section action-section">
          <div v-if="store.migrationStatus !== 'Idle'" class="progress-area">
            <div class="progress-label">{{ store.migrationDetail }}</div>
            <n-progress
              type="line"
              :percentage="Number(store.migrationPercent.toFixed(1))"
              :color="store.migrationStatus === 'RollingBack' ? '#ffd43b' : '#4dabf7'"
              :rail-color="'#1a2738'"
              :height="10"
              :border-radius="5"
              indicator-placement="inside"
            />
          </div>
          <div class="fast-mode-row">
            <n-switch v-model:value="store.fastMode" :disabled="store.migrationStatus !== 'Idle'" />
            <span class="fast-mode-label">{{ t('migration.fastMode') }}</span>
            <n-tooltip trigger="hover">
              <template #trigger>
                <span class="fast-mode-help">?</span>
              </template>
              {{ t('migration.fastModeTip') }}
            </n-tooltip>
          </div>
          <n-button
            type="primary"
            :disabled="!canMigrate"
            :loading="store.migrationStatus === 'Copying'"
            @click="store.startMigration()"
            block
            size="large"
          >
            {{ store.migrationStatus === 'Copying' ? t('migration.migrating') : t('migration.start') }}
          </n-button>
          <div class="migration-hint">
            {{ t('migration.hint') }}
          </div>
        </section>

        <!-- 崩溃恢复 -->
        <JournalBar />

        <!-- 运行日志 -->
        <LogConsole />
      </div>
    </main>

    <!-- 警告弹窗 -->
    <WarningDialog />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { useAppStore } from '../stores/app'
import { toggleLocale } from '../i18n'
import DiskOverview from '../components/DiskOverview.vue'
import TreeView from '../components/TreeView.vue'
import JournalBar from '../components/JournalBar.vue'
import LogConsole from '../components/LogConsole.vue'
import WarningDialog from '../components/WarningDialog.vue'

const { t, locale } = useI18n()
const store = useAppStore()
const targetInfo = ref<{ total: string; free: string } | null>(null)

const canMigrate = computed(() => {
  const hasSource = store.selectedNodes.length > 0 || store.manualSourcePath.trim().length > 0
  const hasTarget = store.targetPath.trim().length > 0
  return hasSource && hasTarget && store.migrationStatus === 'Idle'
})

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return (bytes / (1024 * 1024 * 1024)).toFixed(2) + ' GB'
  if (bytes >= 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(2) + ' MB'
  if (bytes >= 1024) return (bytes / (1024)).toFixed(2) + ' KB'
  return bytes + ' Bytes'
}

async function browseFolder() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t('target.selectFolder'),
    })
    if (selected && typeof selected === 'string') {
      store.targetPath = selected
    }
  } catch {
    // 用户取消
  }
}

async function loadTargetInfo() {
  const match = store.targetPath.match(/^([A-Za-z]):/)
  if (!match) { targetInfo.value = null; return }
  try {
    const info = await invoke<{ drive: string; total: string; free: string; used_percent: number }>('get_disk_info', { drive: match[1] })
    targetInfo.value = info
  } catch { targetInfo.value = null }
}

watch(() => store.targetPath, () => { loadTargetInfo() }, { immediate: true })

// 初始化
import { onMounted } from 'vue'
onMounted(async () => {
  await store.checkAdmin()
  await store.refreshDiskInfo()
  await store.setupListeners()
  await store.checkCrashRecovery()
})
</script>

<style scoped>
.app-root {
  display: flex;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: var(--bg-primary);
  position: relative;
}

.lang-toggle {
  position: fixed;
  top: 12px;
  right: 16px;
  z-index: 100;
  font-size: 13px;
  padding: 0 14px;
  opacity: 1;
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.15));
}
.lang-toggle:hover {
  opacity: 1;
  border-color: var(--accent, #4dabf7);
}

.left-panel {
  width: 480px;
  min-width: 360px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
}

.right-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--bg-primary);
}

.top-bar {
  padding: 12px 16px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.right-content {
  flex: 1;
  padding: 16px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.panel-section {
  background: var(--bg-secondary);
  border-radius: 8px;
  padding: 14px 16px;
}

.section-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 10px;
  display: flex;
  align-items: center;
  gap: 6px;
}

.section-icon {
  font-size: 14px;
}

.section-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.target-input-row {
  display: flex;
  gap: 6px;
}

.source-summary {
  font-size: 12px;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  gap: 4px;
}

.source-summary strong {
  color: var(--text-primary);
}

.sep {
  color: var(--text-secondary);
  opacity: 0.5;
}

.highlight {
  color: var(--success, #51cf66);
  font-weight: 600;
}

.source-hint {
  font-size: 12px;
  color: var(--text-secondary);
  opacity: 0.7;
}

.target-info {
  font-size: 12px;
  color: var(--text-secondary);
}

.action-section {
  background: var(--bg-tertiary, #1a2738);
}

.progress-area {
  margin-bottom: 10px;
}

.fast-mode-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
  font-size: 12px;
  color: var(--text-secondary);
}

.fast-mode-label {
  flex-shrink: 0;
}

.fast-mode-help {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.1);
  font-size: 11px;
  cursor: help;
}

.progress-label {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.migration-hint {
  font-size: 11px;
  color: var(--text-secondary);
  opacity: 0.6;
  text-align: center;
  margin-top: 6px;
}
</style>
