import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Tauri serves this on a fixed port (tauri.conf.json devUrl).
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/crates/**', '**/target/**', '**/_verify/**'] },
  },
  build: {
    // WebView2 is evergreen Chromium.
    target: 'es2022',
    outDir: 'dist',
    emptyOutDir: true,
  },
})
