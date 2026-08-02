<template>
  <n-config-provider :theme="darkTheme" :locale="naiveLocale" :date-locale="naiveDateLocale">
    <n-message-provider>
      <router-view />
      <!-- 帮助对话框：首次启动自动弹出，或通过 store.helpVisible 控制 -->
      <HelpDialog />
      <!-- 更新对话框：由 store.updateVisible 控制 -->
      <UpdateDialog />
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { darkTheme, zhCN, dateZhCN, enUS, dateEnUS } from 'naive-ui'
import { useI18n } from 'vue-i18n'
import { useAppStore } from './stores/app'
import HelpDialog from './components/HelpDialog.vue'
import UpdateDialog from './components/UpdateDialog.vue'

const { locale } = useI18n()
const store = useAppStore()

const naiveLocale = computed(() => (locale.value === 'zh-CN' ? zhCN : enUS))
const naiveDateLocale = computed(() => (locale.value === 'zh-CN' ? dateZhCN : dateEnUS))

// 应用启动时检测是否首次运行，首次则弹出帮助框
// 遵循"软件首次打开要弹出帮助框"的用户需求
onMounted(() => {
  store.initHelpOnFirstLaunch()
})
</script>

<style>
/* 全局已在 global.css 中定义 */
</style>
