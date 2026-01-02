import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { copyFileSync, existsSync } from 'node:fs'
import { resolve } from 'node:path'

const isContentBuild = process.env.BUILD_TARGET === 'content'


const copyWasmAssets = () => ({
  name: 'copy-wasm-assets',
  closeBundle() {
    if (!isContentBuild) {
      const assetsDir = resolve('dist/assets')
      if (!existsSync(assetsDir)) {
         // mkdirSync(assetsDir, { recursive: true })
      }

      const filesToCopy = [
        { src: 'node_modules/pdfjs-dist/build/pdf.worker.min.mjs', dest: 'dist/assets/pdf.worker.min.js' },
        { src: 'node_modules/voy-search/voy_search_bg.wasm', dest: 'dist/assets/voy_search_bg.wasm' },
      ]

      filesToCopy.forEach(({ src, dest }) => {
        try {
          const sourcePath = resolve(src)
          const destPath = resolve(dest)
          if (existsSync(sourcePath)) {
            copyFileSync(sourcePath, destPath)
            console.log(`Copied ${src} to ${dest}`)
          } else {
             console.warn(`Source file not found: ${src}`)
          }
        } catch (e) {
             console.error(`Failed to copy ${src}:`, e)
        }
      })
    }
  }
})

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
console.warn = (...args: unknown[]) => {
  const message = args.join(' ')
  if (message.includes('@property') || message.includes('Unknown at rule')) {
    return
  }
  originalConsoleWarn(...args)
}

export default defineConfig({
  plugins: isContentBuild ? [] : [vue(), tailwindcss(), copyInboxSDKPlugin(), copyWasmAssets()],
  resolve: {
    alias: {
      '@': resolve(__dirname, './src')
    }
  },
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
      input: 'src/content/main.ts',
      output: {
        entryFileNames: 'content.js',
        format: 'iife',
        name: 'MeTLDRContent',
        inlineDynamicImports: true,
      },
    } : {
      input: {
        side_panel: 'index.html',
        welcome: 'welcome.html',
        background: 'src/background/index.ts',
      },
      output: {
        entryFileNames: '[name].js',
        chunkFileNames: 'chunks/[name].js',
        assetFileNames: 'assets/[name][extname]',
      }
    }
  }
})
