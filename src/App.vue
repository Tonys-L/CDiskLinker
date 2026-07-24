<template>
  <n-config-provider :theme="darkTheme" :locale="naiveLocale" :date-locale="naiveDateLocale">
    <n-message-provider>
      <router-view />
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { darkTheme, zhCN, dateZhCN, enUS, dateEnUS } from 'naive-ui'
import { useI18n } from 'vue-i18n'

const { locale } = useI18n()

const naiveLocale = computed(() => (locale.value === 'zh-CN' ? zhCN : enUS))
const naiveDateLocale = computed(() => (locale.value === 'zh-CN' ? dateZhCN : dateEnUS))
</script>

<style>
/* 全局已在 global.css 中定义 */
</style>
