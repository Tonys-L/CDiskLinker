<!--
  更新对话框：发现新版本时展示版本号、更新日志，提供下载安装进度。
  - 由 store.updateVisible 控制显示
  - 下载安装过程中显示进度条，禁用关闭按钮避免中断
  - 安装完成后自动 relaunch（由 store.downloadAndInstallUpdate 处理）
-->
<template>
  <n-modal
    :show="store.updateVisible"
    preset="card"
    :title="t('update.title')"
    style="width: 480px; max-width: 92vw;"
    :mask-closable="!store.updateDownloading"
    :close-on-esc="!store.updateDownloading"
    @update:show="(v: boolean) => { if (!v) store.closeUpdateDialog() }"
  >
    <div class="update-body">
      <!-- 检查中 -->
      <div v-if="store.updateChecking" class="update-status">
        <n-spin size="small" />
        <span>{{ t('update.checking') }}</span>
      </div>

      <!-- 错误提示 -->
      <div v-else-if="store.updateErrorMsg" class="update-error">
        {{ t('update.fail') }}: {{ store.updateErrorMsg }}
      </div>

      <!-- 发现新版本 -->
      <div v-else-if="store.updateInfo" class="update-info">
        <div class="update-version-row">
          <span class="update-label">{{ t('update.version') }}:</span>
          <span class="update-version">v{{ store.updateInfo.version }}</span>
        </div>
        <div class="update-notes-block">
          <div class="update-label">{{ t('update.notes') }}:</div>
          <div class="update-notes-content">{{ store.updateInfo.body || '-' }}</div>
        </div>
      </div>

      <!-- 已是最新 -->
      <div v-else class="update-status">
        <span>{{ t('update.latest') }}</span>
      </div>

      <!-- 下载进度 -->
      <div v-if="store.updateDownloading" class="update-progress">
        <n-progress
          type="line"
          :percentage="store.updateProgress"
          :height="12"
          :border-radius="6"
          indicator-placement="inside"
        />
        <div class="update-progress-text">{{ store.updateProgressText }}</div>
      </div>
    </div>

    <template #footer>
      <div class="update-footer">
        <n-button
          v-if="!store.updateDownloading"
          size="small"
          tertiary
          @click="store.closeUpdateDialog()"
        >
          {{ store.updateInfo ? t('update.later') : t('common.close') }}
        </n-button>
        <n-button
          v-if="store.updateInfo && !store.updateDownloading"
          type="primary"
          size="small"
          @click="store.downloadAndInstallUpdate()"
        >
          {{ t('update.download') }}
        </n-button>
        <n-button
          v-if="!store.updateInfo && !store.updateChecking"
          size="small"
          type="primary"
          @click="store.checkForUpdate()"
        >
          {{ t('help.checkUpdate') }}
        </n-button>
      </div>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useAppStore } from '../stores/app'

const { t } = useI18n()
const store = useAppStore()
</script>

<style scoped>
.update-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 60px;
}

.update-status {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-secondary, #adb5bd);
  padding: 8px 0;
}

.update-error {
  font-size: 13px;
  color: var(--error, #ff6b6b);
  padding: 8px 12px;
  background: rgba(255, 107, 107, 0.1);
  border-radius: 6px;
}

.update-info {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.update-version-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}

.update-label {
  color: var(--text-secondary, #adb5bd);
}

.update-version {
  color: var(--accent, #4dabf7);
  font-weight: 600;
  font-size: 15px;
}

.update-notes-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.update-notes-content {
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-primary, #fff);
  padding: 10px 12px;
  background: var(--bg-tertiary, #243447);
  border-radius: 6px;
  white-space: pre-wrap;
  max-height: 200px;
  overflow-y: auto;
}

.update-progress {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 4px;
}

.update-progress-text {
  font-size: 11px;
  color: var(--text-secondary, #adb5bd);
  text-align: center;
}

.update-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
