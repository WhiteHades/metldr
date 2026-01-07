import { UIService } from './UIService'
import type { Theme } from '../types'

// svg icons (lucide-style, pastel)
const ICONS = {
  fileText: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/></svg>`,
  message: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z"/></svg>`,
  copy: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>`,
  panel: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/></svg>`,
  file: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14,2 14,8 20,8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10,9 9,9 8,9"/></svg>`,
  x: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`,
  upload: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>`
}

export class PdfToolbar {
  private container: HTMLElement | null = null
  private shadowRoot: ShadowRoot | null = null
  private fab: HTMLElement | null = null
  private menu: HTMLElement | null = null
  private resultCard: HTMLElement | null = null
  private styleEl: HTMLElement | null = null
  private isExpanded = false
  private isPdfPage = false
  private isLocalPdf = false
  private isProcessing = false
  private sidePanelOpen = false
  private themeUnsubscribe: (() => void) | null = null
  private localPdfPrompt: HTMLElement | null = null
  private progressBar: HTMLElement | null = null
  private messageListener: ((message: any) => void) | null = null

  constructor() {
    this.detectPdfPage()
  }

  private detectPdfPage(): void {
    const url = window.location.href.toLowerCase()
    const contentType = document.contentType?.toLowerCase() || ''
    
    this.isPdfPage = 
      url.endsWith('.pdf') || 
      url.includes('.pdf?') || 
      url.includes('.pdf#') ||
      contentType === 'application/pdf'
    
    // detect if this is a local file:// PDF
    this.isLocalPdf = this.isPdfPage && window.location.href.startsWith('file://')
    
    if (this.isPdfPage) {
      console.log('metldr: pdf page detected, injecting toolbar', { isLocal: this.isLocalPdf })
      this.injectFab()
    }
  }

  private injectFab(): void {
    this.container = document.createElement('div')
    this.container.id = 'metldr-pdf-toolbar'
    this.shadowRoot = this.container.attachShadow({ mode: 'open' })
    
    // inject styles
    this.styleEl = document.createElement('style')
    this.updateStyles(UIService.currentTheme)
    this.shadowRoot.appendChild(this.styleEl)
    
    // subscribe to theme changes
    this.themeUnsubscribe = UIService.onChange((_name, theme) => {
      this.updateStyles(theme)
    })
    
    // create fab
    this.fab = document.createElement('button')
    this.fab.className = 'metldr-pdf-fab'
    this.fab.setAttribute('aria-label', 'metldr PDF Tools')
    this.fab.innerHTML = ICONS.file
    this.fab.onclick = () => {
      console.log('metldr: PDF FAB clicked, toggling menu')
      this.toggleMenu()
    }
    
    this.shadowRoot.appendChild(this.fab)
    document.body.appendChild(this.container)
    
    // animate in via css class
    requestAnimationFrame(() => {
      this.fab?.classList.add('visible')
    })
    
    this.setupCloseHandlers(this.shadowRoot)
    this.setupProgressListener()
  }
  
  private setupProgressListener(): void {
    const currentUrl = window.location.href
    
    this.messageListener = (message: any) => {
      if (message?.type === 'INDEXING_PROGRESS' && message.sourceId === currentUrl) {
        this.updateProgress(message.percent)
      }
    }
    
    chrome.runtime.onMessage.addListener(this.messageListener)
  }
  
  private updateProgress(percent: number): void {
    const shadow = this.container?.shadowRoot
    if (!shadow) return
    
    if (!this.progressBar) {
      // create progress bar
      this.progressBar = document.createElement('div')
      this.progressBar.className = 'metldr-pdf-progress'
      this.progressBar.innerHTML = `
        <div class="progress-content">
          <span class="progress-text">indexing...</span>
          <span class="progress-percent">0%</span>
        </div>
        <div class="progress-bar">
          <div class="progress-fill"></div>
        </div>
      `
      shadow.appendChild(this.progressBar)
      requestAnimationFrame(() => this.progressBar?.classList.add('visible'))
    }
    
    // update progress
    const fill = this.progressBar.querySelector('.progress-fill') as HTMLElement
    const percentText = this.progressBar.querySelector('.progress-percent')
    if (fill) fill.style.width = `${percent}%`
    if (percentText) percentText.textContent = `${percent}%`
    
    // hide when complete
    if (percent >= 100) {
      setTimeout(() => this.hideProgress(), 1500)
    }
  }
  
