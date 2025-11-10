import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

// check if building content script separately
const isContentBuild = process.env.BUILD_TARGET === 'content'

// suppress lightningcss @property warnings
const originalConsoleWarn = console.warn
console.warn = (...args) => {
  const message = args.join(' ')
  if (message.includes('@property') || message.includes('Unknown at rule')) {
    return
  }
  originalConsoleWarn(...args)
}

export default defineConfig({
  plugins: isContentBuild ? [] : [vue(), tailwindcss()], 
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
      }
    } : {
      // main build
      input: {
        side_panel: 'index.html',
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
