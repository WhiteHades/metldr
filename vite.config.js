import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { resolve, dirname } from 'node:path'

const isContentBuild = process.env.BUILD_TARGET === 'content'

const copyInboxSDKPlugin = () => ({
  name: 'copy-inboxsdk-pageworld',
  closeBundle() {
    if (!isContentBuild) {
      const src = resolve('node_modules/@inboxsdk/core/pageWorld.js')
      const dest = resolve('dist/pageWorld.js')
      if (existsSync(src)) {
        copyFileSync(src, dest)
        console.log('copied pageWorld.js to dist/')
      }
    }
  }
})

const originalConsoleWarn = console.warn
console.warn = (...args) => {
  const message = args.join(' ')
  if (message.includes('@property') || message.includes('Unknown at rule')) {
    return
  }
  originalConsoleWarn(...args)
}

export default defineConfig({
  plugins: isContentBuild ? [] : [vue(), tailwindcss(), copyInboxSDKPlugin()],
  css: {
    lightningcss: {
      errorRecovery: true,
      targets: {
        chrome: 85,
      }
    }
  },
  build: {
    outDir: 'dist',
    emptyOutDir: !isContentBuild,
    rollupOptions: isContentBuild ? {
      input: 'src/content/main.js',
      output: {
        entryFileNames: 'content.js',
        format: 'iife',
        name: 'MeTLDRContent',
        inlineDynamicImports: true,
      },
    } : {
      // main build
      input: {
        side_panel: 'index.html',
        welcome: 'welcome.html',
        background: 'src/background/index.js',
      },
      output: {
        entryFileNames: '[name].js',
        chunkFileNames: 'chunks/[name].js',
        assetFileNames: 'assets/[name][extname]',
      }
    }
  }
})
