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
    <!-- 搜索框：过滤已展开的目录节点 -->
    <div class="tree-search">
      <n-input
        v-model:value="searchKeyword"
        :placeholder="t('tree.searchPlaceholder')"
        size="small"
        clearable
      >
        <template #prefix>
          <span class="search-icon">🔍</span>
        </template>
      </n-input>
      <span v-if="searchKeyword.trim() && matchCount > 0" class="search-count">
        {{ t('tree.matchCount', { count: matchCount }) }}
      </span>
    </div>
    <div class="tree-body" ref="scrollContainer">
      <div
        v-for="node in filteredNodes"
        :key="node.id"
        class="tree-item"
        :class="{
          'is-forbidden': node.rating === 'Forbidden',
          'is-warning': node.rating === 'Warning',
          'is-selected': node.is_selected,
          'is-junction': node.is_junction,
          'is-search-match': isMatched(node),
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
          <!-- 本软件迁移：文件夹 + 穿出的向右箭头（单 SVG 图标） -->
          <svg
            v-if="node.is_junction && node.is_migrated_by_us"
            class="migrated-icon"
            viewBox="0 0 16 16"
            width="14"
            height="14"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <!-- 文件夹底部（右下角留缺口让箭头穿出） -->
            <path d="M1.5 4.5 L1.5 12.5 L8 12.5" />
            <!-- 文件夹顶部 tab -->
            <path d="M1.5 5.5 L5 5.5 L6 7 L9 7" />
            <!-- 向右转移箭头：从文件夹内部穿出 -->
            <path d="M6.5 10 L13.5 10" />
            <path d="M11 7.5 L13.5 10 L11 12.5" />
          </svg>
          <template v-else>{{ node.is_junction ? '🔗' : '📁' }}</template>
        </span>
        <!-- 名称与大小（搜索时高亮匹配文本） -->
        <span class="node-name" v-html="highlightName(node.name)"></span>
        <span class="node-size">{{
          node.is_junction ? '—' :
          (node.rating === 'Forbidden' ? '—' :
          (node.size_text || t('tree.calculating')))
        }}</span>
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
          v-if="node.is_junction && node.is_migrated_by_us"
          size="tiny"
          type="success"
          :bordered="false"
        >{{ t('tree.migratedByUs') }}</n-tag>
        <n-tag
          v-else-if="node.is_junction"
          size="tiny"
          type="info"
          :bordered="false"
        >{{ t('tree.junction') }}</n-tag>
      </div>
      <!-- 空状态：未扫描 -->
      <div v-if="store.treeNodes.length === 0" class="tree-empty">
        <template v-if="store.migrationStatus === 'Scanning'">
          <n-spin size="small" />
          <span style="margin-left: 8px">{{ store.scanDetail || t('tree.scanning') }}</span>
        </template>
        <template v-else>
          {{ t('tree.empty') }}
        </template>
      </div>
      <!-- 空状态：搜索无匹配 -->
      <div v-else-if="searchKeyword.trim() && filteredNodes.length === 0" class="tree-empty">
        {{ t('tree.noMatch') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'
import type { TreeNode } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()

// 搜索关键词（空值时显示全部节点）
const searchKeyword = ref('')
const scrollContainer = ref<HTMLElement | null>(null)

// 判断节点名称是否匹配搜索关键词
function isMatched(node: TreeNode): boolean {
  const kw = searchKeyword.value.trim().toLowerCase()
  if (!kw) return false
  return node.name.toLowerCase().includes(kw)
}

// 过滤后的节点列表：保留匹配节点 + 其祖先路径（让用户看到匹配项在树中的位置）
const filteredNodes = computed(() => {
  const kw = searchKeyword.value.trim().toLowerCase()
  if (!kw) return store.treeNodes

  // 收集所有匹配节点的路径（小写，用于前缀比较）
  const matchedPaths = store.treeNodes
    .filter(n => n.name.toLowerCase().includes(kw))
    .map(n => n.path.toLowerCase())

  if (matchedPaths.length === 0) return []

  // 保留条件：自己匹配，或是某个匹配节点的祖先（路径前缀）
  return store.treeNodes.filter(node => {
    if (node.name.toLowerCase().includes(kw)) return true
    const nodePathLower = node.path.toLowerCase() + '\\'
    return matchedPaths.some(p => p.startsWith(nodePathLower))
  })
})

// 匹配数量
const matchCount = computed(() => {
  const kw = searchKeyword.value.trim().toLowerCase()
  if (!kw) return 0
  return store.treeNodes.filter(n => n.name.toLowerCase().includes(kw)).length
})

// 搜索时自动滚动到第一个匹配项
watch(filteredNodes, (nodes) => {
  if (searchKeyword.value.trim() && nodes.length > 0) {
    nextTick(() => {
      const firstMatch = scrollContainer.value?.querySelector('.is-search-match')
      firstMatch?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    })
  }
})

function onNodeClick(node: TreeNode) {
  if (node.has_children) {
    store.toggleNodeExpand(node.id)
  }
}

// === 高亮匹配文本（防 XSS：先转义 HTML，再插入 <mark>） ===

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function highlightName(name: string): string {
  const kw = searchKeyword.value.trim()
  const escaped = escapeHtml(name)
  if (!kw) return escaped
  const regex = new RegExp(`(${escapeRegex(kw)})`, 'gi')
  return escaped.replace(regex, '<mark class="search-highlight">$1</mark>')
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

/* 搜索框区域 */
.tree-search {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.search-icon {
  font-size: 12px;
  opacity: 0.7;
}

.search-count {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  flex-shrink: 0;
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

/* 搜索匹配项高亮 */
.tree-item.is-search-match {
  background: rgba(255, 213, 79, 0.08);
}

.tree-item.is-search-match.is-selected {
  background: rgba(77, 171, 247, 0.15);
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
  display: inline-flex;
  align-items: center;
}

/* 本软件迁移目录：文件夹 + 穿出箭头（单 SVG 图标）
   用 success 绿色与"已迁移"标签呼应，区别于普通 Junction（🔗） */
.folder-icon .migrated-icon {
  color: var(--success-color, #18a058);
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

/* 高亮匹配文本（v-html 内容需要 :deep 穿透 scoped 限制） */
:deep(.search-highlight) {
  background: rgba(255, 213, 79, 0.4);
  color: inherit;
  padding: 0 1px;
  border-radius: 2px;
  font-weight: 600;
}
</style>
