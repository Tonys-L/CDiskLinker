import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import Components from 'unplugin-vue-components/vite'
import { NaiveUiResolver } from 'unplugin-vue-components/resolvers'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    vue(),
    Components({
      resolvers: [NaiveUiResolver()],
    }),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Windows 上 localhost 默认解析为 IPv4 (127.0.0.1)，
    // 而 Vite 默认监听 IPv6 ([::1])，导致 Tauri webview 连接失败（"无法打开 localhost"）。
    // 显式绑定 127.0.0.1 确保 Tauri webview 可访问。
    host: '127.0.0.1',
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'esnext',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
})
