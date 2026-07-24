<template>
  <n-modal
    v-model:show="store.showWarningDialog"
    preset="dialog"
    :title="t('migration.warningTitle')"
    type="warning"
    :positive-text="t('migration.warningPositive')"
    :negative-text="t('migration.warningNegative')"
    @positive-click="store.doMigration()"
  >
    <p>{{ t('migration.warningBody') }}</p>
    <ul class="warning-list">
      <li v-for="name in store.warningPaths" :key="name">{{ name }}</li>
    </ul>
    <p style="margin-top: 10px; color: var(--text-secondary); font-size: 12px;">
      {{ t('migration.warningConfirm') }}
    </p>
  </n-modal>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()
</script>

<style scoped>
.warning-list {
  margin: 8px 0;
  padding-left: 20px;
  color: var(--warning, #ffd43b);
}

.warning-list li {
  margin: 4px 0;
  font-size: 13px;
}
</style>
