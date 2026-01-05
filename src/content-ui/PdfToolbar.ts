import { UIService } from './UIService'
import type { Theme } from '../types'

// svg icons (lucide-style, pastel)
const ICONS = {
  fileText: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/></svg>`,
  message: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z"/></svg>`,
  copy: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>`,
  panel: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/></svg>`,
  file: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14,2 14,8 20,8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10,9 9,9 8,9"/></svg>`,
  x: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`
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
  private isProcessing = false
  private themeUnsubscribe: (() => void) | null = null

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
    
    if (this.isPdfPage) {
      console.log('metldr: pdf page detected, injecting toolbar')
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
  }

  private updateStyles(theme: Theme): void {
    if (!this.styleEl) return
    this.styleEl.textContent = this.getToolbarStyles(theme)
  }

  private setupCloseHandlers(shadow: ShadowRoot): void {
    document.addEventListener('click', (e) => {
      if (this.isExpanded && !shadow.contains(e.target as Node)) {
        this.collapseMenu()
      }
    })
    
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && this.isExpanded) {
        this.collapseMenu()
      }
    })
  }

  private toggleMenu(): void {
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
      { icon: ICONS.copy, label: 'Copy Text', action: () => this.extractText() },
      { icon: ICONS.panel, label: 'Open Panel', action: () => this.openSidePanel() },
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
    this.isProcessing = true
    this.showLoading()
    
    try {
      const response = await chrome.runtime.sendMessage({
        type: 'PDF_SUMMARIZE',
        url: window.location.href
      }) as { success: boolean; summary?: { bullets: string[] }; error?: string }
      
      if (response?.success && response.summary) {
        this.showResult(response.summary.bullets)
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
    chrome.runtime.sendMessage({ type: 'OPEN_SIDE_PANEL', focus: 'chat' })
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
      <div class="result-actions">
        <button class="result-action" data-action="panel">Open in Panel</button>
        <button class="result-action" data-action="copy">Copy</button>
      </div>
    `
    
    shadow.appendChild(this.resultCard)
    
    requestAnimationFrame(() => this.resultCard?.classList.add('visible'))
    
    this.resultCard.querySelector('.result-close')?.addEventListener('click', () => {
      this.hideResult()
    })
    
    this.resultCard.querySelector('[data-action="panel"]')?.addEventListener('click', () => {
      this.openSidePanel()
      this.hideResult()
    })
    
    this.resultCard.querySelector('[data-action="copy"]')?.addEventListener('click', async () => {
      await navigator.clipboard.writeText(bullets.join('\n• '))
      this.showToast('Copied to clipboard')
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
      
      .result-actions {
        display: flex;
        gap: 8px;
        padding: 10px 14px;
        background: ${theme.bgSecondary};
        border-top: 1px solid ${theme.borderSubtle};
      }
      
      .result-action {
        flex: 1;
        padding: 8px 10px;
        border: 1px solid ${theme.border};
        border-radius: 8px;
        background: ${theme.bg};
        color: ${theme.text};
        font-size: 11px;
        font-weight: 500;
        cursor: pointer;
        transition: all 0.15s;
      }
      
      .result-action:hover {
        border-color: ${theme.primary};
        color: ${theme.primary};
      }
    `
  }

  cleanup(): void {
    this.themeUnsubscribe?.()
    this.container?.remove()
    this.container = null
    this.fab = null
    this.menu = null
    this.resultCard = null
    this.styleEl = null
  }
}
