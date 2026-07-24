<template>
  <div class="target-selector">
    <div class="selector-header">
      <span class="selector-title">{{ t('target.title') }}</span>
    </div>
    <div class="selector-body">
      <div class="target-path-row">
        <n-input
          v-model:value="store.targetPath"
          :placeholder="t('target.placeholder')"
          size="small"
          clearable
        />
        <n-button size="small" @click="browseFolder" quaternary>
          {{ t('common.browse') }}
        </n-button>
      </div>
      <div class="drive-shortcuts">
        <n-tag
          v-for="d in drives"
          :key="d"
          :type="store.targetPath.startsWith(d + ':') ? 'primary' : 'default'"
          size="small"
          style="cursor: pointer"
          @click="selectDrive(d)"
        >
          {{ t('target.driveLabel', { drive: d }) }}
        </n-tag>
      </div>
      <div v-if="targetInfo" class="target-info">
        <span>{{ t('target.free2') }}: <strong class="highlight">{{ targetInfo.free }}</strong></span>
        <span>{{ t('target.total2') }}: {{ targetInfo.total }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

const { t } = useI18n()
const store = useAppStore()
const drives = ['D', 'E', 'F', 'G']
const targetInfo = ref<{ total: string; free: string } | null>(null)

function selectDrive(d: string) {
  store.targetPath = `${d}:\\CDiskLinker_Moved`
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
  } catch (e) {
    // 用户取消选择，忽略
  }
}

async function loadTargetInfo() {
  // 从 targetPath 提取盘符
  const match = store.targetPath.match(/^([A-Za-z]):/)
  if (!match) {
    targetInfo.value = null
    return
  }
  try {
    const info = await invoke<{ drive: string; total: string; free: string; used_percent: number }>('get_disk_info', { drive: match[1] })
    targetInfo.value = info
  } catch {
    targetInfo.value = null
  }
}

watch(() => store.targetPath, () => {
  loadTargetInfo()
}, { immediate: true })
</script>

<style scoped>
.target-selector {
  background: var(--bg-secondary);
  border-radius: 8px;
  padding: 14px 16px;
  flex: 1;
  min-width: 220px;
}

.selector-header {
  margin-bottom: 10px;
}

.selector-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.selector-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.target-path-row {
  display: flex;
  gap: 4px;
}

.drive-shortcuts {
  display: flex;
  gap: 6px;
}

.target-info {
  display: flex;
  gap: 16px;
  font-size: 12px;
  color: var(--text-secondary);
}

.highlight {
  color: var(--success, #51cf66);
}
</style>
