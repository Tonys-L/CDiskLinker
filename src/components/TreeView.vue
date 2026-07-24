<template>
  <div class="tree-view">
    <div class="tree-header">
      <span class="tree-title">{{ t('tree.title') }}</span>
      <div class="tree-actions">
        <n-button type="primary" size="small" :loading="store.migrationStatus === 'Scanning'" @click="store.scanDisk()">
          {{ store.treeNodes.length > 0 ? t('tree.rescan') : t('tree.scan') }}
        </n-button>
      </div>
    </div>
    <div class="tree-body" ref="scrollContainer">
      <div
        v-for="node in store.treeNodes"
        :key="node.id"
        class="tree-item"
        :class="{
          'is-forbidden': node.rating === 'Forbidden',
          'is-warning': node.rating === 'Warning',
          'is-selected': node.is_selected,
          'is-junction': node.is_junction,
        }"
        :style="{ paddingLeft: 8 + node.level * 20 + 'px' }"
        @click="onNodeClick(node)"
      >
        <!-- 评级指示条 -->
        <div
          class="rating-bar"
          :class="`rating-${node.rating.toLowerCase()}`"
        />
        <!-- 展开按钮 -->
        <div
          v-if="node.has_children"
          class="expand-btn"
          :class="{ expanded: node.is_expanded }"
          @click.stop="store.toggleNodeExpand(node.id)"
        >
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M3 1L7 5L3 9" fill="currentColor"/></svg>
        </div>
        <div v-else class="expand-placeholder" />
        <!-- 勾选框 -->
        <n-checkbox
          :checked="node.is_selected"
          :disabled="node.rating === 'Forbidden' || node.is_junction"
          size="small"
          @update:checked="store.toggleNodeSelect(node.id)"
          @click.stop
        />
        <!-- 文件夹图标 -->
        <span class="folder-icon" :class="{ junction: node.is_junction }">
          {{ node.is_junction ? '🔗' : '📁' }}
        </span>
        <!-- 名称与大小 -->
        <span class="node-name">{{ node.name }}</span>
        <span class="node-size">{{ node.rating === 'Forbidden' ? '—' : (node.size_text || t('tree.calculating')) }}</span>
        <!-- 评级标签 -->
        <n-tag
          v-if="node.rating === 'Warning'"
          size="tiny"
          type="warning"
          :bordered="false"
        >{{ t('tree.warning') }}</n-tag>
        <n-tag
          v-if="node.rating === 'Forbidden'"
          size="tiny"
          type="error"
          :bordered="false"
        >{{ t('tree.forbidden') }}</n-tag>
        <n-tag
          v-if="node.is_junction"
          size="tiny"
          type="info"
          :bordered="false"
        >{{ t('tree.migrated') }}</n-tag>
      </div>
      <div v-if="store.treeNodes.length === 0" class="tree-empty">
        <template v-if="store.migrationStatus === 'Scanning'">
          <n-spin size="small" />
          <span style="margin-left: 8px">{{ store.scanDetail || t('tree.scanning') }}</span>
        </template>
        <template v-else>
          {{ t('tree.empty') }}
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'
import type { TreeNode } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()

function onNodeClick(node: TreeNode) {
  if (node.has_children) {
    store.toggleNodeExpand(node.id)
  }
}
</script>

<style scoped>
.tree-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.tree-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.tree-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.tree-body {
  flex: 1;
  overflow-y: auto;
  padding: 6px 0;
}

.tree-item {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 38px;
  padding-right: 14px;
  cursor: pointer;
  position: relative;
  transition: background 0.15s;
}

.tree-item:hover {
  background: var(--bg-tertiary);
}

.tree-item.is-selected {
  background: rgba(77, 171, 247, 0.1);
}

.tree-item.is-forbidden {
  opacity: 0.5;
}

.tree-item.is-junction {
  opacity: 0.6;
}

.rating-bar {
  position: absolute;
  left: 0;
  top: 4px;
  bottom: 4px;
  width: 3px;
  border-radius: 2px;
}

.rating-safe { background: var(--success, #51cf66); }
.rating-warning { background: var(--warning, #ffd43b); }
.rating-forbidden { background: var(--danger, #ff6b6b); }

.expand-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  flex-shrink: 0;
  color: var(--text-secondary);
  transition: transform 0.2s;
}

.expand-btn.expanded {
  transform: rotate(90deg);
}

.expand-placeholder {
  width: 18px;
  flex-shrink: 0;
}

.folder-icon {
  font-size: 14px;
  flex-shrink: 0;
}

.node-name {
  flex: 1;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary);
}

.node-size {
  font-size: 12px;
  color: var(--text-secondary);
  flex-shrink: 0;
}

.tree-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 200px;
  color: var(--text-secondary);
  font-size: 13px;
}
</style>
