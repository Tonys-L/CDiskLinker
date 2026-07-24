<template>
  <div v-if="store.crashRecoveryMsg" class="journal-bar">
    <div class="journal-icon">⚠️</div>
    <div class="journal-content">
      <span class="journal-title">{{ t('journal.title') }}</span>
      <span class="journal-detail">{{ store.crashRecoveryMsg }}</span>
    </div>
    <n-button size="small" type="warning" @click="store.rollbackJournal()">
      {{ t('journal.rollback') }}
    </n-button>
    <n-button size="small" quaternary @click="store.crashRecoveryMsg = ''">
      {{ t('common.close') }}
    </n-button>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()
</script>

<style scoped>
.journal-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  background: rgba(255, 212, 59, 0.1);
  border: 1px solid rgba(255, 212, 59, 0.3);
  border-radius: 8px;
}

.journal-icon {
  font-size: 18px;
  flex-shrink: 0;
}

.journal-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.journal-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--warning, #ffd43b);
}

.journal-detail {
  font-size: 11px;
  color: var(--text-secondary);
}
</style>
