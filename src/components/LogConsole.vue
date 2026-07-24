<template>
  <div class="log-console">
    <div class="log-header">
      <span class="log-title">运行日志</span>
      <n-button size="tiny" quaternary @click="store.logs = []">清空</n-button>
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
        <span class="log-msg">{{ log.message }}</span>
      </div>
      <div v-if="store.logs.length === 0" class="log-empty">暂无日志</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
import { useAppStore } from '../stores/app'

const store = useAppStore()
const logContainer = ref<HTMLElement | null>(null)

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
  width: 42px;
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
