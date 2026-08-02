<template>
  <div class="log-console">
    <div class="log-header">
      <span class="log-title">{{ t('log.title') }}</span>
      <n-button size="tiny" quaternary @click="store.logs = []">{{ t('log.clear') }}</n-button>
    </div>
    <div class="log-body" ref="logContainer">
      <div
        v-for="(log, idx) in store.logs"
        :key="idx"
        class="log-entry"
        :class="`log-${log.level}`"
      >
        <span class="log-time">{{ log.timestamp }}</span>
        <span class="log-level">[{{ log.level.toUpperCase() }}]</span>
        <span class="log-msg">{{ formatLogMessage(log) }}</span>
      </div>
      <div v-if="store.logs.length === 0" class="log-empty">{{ t('log.empty') }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore, type LogEntry } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()
const logContainer = ref<HTMLElement | null>(null)

// 优先用 i18n key 翻译，未命中或无 key 时回退到原始 message
function formatLogMessage(log: LogEntry): string {
  if (log.i18nKey) {
    const translated = t(log.i18nKey, { ...(log.params || {}) })
    // vue-i18n 在 key 未命中时返回 key 字符串本身，此时回退到 message
    if (translated && translated !== log.i18nKey) {
      return translated
    }
  }
  return log.message
}

watch(() => store.logs.length, async () => {
  await nextTick()
  if (logContainer.value) {
    logContainer.value.scrollTop = logContainer.value.scrollHeight
  }
})
</script>

<style scoped>
.log-console {
  background: var(--bg-secondary);
  border-radius: 8px;
  padding: 10px 14px;
  flex: 1;
  min-height: 120px;
  display: flex;
  flex-direction: column;
}

.log-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
  flex-shrink: 0;
}

.log-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.log-body {
  flex: 1;
  overflow-y: auto;
  font-family: 'Cascadia Code', 'Consolas', monospace;
  font-size: 11px;
  line-height: 1.6;
}

.log-entry {
  display: flex;
  gap: 6px;
  padding: 1px 0;
}

.log-time {
  color: var(--text-secondary);
  flex-shrink: 0;
}

.log-level {
  flex-shrink: 0;
  min-width: 75px;
}

.log-info .log-level { color: var(--accent, #4dabf7); }
.log-warn .log-level { color: var(--warning, #ffd43b); }
.log-error .log-level { color: var(--danger, #ff6b6b); }
.log-success .log-level { color: var(--success, #51cf66); }

.log-msg {
  color: var(--text-primary);
  word-break: break-all;
}

.log-empty {
  color: var(--text-secondary);
  text-align: center;
  padding: 20px;
}
</style>
