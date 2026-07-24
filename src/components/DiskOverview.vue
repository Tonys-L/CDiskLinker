<template>
  <div class="disk-overview">
    <div class="disk-info">
      <div class="disk-title">
        <n-tag :type="store.isAdmin ? 'success' : 'error'" size="small" round>
          {{ store.isAdmin ? '管理员' : '未提权' }}
        </n-tag>
        <span class="disk-label">C盘空间概览</span>
      </div>
      <div v-if="store.diskInfo" class="disk-details">
        <div class="disk-stats">
          <div class="stat">
            <span class="stat-label">总容量</span>
            <span class="stat-value">{{ store.diskInfo.total }}</span>
          </div>
          <div class="stat">
            <span class="stat-label">可用空间</span>
            <span class="stat-value highlight">{{ store.diskInfo.free }}</span>
          </div>
          <div class="stat">
            <span class="stat-label">已使用</span>
            <span class="stat-value warn">{{ (store.diskInfo.used_percent * 100).toFixed(1) }}%</span>
          </div>
        </div>
        <n-progress
          type="line"
          :percentage="Number((store.diskInfo.used_percent * 100).toFixed(1))"
          :color="store.diskInfo.used_percent > 0.85 ? '#ff6b6b' : '#4dabf7'"
          :rail-color="'#1a2738'"
          :height="12"
          :border-radius="6"
          indicator-placement="inside"
        />
      </div>
      <div v-else class="disk-loading">加载中...</div>
    </div>
    <div class="actions">
      <n-button v-if="!store.isAdmin" type="warning" size="small" @click="store.elevateSelf()">
        提权运行
      </n-button>
      <n-button type="primary" size="small" @click="store.refreshDiskInfo()">
        刷新
      </n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useAppStore } from '../stores/app'

const store = useAppStore()
</script>

<style scoped>
.disk-overview {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}

.disk-title {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}

.disk-label {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.disk-stats {
  display: flex;
  gap: 24px;
  margin-bottom: 8px;
}

.stat {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.stat-label {
  font-size: 11px;
  color: var(--text-secondary);
}

.stat-value {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.stat-value.highlight {
  color: var(--success, #51cf66);
}

.stat-value.warn {
  color: var(--warning, #ffd43b);
}

.actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
</style>