  private hideProgress(): void {
    if (!this.progressBar) return
    this.progressBar.classList.remove('visible')
    setTimeout(() => {
      this.progressBar?.remove()
      this.progressBar = null
    }, 200)
  }

  private updateStyles(theme: Theme): void {
    if (!this.styleEl) return
    this.styleEl.textContent = this.getToolbarStyles(theme)
  }

  private setupCloseHandlers(shadow: ShadowRoot): void {
    document.addEventListener('click', (e) => {
      const path = e.composedPath()
      let isClickInside = false
      
      for (const el of path) {
        if (el === this.container) {
          isClickInside = true
          break
        }
        if (el instanceof Node && el.getRootNode() === shadow) {
          isClickInside = true
          break
        }
      }
      
      if (!isClickInside) {
        if (this.isExpanded) this.collapseMenu()
        if (this.resultCard) this.hideResult()
        if (this.localPdfPrompt) this.hideLocalPdfPrompt()
      }
    }, true)
    
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        if (this.isExpanded) this.collapseMenu()
        if (this.resultCard) this.hideResult()
        if (this.localPdfPrompt) this.hideLocalPdfPrompt()
      }
    })
  }

  private toggleMenu(): void {
    if (this.resultCard || this.localPdfPrompt) {
      this.hideResult()
      this.hideLocalPdfPrompt()
      return
    }
    
    if (this.isExpanded) {
      this.collapseMenu()
    } else {
      this.expandMenu()
    }
  }

  private expandMenu(): void {
    if (this.isExpanded || !this.fab) return
    this.isExpanded = true
    
    this.fab.classList.add('expanded')
    
    this.menu = document.createElement('div')
    this.menu.className = 'metldr-pdf-menu'
    
    const menuItems = [
      { icon: ICONS.fileText, label: 'Summarize', action: () => this.summarize() },
      { icon: ICONS.message, label: 'Chat', action: () => this.openChat() },
    ]
    
    menuItems.forEach((item, i) => {
      const btn = document.createElement('button')
      btn.className = 'metldr-pdf-menu-item'
      btn.style.setProperty('--delay', `${i * 0.05}s`)
      btn.innerHTML = `<span class="icon">${item.icon}</span><span class="label">${item.label}</span>`
      btn.onclick = (e) => {
        e.stopPropagation()
        item.action()
      }
      this.menu!.appendChild(btn)
    })
    
    // append to shadow root directly
    if (this.shadowRoot) {
      this.shadowRoot.appendChild(this.menu)
    }
    
    // trigger animation
    requestAnimationFrame(() => {
      this.menu?.classList.add('visible')
    })
  }

  private collapseMenu(): void {
    if (!this.isExpanded || !this.fab) return
    this.isExpanded = false
    
    this.fab.classList.remove('expanded')
    
    if (this.menu) {
      this.menu.classList.remove('visible')
      this.menu.classList.add('hiding')
      
      setTimeout(() => {
        this.menu?.remove()
        this.menu = null
      }, 200)
    }
  }

  async summarize(): Promise<void> {
    if (this.isProcessing) return
    
    // for local PDFs, check cache first before showing file picker
    if (this.isLocalPdf) {
      try {
        const response = await chrome.runtime.sendMessage({
          type: 'GET_PAGE_CACHE',
          url: window.location.href
        }) as { summary?: { bullets?: string[]; title?: string } } | null
        
        if (response?.summary?.bullets?.length) {
          this.collapseMenu()
          this.showResult(response.summary.bullets)
          return
        }
      } catch {
        // cache check failed, continue with file picker
      }
      
      this.collapseMenu()
      this.showLocalPdfPrompt('summarize')
      return
    }
    
    this.isProcessing = true
    this.showLoading()
    
    try {
      const response = await chrome.runtime.sendMessage({
        type: 'PDF_SUMMARIZE',
        url: window.location.href
      }) as { success: boolean; summary?: { bullets: string[] }; error?: string }
      
      if (response?.success && response.summary) {
        this.showResult(response.summary.bullets)
      } else if (response?.error === 'LOCAL_PDF_NEEDS_PICKER') {
        // fallback: show file picker prompt
        this.showLocalPdfPrompt('summarize')
      } else {
        this.showError(response?.error || 'Failed to summarize')
      }
    } catch (err) {
      console.error('metldr: pdf summarize error:', err)
      this.showError((err as Error).message)
    } finally {
      this.isProcessing = false
      this.hideLoading()
      this.collapseMenu()
    }
  }

  async extractText(): Promise<void> {
    if (this.isProcessing) return
    
    // for local PDFs, show file picker prompt directly
    if (this.isLocalPdf) {
      this.collapseMenu()
      this.showLocalPdfPrompt('copy')
      return
    }
    
    this.isProcessing = true
    this.showLoading()
    
    try {
      const response = await chrome.runtime.sendMessage({
        type: 'PDF_EXTRACT_TEXT',
        url: window.location.href
      }) as { success: boolean; text?: string; wordCount?: number; error?: string }
      
      if (response?.success && response.text) {
        await navigator.clipboard.writeText(response.text)
        this.showToast(`Copied ${response.wordCount?.toLocaleString() || 'text'} words`)
      } else if (response?.error === 'LOCAL_PDF_NEEDS_PICKER') {
        // fallback: show file picker prompt
        this.showLocalPdfPrompt('copy')
      } else {
        this.showError(response?.error || 'Failed to extract text')
      }
    } catch (err) {
      console.error('metldr: pdf extract error:', err)
      this.showError((err as Error).message)
    } finally {
      this.isProcessing = false
      this.hideLoading()
      this.collapseMenu()
    }
  }

  openChat(): void {
    // send toggle message - background will open panel if closed, or broadcast close message to panel
    chrome.runtime.sendMessage({ type: 'TOGGLE_SIDE_PANEL', focus: 'chat' })
    this.collapseMenu()
  }

  openSidePanel(): void {
    chrome.runtime.sendMessage({ type: 'OPEN_SIDE_PANEL' })
    this.collapseMenu()
  }

  private showLoading(): void {
    this.fab?.classList.add('loading')
  }

  private hideLoading(): void {
    this.fab?.classList.remove('loading')
  }

  private showToast(message: string): void {
    const shadow = this.container?.shadowRoot
    if (!shadow) return
    
    const toast = document.createElement('div')
    toast.className = 'metldr-pdf-toast'
    toast.textContent = message
    shadow.appendChild(toast)
    
    requestAnimationFrame(() => toast.classList.add('visible'))
    
    setTimeout(() => {
      toast.classList.remove('visible')
      setTimeout(() => toast.remove(), 200)
    }, 2500)
  }

  private showError(message: string): void {
    const shadow = this.container?.shadowRoot
    if (!shadow) return
    
    const toast = document.createElement('div')
    toast.className = 'metldr-pdf-toast error'
    toast.textContent = message
    shadow.appendChild(toast)
    
    requestAnimationFrame(() => toast.classList.add('visible'))
    
    setTimeout(() => {
      toast.classList.remove('visible')
      setTimeout(() => toast.remove(), 200)
    }, 3500)
  }

  private showResult(bullets: string[]): void {
    const shadow = this.container?.shadowRoot
    if (!shadow) return
    
    this.resultCard?.remove()
    
    this.resultCard = document.createElement('div')
    this.resultCard.className = 'metldr-pdf-result'
    
    this.resultCard.innerHTML = `
      <div class="result-header">
        <span class="result-icon">${ICONS.fileText}</span>
        <span class="result-title">Summary</span>
        <button class="result-close" aria-label="Close">${ICONS.x}</button>
      </div>
      <ul class="result-bullets">
        ${bullets.slice(0, 5).map(b => `<li>${this.escapeHtml(b)}</li>`).join('')}
      </ul>
      <button class="result-copy-btn" aria-label="Copy summary">
        ${ICONS.copy}
        <span class="copy-feedback">Copied!</span>
      </button>
    `
    
    shadow.appendChild(this.resultCard)
    
    requestAnimationFrame(() => this.resultCard?.classList.add('visible'))
    
    this.resultCard.querySelector('.result-close')?.addEventListener('click', (e) => {
      e.stopPropagation()
      this.hideResult()
    })
    
    const copyBtn = this.resultCard.querySelector('.result-copy-btn')
    copyBtn?.addEventListener('click', async (e) => {
      e.stopPropagation()
      await navigator.clipboard.writeText(bullets.join('\n• '))
      copyBtn.classList.add('copied')
      setTimeout(() => copyBtn.classList.remove('copied'), 1500)
    })
  }

  private hideResult(): void {
    if (!this.resultCard) return
    
    this.resultCard.classList.remove('visible')
    setTimeout(() => {
      this.resultCard?.remove()
      this.resultCard = null
    }, 200)
  }

  private async showLocalPdfPrompt(action: 'summarize' | 'copy'): Promise<void> {
    const shadow = this.container?.shadowRoot
    if (!shadow) return
    
    try {
      const response = await chrome.runtime.sendMessage({
        type: 'GET_PAGE_CACHE',
        url: window.location.href
      }) as { summary?: { bullets?: string[]; title?: string } } | null
      
      const cached = response?.summary
      if (cached?.bullets?.length) {
        if (action === 'summarize') {
          this.showResult(cached.bullets)
          return
        } else if (action === 'copy') {
          await navigator.clipboard.writeText(cached.bullets.join('\n• '))
          this.showToast('Copied cached summary')
          return
        }
      }
    } catch {
      // cache check failed, continue with file picker prompt
    }
    
    this.localPdfPrompt?.remove()
    
    this.localPdfPrompt = document.createElement('div')
    this.localPdfPrompt.className = 'metldr-pdf-prompt'
    
    const actionText = action === 'summarize' ? 'Summarize' : 'Copy Text'
    
    this.localPdfPrompt.innerHTML = `
      <div class="prompt-header">
        <span class="prompt-icon">${ICONS.file}</span>
        <span class="prompt-title">Local PDF</span>
        <button class="prompt-close" aria-label="Close">${ICONS.x}</button>
      </div>
      <p class="prompt-text">Select the PDF file to ${action}</p>
      <button class="prompt-action" data-action="${action}">
        <span class="icon">${ICONS.upload}</span>
        Select File
      </button>
    `
    
    shadow.appendChild(this.localPdfPrompt)
    requestAnimationFrame(() => this.localPdfPrompt?.classList.add('visible'))
    
    this.localPdfPrompt.querySelector('.prompt-close')?.addEventListener('click', () => {
      this.hideLocalPdfPrompt()
    })
    
    this.localPdfPrompt.querySelector('.prompt-action')?.addEventListener('click', () => {
      this.openFilePicker(action)
    })
  }

  private hideLocalPdfPrompt(): void {
    if (!this.localPdfPrompt) return
    
    this.localPdfPrompt.classList.remove('visible')
    setTimeout(() => {
      this.localPdfPrompt?.remove()
      this.localPdfPrompt = null
    }, 200)
  }

  // opens native file picker and processes PDF using existing pdfService via background
  private async openFilePicker(action: 'summarize' | 'copy'): Promise<void> {
    this.hideLocalPdfPrompt()
    this.showLoading()
    
    try {
      // create file input for user gesture
      const input = document.createElement('input')
      input.type = 'file'
      input.accept = '.pdf,application/pdf'
      input.style.display = 'none'
      document.body.appendChild(input)
      
      const file = await new Promise<File | null>((resolve) => {
        input.onchange = () => {
          const f = input.files?.[0] || null
          document.body.removeChild(input)
          resolve(f)
        }
        input.oncancel = () => {
          document.body.removeChild(input)
          resolve(null)
        }
        input.click()
      })
      
      if (!file) {
        this.hideLoading()
        return
      }
      
      console.log('metldr: processing local PDF:', file.name)
      
      // read file as ArrayBuffer and send to background for processing
      const arrayBuffer = await file.arrayBuffer()
      const uint8Array = new Uint8Array(arrayBuffer)
      
      // send to background for processing with sourceUrl for proper caching
      const response = await chrome.runtime.sendMessage({
        type: 'PDF_PROCESS_ARRAYBUFFER',
        data: Array.from(uint8Array),
        filename: file.name,
        action,
        sourceUrl: window.location.href
      }) as { success: boolean; summary?: { bullets: string[] }; text?: string; wordCount?: number; error?: string }
      
      if (response?.success) {
        if (action === 'summarize' && response.summary?.bullets) {
          this.showResult(response.summary.bullets)
        } else if (action === 'copy' && response.text) {
          await navigator.clipboard.writeText(response.text)
          this.showToast(`Copied ${response.wordCount?.toLocaleString() || 'text'} words`)
        }
      } else {
        this.showError(response?.error || 'Failed to process PDF')
      }
    } catch (err) {
      console.error('metldr: file picker error:', err)
      this.showError((err as Error).message)
    } finally {
      this.hideLoading()
    }
  }

  private escapeHtml(text: string): string {
    const div = document.createElement('div')
    div.textContent = text
    return div.innerHTML
  }

  private getToolbarStyles(theme: Theme): string {
    return `
      * {
        box-sizing: border-box;
        font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
      }
      
      .metldr-pdf-fab {
        position: fixed;
        bottom: 24px;
        right: 24px;
        z-index: 999998;
        width: 48px;
        height: 48px;
        border-radius: 14px;
        border: 1px solid ${theme.border};
        background: ${theme.bg};
        color: ${theme.primary};
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        box-shadow: 0 4px 16px ${theme.shadow}, 0 0 0 1px ${theme.borderSubtle};
        opacity: 0;
        transform: scale(0.8) translateY(10px);
        transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
      }
      
      .metldr-pdf-fab.visible {
        opacity: 1;
        transform: scale(1) translateY(0);
      }
      
      .metldr-pdf-fab:hover {
        border-color: ${theme.primary};
        box-shadow: 0 6px 24px ${theme.shadow}, 0 0 0 1px ${theme.primary};
        transform: scale(1.05);
      }
      
      .metldr-pdf-fab.expanded {
        transform: rotate(45deg);
        background: ${theme.bgSecondary};
      }
      
      .metldr-pdf-fab.loading {
        pointer-events: none;
        opacity: 0.7;
      }
      
      .metldr-pdf-fab.loading::after {
        content: '';
        position: absolute;
        width: 20px;
        height: 20px;
        border: 2px solid ${theme.border};
        border-top-color: ${theme.primary};
        border-radius: 50%;
        animation: spin 0.8s linear infinite;
      }
      
      @keyframes spin {
        to { transform: rotate(360deg); }
      }
      
      .metldr-pdf-menu {
        position: fixed;
        bottom: 80px;
        right: 24px;
        z-index: 999997;
        display: flex;
        flex-direction: column;
        gap: 6px;
      }
      
      .metldr-pdf-menu-item {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 10px 14px;
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-radius: 12px;
        font-size: 13px;
        font-weight: 500;
        color: ${theme.text};
        cursor: pointer;
        white-space: nowrap;
        box-shadow: 0 2px 8px ${theme.shadow};
        opacity: 0;
        transform: translateX(20px);
        transition: all 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
        transition-delay: var(--delay, 0s);
      }
      
      .metldr-pdf-menu.visible .metldr-pdf-menu-item {
        opacity: 1;
        transform: translateX(0);
      }
      
      .metldr-pdf-menu.hiding .metldr-pdf-menu-item {
        opacity: 0;
        transform: translateX(20px);
        transition-delay: 0s;
      }
      
      .metldr-pdf-menu-item:hover {
        background: ${theme.bgSecondary};
        border-color: ${theme.primary};
        transform: translateX(-4px);
      }
      
      .metldr-pdf-menu-item .icon {
        color: ${theme.primary};
        opacity: 0.8;
      }
      
      .metldr-pdf-toast {
        position: fixed;
        bottom: 90px;
        right: 24px;
        z-index: 999999;
        padding: 10px 14px;
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-radius: 10px;
        font-size: 13px;
        font-weight: 500;
        color: ${theme.text};
        box-shadow: 0 4px 16px ${theme.shadow};
        opacity: 0;
        transform: translateY(8px);
        transition: all 0.2s ease;
      }
      
      .metldr-pdf-toast.visible {
        opacity: 1;
        transform: translateY(0);
      }
      
      .metldr-pdf-toast.error {
        border-color: #ef4444;
        color: #ef4444;
      }
      
      .metldr-pdf-result {
        position: fixed;
        bottom: 80px;
        right: 24px;
        z-index: 999996;
        width: 300px;
        max-width: calc(100vw - 48px);
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-radius: 16px;
        box-shadow: 0 8px 32px ${theme.shadow};
        overflow: hidden;
        opacity: 0;
        transform: scale(0.95) translateY(8px);
        transition: all 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
        padding-bottom: 48px;
      }
      
      .metldr-pdf-result.visible {
        opacity: 1;
        transform: scale(1) translateY(0);
      }
      
      .result-header {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 12px 14px;
        background: ${theme.bgSecondary};
        border-bottom: 1px solid ${theme.borderSubtle};
        border-radius: 16px 16px 0 0;
      }
      
      .result-icon {
        color: ${theme.primary};
        opacity: 0.8;
      }
      
      .result-title {
        flex: 1;
        font-size: 13px;
        font-weight: 600;
        color: ${theme.text};
      }
      
      .result-close {
        width: 24px;
        height: 24px;
        border: none;
        background: none;
        color: ${theme.textMuted};
        cursor: pointer;
        border-radius: 6px;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s;
      }
      
      .result-close:hover {
        background: ${theme.border};
        color: ${theme.text};
      }
      
      .result-bullets {
        margin: 0;
        padding: 12px 14px;
        list-style: none;
      }
      
      .result-bullets li {
        position: relative;
        padding-left: 14px;
        margin-bottom: 8px;
        font-size: 12px;
        line-height: 1.5;
        color: ${theme.text};
      }
      
      .result-bullets li:last-child {
        margin-bottom: 0;
      }
      
      .result-bullets li::before {
        content: '';
        position: absolute;
        left: 0;
        top: 7px;
        width: 4px;
        height: 4px;
        border-radius: 50%;
        background: ${theme.primary};
        opacity: 0.6;
      }
      
      .result-copy-btn {
        position: absolute;
        bottom: 10px;
        right: 10px;
        width: 32px;
        height: 32px;
        border: 1px solid ${theme.border};
        border-radius: 8px;
        background: ${theme.bg};
        color: ${theme.textMuted};
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.2s;
      }
      
      .result-copy-btn:hover {
        border-color: ${theme.primary};
        color: ${theme.primary};
        background: ${theme.bgSecondary};
      }
      
      .result-copy-btn .copy-feedback {
        position: absolute;
        right: 100%;
        margin-right: 8px;
        white-space: nowrap;
        font-size: 11px;
        font-weight: 500;
        color: ${theme.primary};
        opacity: 0;
        transform: translateX(4px);
        transition: all 0.2s;
        pointer-events: none;
      }
      
      .result-copy-btn.copied {
        border-color: ${theme.primary};
        color: ${theme.primary};
      }
      
      .result-copy-btn.copied .copy-feedback {
        opacity: 1;
        transform: translateX(0);
      }
      
      /* local PDF prompt styles */
      .metldr-pdf-prompt {
        position: fixed;
        bottom: 80px;
        right: 24px;
        z-index: 999996;
        width: 280px;
        max-width: calc(100vw - 48px);
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-radius: 16px;
        box-shadow: 0 8px 32px ${theme.shadow};
        overflow: hidden;
        opacity: 0;
        transform: scale(0.95) translateY(8px);
        transition: all 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
      }
      
      .metldr-pdf-prompt.visible {
        opacity: 1;
        transform: scale(1) translateY(0);
      }
      
      .prompt-header {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 12px 14px;
        background: ${theme.bgSecondary};
        border-bottom: 1px solid ${theme.borderSubtle};
      }
      
      .prompt-icon {
        color: ${theme.primary};
        opacity: 0.8;
      }
      
      .prompt-title {
        flex: 1;
        font-size: 13px;
        font-weight: 600;
        color: ${theme.text};
      }
      
      .prompt-close {
        width: 24px;
        height: 24px;
        border: none;
        background: none;
        color: ${theme.textMuted};
        cursor: pointer;
        border-radius: 6px;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s;
      }
      
      .prompt-close:hover {
        background: ${theme.border};
        color: ${theme.text};
      }
      
      .prompt-text {
        margin: 0;
        padding: 12px 14px;
        font-size: 12px;
        color: ${theme.textMuted};
        line-height: 1.5;
      }
      
      .prompt-action {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        width: calc(100% - 28px);
        margin: 0 14px 14px;
        padding: 10px 16px;
        border: 1px solid color-mix(in srgb, ${theme.primary} 40%, transparent);
        border-radius: 10px;
        background: color-mix(in srgb, ${theme.primary} 15%, transparent);
        color: ${theme.primary};
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        transition: all 0.15s;
      }
      
      .prompt-action:hover {
        background: color-mix(in srgb, ${theme.primary} 25%, transparent);
        border-color: color-mix(in srgb, ${theme.primary} 60%, transparent);
      }
      
      .prompt-action .icon {
        display: flex;
      }
      
      /* progress bar styles */
      .metldr-pdf-progress {
        position: fixed;
        bottom: 80px;
        right: 24px;
        z-index: 999996;
        width: 200px;
        padding: 10px 14px;
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-radius: 12px;
        box-shadow: 0 4px 16px ${theme.shadow};
        opacity: 0;
        transform: translateY(8px);
        transition: all 0.2s ease;
      }
      
      .metldr-pdf-progress.visible {
        opacity: 1;
        transform: translateY(0);
      }
      
      .progress-content {
        display: flex;
        justify-content: space-between;
        align-items: center;
        margin-bottom: 6px;
      }
      
      .progress-text {
        font-size: 11px;
        color: ${theme.textMuted};
      }
      
      .progress-percent {
        font-size: 11px;
        font-weight: 600;
        color: ${theme.primary};
      }
      
      .progress-bar {
        height: 4px;
        background: ${theme.border};
        border-radius: 2px;
        overflow: hidden;
      }
      
      .progress-fill {
        height: 100%;
        width: 0%;
        background: ${theme.primary};
        border-radius: 2px;
        transition: width 0.2s ease;
      }
    `
  }

  cleanup(): void {
    this.themeUnsubscribe?.()
    if (this.messageListener) {
      chrome.runtime.onMessage.removeListener(this.messageListener)
      this.messageListener = null
    }
    this.container?.remove()
    this.container = null
    this.fab = null
    this.menu = null
    this.resultCard = null
    this.progressBar = null
    this.styleEl = null
  }
}
