<!--
  帮助对话框：首次启动自动弹出，也可通过右上角 "?" 按钮手动打开。
  内容包含：软件作用、基本用法、举例、注意事项，支持中英文切换。
  底部提供"我已了解"按钮与"检查更新"入口（与 UpdateDialog 解耦，仅触发可见性）。
-->
<template>
  <n-modal
    :show="store.helpVisible"
    preset="card"
    :title="t('help.title')"
    style="width: 640px; max-width: 92vw;"
    :mask-closable="false"
    :close-on-esc="true"
    @update:show="(v: boolean) => (store.helpVisible = v)"
  >
    <div class="help-body">
      <!-- 软件作用 -->
      <section class="help-section">
        <h3 class="help-section-title">{{ t('help.whatIs') }}</h3>
        <p class="help-text">{{ t('help.whatIsBody') }}</p>
      </section>

      <!-- 基本用法 -->
      <section class="help-section">
        <h3 class="help-section-title">{{ t('help.steps') }}</h3>
        <ol class="help-steps">
          <li>{{ t('help.step1') }}</li>
          <li>{{ t('help.step2') }}</li>
          <li>{{ t('help.step3') }}</li>
          <li>{{ t('help.step4') }}</li>
          <li>{{ t('help.step5') }}</li>
        </ol>
      </section>

      <!-- 举例 -->
      <section class="help-section">
        <h3 class="help-section-title">{{ t('help.example') }}</h3>
        <p class="help-text help-example">{{ t('help.exampleBody') }}</p>
      </section>

      <!-- 注意事项 -->
      <section class="help-section">
        <h3 class="help-section-title">{{ t('help.notes') }}</h3>
        <ul class="help-notes">
          <li>{{ t('help.note1') }}</li>
          <li>{{ t('help.note2') }}</li>
          <li>{{ t('help.note3') }}</li>
          <li>{{ t('help.note4') }}</li>
        </ul>
      </section>
    </div>

    <template #footer>
      <div class="help-footer">
        <n-button size="small" tertiary @click="checkUpdate">
          {{ t('help.checkUpdate') }}
        </n-button>
        <n-button size="small" type="primary" @click="close">
          {{ t('help.gotIt') }}
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

// 关闭：同时标记"已看过帮助"，避免下次启动重复弹出
function close() {
  store.dismissHelp()
}

// 触发更新检查（委托给 app store 的 updateVisible，由 UpdateDialog 接管 UI）
function checkUpdate() {
  store.helpVisible = false
  store.triggerUpdateCheck()
}
</script>

<style scoped>
.help-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  max-height: 60vh;
  overflow-y: auto;
  padding-right: 4px;
}

.help-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.help-section-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #fff);
  padding-left: 8px;
  border-left: 3px solid var(--accent, #4dabf7);
}

.help-text {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-secondary, #adb5bd);
}

.help-example {
  padding: 10px 12px;
  background: var(--bg-tertiary, #243447);
  border-radius: 6px;
  border-left: 2px solid var(--accent, #4dabf7);
}

.help-steps,
.help-notes {
  margin: 0;
  padding-left: 20px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.help-steps li,
.help-notes li {
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-secondary, #adb5bd);
}

.help-footer {
  display: flex;
  justify-content: space-between;
  gap: 8px;
}
</style>
