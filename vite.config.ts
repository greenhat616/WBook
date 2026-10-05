import React from '@vitejs/plugin-react'
import Icons from 'unplugin-icons/vite'
import I18nextLoader from 'vite-plugin-i18next-loader'
import Svgr from 'vite-plugin-svgr'

import { defineConfig } from 'vite'
// https://vitejs.dev/config/
export default defineConfig({
  plugins: [
    React(),
    Svgr(),
    I18nextLoader({
      paths: ['./locales']
    }),
    Icons({
      compiler: 'jsx' // or 'solid'
    })
  ],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    // Rust build outputs can be locked while Cargo and Vite run together on Windows.
    watch: { ignored: ['**/backend/**'] }
  },
  // 3. to make use of `TAURI_DEBUG` and other env variables
  // https://tauri.app/v1/api/config#buildconfig.beforedevcommand
  envPrefix: ['VITE_', 'TAURI_'],
  resolve: {
    alias: {
      '@': '/src',
      '~': '/'
    }
  }
})
