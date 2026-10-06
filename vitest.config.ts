import { defaultExclude, defineConfig, mergeConfig } from 'vitest/config'
import viteConfig from './vite.config.ts'
export default mergeConfig(
  viteConfig,
  defineConfig({
    // @lit/react's "node" export is a server renderer that never sets element
    // properties, so jsdom tests need the browser build. Inlining routes the
    // import through Vite, where this condition applies.
    resolve: { conditions: ['browser'] },
    optimizeDeps: {
      entries: []
    },
    test: {
      testTimeout: 30_000,
      name: 'unit',
      // setupFiles: ['./test/setup.ts'],
      exclude: [...defaultExclude, '**/target/**', '**/dist/**'],
      server: { deps: { inline: ['@m3e/react', '@lit/react'] } }
    }
  })
)
