<template>
  <div class="large-dir-panel">
    <!-- 顶部：标题 + 扫描按钮 -->
    <div class="panel-header">
      <span class="panel-title">{{ t('largeDir.title') }}</span>
      <div class="panel-actions">
        <n-button
          type="primary"
          size="small"
          :loading="store.largeDirsScanning"
          :disabled="store.largeDirsScanning"
          @click="store.scanLargeDirs()"
        >
          {{ store.largeDirsScanning ? t('largeDir.rescanning') : t('largeDir.scan') }}
        </n-button>
      </div>
    </div>

    <!-- 列表区 -->
    <div class="panel-body" ref="scrollContainer">
      <div
        v-for="(dir, idx) in store.largeDirs"
        :key="dir.path"
        class="dir-item"
        :class="{
          'is-warning': dir.rating === 'Warning',
          'is-forbidden': dir.rating === 'Forbidden',
          'is-selected': isPathSelected(dir.path),
        }"
        :title="dir.path"
      >
        <!-- 排名徽章 -->
        <div class="rank-badge" :class="`rank-${idx + 1}`">
          <template v-if="idx === 0">🥇</template>
          <template v-else-if="idx === 1">🥈</template>
          <template v-else-if="idx === 2">🥉</template>
          <template v-else>{{ idx + 1 }}</template>
        </div>

        <!-- 评级指示条 -->
        <div
          class="rating-bar"
          :class="`rating-${dir.rating.toLowerCase()}`"
        />

        <!-- 文件夹图标 -->
        <span class="folder-icon">📁</span>

        <!-- 路径与名称 -->
        <div class="dir-info">
          <div class="dir-name">{{ dir.name }}</div>
          <div class="dir-path">{{ dir.path }}</div>
        </div>

        <!-- 大小（右对齐高亮） -->
        <span class="dir-size">{{ dir.size_text }}</span>

        <!-- 评级标签 -->
        <n-tag
          v-if="dir.rating === 'Warning'"
          size="tiny"
          type="warning"
          :bordered="false"
        >{{ t('tree.warning') }}</n-tag>
        <n-tag
          v-else-if="dir.rating === 'Forbidden'"
          size="tiny"
          type="error"
          :bordered="false"
        >{{ t('tree.forbidden') }}</n-tag>

        <!-- 设为源目录按钮 -->
        <n-button
          size="tiny"
          :type="isPathSelected(dir.path) ? 'success' : 'default'"
          :disabled="isPathSelected(dir.path)"
          @click="store.selectLargeDir(dir.path)"
        >
          {{ isPathSelected(dir.path) ? t('largeDir.selected') : t('largeDir.select') }}
        </n-button>
      </div>

      <!-- 空状态 -->
      <div v-if="store.largeDirs.length === 0" class="empty-state">
        <template v-if="store.largeDirsScanning">
          <n-spin size="small" />
          <span style="margin-left: 8px">{{ t('largeDir.rescanning') }}</span>
        </template>
        <template v-else>
          {{ t('largeDir.empty') }}
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()

// 判断该路径是否已设为源目录（手动源路径输入框中的值）
function isPathSelected(path: string): boolean {
  return store.manualSourcePath.trim() === path
}
</script>

<style scoped>
.large-dir-panel {
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
  padding: 6px 0;
}

.dir-item {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 44px;
  padding: 6px 14px 6px 8px;
  position: relative;
  transition: background 0.15s;
}

.dir-item:hover {
  background: var(--bg-tertiary);
}

.dir-item.is-selected {
  background: rgba(77, 171, 247, 0.1);
}

.dir-item.is-warning {
  /* 警告目录通过 tag 标识，不改变整体透明度，便于用户识别 */
}

/* 排名徽章 */
.rank-badge {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  font-size: 16px;
  font-weight: 700;
  color: var(--text-secondary);
  border-radius: 6px;
  background: var(--bg-tertiary, rgba(255, 255, 255, 0.05));
}

/* 4 名以后用数字，字号略小 */
.rank-badge.rank-4,
.rank-badge.rank-5,
.rank-badge.rank-6,
.rank-badge.rank-7,
.rank-badge.rank-8,
.rank-badge.rank-9,
.rank-badge.rank-10,
.rank-badge.rank-11,
.rank-badge.rank-12,
.rank-badge.rank-13,
.rank-badge.rank-14,
.rank-badge.rank-15,
.rank-badge.rank-16,
.rank-badge.rank-17,
.rank-badge.rank-18,
.rank-badge.rank-19,
.rank-badge.rank-20 {
  font-size: 12px;
}

/* 评级指示条 */
.rating-bar {
  position: absolute;
  left: 0;
  top: 6px;
  bottom: 6px;
  width: 3px;
  border-radius: 2px;
}

.rating-safe { background: var(--success, #51cf66); }
.rating-warning { background: var(--warning, #ffd43b); }
.rating-forbidden { background: var(--danger, #ff6b6b); }

.folder-icon {
  font-size: 14px;
  flex-shrink: 0;
}

/* 路径与名称 */
.dir-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.dir-name {
  font-size: 13px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dir-path {
  font-size: 11px;
  color: var(--text-secondary);
  opacity: 0.7;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dir-size {
  font-size: 13px;
  font-weight: 600;
  color: var(--accent, #4dabf7);
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 200px;
  color: var(--text-secondary);
  font-size: 13px;
}
</style>
