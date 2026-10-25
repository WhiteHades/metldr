import { defineConfig, normalizePath, type Plugin } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { copyFileSync, existsSync, watch } from 'node:fs'
import { basename, dirname, resolve } from 'node:path'
import { execFileSync } from 'node:child_process'

const isContentBuild = process.env.BUILD_TARGET === 'content'
const privateFiles = [
  '**/.tmp/**', '**/prototypes/**', '**/docs/**',
  '**/{AGENTS,CLAUDE,PRODUCT,DESIGN}.md', '**/crates/**/target/**',
]
const welcomeGenerator = resolve('crates/metldr-welcome/target/release',
  process.platform === 'win32' ? 'welcome-guide.exe' : 'welcome-guide')

const rustWelcome = (): Plugin => ({
  name: 'rust-welcome',
  configureServer(server) {
    const directory = normalizePath(resolve('public/welcome')) + '/'
    let pending: ReturnType<typeof setTimeout> | undefined
    const refresh = () => {
      clearTimeout(pending)
      pending = setTimeout(() => {
        server.ws.send({ type: 'full-reload', path: '/welcome.html' })
        server.config.logger.info('Welcome preview updated.', { timestamp: true })
      }, 100)
    }
    const reload = (file: string) => {
      if (normalizePath(file).startsWith(directory)) refresh()
    }
    const generator = watch(dirname(welcomeGenerator), (_, file) => {
      if (file === basename(welcomeGenerator)) refresh()
    })
    generator.on('error', error => server.config.logger.error(
      `Welcome generator watch stopped: ${error.message}. Restart the development server.`,
    ))
    server.watcher.on('change', reload).on('add', reload)
    server.httpServer?.once('close', () => {
      clearTimeout(pending)
      generator.close()
      server.watcher.off('change', reload).off('add', reload)
    })
  },
  transformIndexHtml: {
    order: 'pre',
    handler(html, context) {
      if (resolve(context.filename) !== resolve('welcome.html')) return html
      return execFileSync(welcomeGenerator, [
        resolve('welcome.html'), '-',
      ], { encoding: 'utf8', timeout: 5_000, maxBuffer: 1_048_576, windowsHide: true })
    },
  },
})


const copyWasmAssets = () => ({
  name: 'copy-wasm-assets',
  writeBundle() {
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
  writeBundle() {
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
  server: {
    watch: { ignored: privateFiles },
    fs: { deny: ['.env', '.env.*', '*.{crt,pem}', '**/.git/**', ...privateFiles] },
  },
  plugins: isContentBuild ? [] : [rustWelcome(), vue(), tailwindcss(), copyInboxSDKPlugin(), copyWasmAssets()],
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
    modulePreload: false,
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
        panel: 'panel.html',
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
