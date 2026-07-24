<template>
  <n-modal
    v-model:show="store.showWarningDialog"
    preset="dialog"
    title="迁移风险确认"
    type="warning"
    positive-text="确认迁移"
    negative-text="取消"
    @positive-click="store.doMigration()"
  >
    <p>以下目录属于配置数据区，迁移后可能导致相关软件配置丢失：</p>
    <ul class="warning-list">
      <li v-for="name in store.warningPaths" :key="name">{{ name }}</li>
    </ul>
    <p style="margin-top: 10px; color: var(--text-secondary); font-size: 12px;">
      确认迁移请点击"确认迁移"，这些目录的数据将被物理移至目标盘并建立联接。
    </p>
  </n-modal>
</template>

<script setup lang="ts">
import { useAppStore } from '../stores/app'

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
