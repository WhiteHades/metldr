import { gsap } from 'gsap'
import { formatTime } from '../utils/text'
import type { Theme, ThemeName, ThemeListener, Summary, IntentStyle } from '../types'

export const THEME_COLORS: Record<ThemeName, Theme> = {
  default: {
    primary: 'oklch(0.55 0.12 240)',
    secondary: 'oklch(0.50 0.10 280)',
    accent: 'oklch(0.53 0.11 190)',
    bg: 'oklch(0.14 0.01 265)',
    bgSecondary: 'oklch(0.18 0.01 265)',
    text: 'oklch(0.75 0.01 265)',
    textMuted: 'oklch(0.50 0.01 265)',
    border: 'oklch(0.28 0.01 265 / 0.3)',
    borderSubtle: 'oklch(0.28 0.01 265 / 0.15)',
    shadow: 'oklch(0 0 0 / 0.15)',
  },
  light: {
    primary: 'oklch(0.38 0.20 240)',
    secondary: 'oklch(0.42 0.16 280)',
    accent: 'oklch(0.45 0.17 190)',
    bg: 'oklch(1.00 0.00 0)',
    bgSecondary: 'oklch(0.975 0.00 265)',
    text: 'oklch(0.18 0.02 265)',
    textMuted: 'oklch(0.58 0.01 265)',
    border: 'oklch(0.80 0.01 265 / 0.3)',
    borderSubtle: 'oklch(0.80 0.01 265 / 0.15)',
    shadow: 'oklch(0 0 0 / 0.08)',
  },
  cyberpunk: {
    primary: 'oklch(0.75 0.22 200)',
    secondary: 'oklch(0.60 0.24 340)',
    accent: 'oklch(0.84 0.16 100)',
    bg: 'oklch(0.15 0.01 265)',
    bgSecondary: 'oklch(0.19 0.01 265)',
    text: 'oklch(0.88 0.02 265)',
    textMuted: 'oklch(0.50 0.02 265)',
    border: 'oklch(0.75 0.22 200 / 0.2)',
    borderSubtle: 'oklch(0.75 0.22 200 / 0.1)',
    shadow: 'oklch(0.75 0.22 200 / 0.10)',
  },
  catppuccin: {
    primary: 'oklch(0.87 0.04 30)',
    secondary: 'oklch(0.72 0.13 290)',
    accent: 'oklch(0.77 0.12 35)',
    bg: 'oklch(0.19 0.02 265)',
    bgSecondary: 'oklch(0.23 0.02 265)',
    text: 'oklch(0.87 0.03 250)',
    textMuted: 'oklch(0.54 0.03 250)',
    border: 'oklch(0.87 0.04 30 / 0.2)',
    borderSubtle: 'oklch(0.87 0.04 30 / 0.1)',
    shadow: 'oklch(0.87 0.04 30 / 0.15)',
  },
  gruvbox: {
    primary: 'oklch(0.66 0.15 45)',
    secondary: 'oklch(0.68 0.12 150)',
    accent: 'oklch(0.75 0.14 80)',
    bg: 'oklch(0.22 0.01 70)',
    bgSecondary: 'oklch(0.26 0.01 70)',
    text: 'oklch(0.86 0.04 70)',
    textMuted: 'oklch(0.58 0.02 70)',
    border: 'oklch(0.66 0.15 45 / 0.3)',
    borderSubtle: 'oklch(0.66 0.15 45 / 0.15)',
    shadow: 'oklch(0.66 0.15 45 / 0.2)',
  },
}

const INTENT_COLORS: Record<string, IntentStyle> = {
  'invoice': { bg: '#10b981', text: '#ffffff' },
  'receipt': { bg: '#059669', text: '#ffffff' },
  'payment': { bg: '#047857', text: '#ffffff' },
  'refund': { bg: '#34d399', text: '#064e3b' },
  'subscription': { bg: '#6ee7b7', text: '#064e3b' },
  'meeting request': { bg: '#3b82f6', text: '#ffffff' },
  'calendar invite': { bg: '#2563eb', text: '#ffffff' },
  'reminder': { bg: '#60a5fa', text: '#1e3a8a' },
  'reschedule': { bg: '#93c5fd', text: '#1e3a8a' },
  'cancellation': { bg: '#1d4ed8', text: '#ffffff' },
  'flight booking': { bg: '#06b6d4', text: '#ffffff' },
  'hotel reservation': { bg: '#0891b2', text: '#ffffff' },
  'travel itinerary': { bg: '#22d3ee', text: '#164e63' },
  'ticket': { bg: '#67e8f9', text: '#164e63' },
  'reservation': { bg: '#0e7490', text: '#ffffff' },
  'order confirmation': { bg: '#6366f1', text: '#ffffff' },
  'shipping update': { bg: '#818cf8', text: '#1e1b4b' },
  'delivery notice': { bg: '#4f46e5', text: '#ffffff' },
  'tracking': { bg: '#a5b4fc', text: '#1e1b4b' },
  'return/exchange': { bg: '#7c3aed', text: '#ffffff' },
  'account alert': { bg: '#f43f5e', text: '#ffffff' },
  'security alert': { bg: '#e11d48', text: '#ffffff' },
  'password reset': { bg: '#fb7185', text: '#881337' },
  'verification': { bg: '#fda4af', text: '#881337' },
  'login notification': { bg: '#be123c', text: '#ffffff' },
  'task assignment': { bg: '#475569', text: '#ffffff' },
  'project update': { bg: '#64748b', text: '#ffffff' },
  'status report': { bg: '#94a3b8', text: '#1e293b' },
  'feedback request': { bg: '#334155', text: '#ffffff' },
  'approval request': { bg: '#1e293b', text: '#ffffff' },
  'personal': { bg: '#ec4899', text: '#ffffff' },
  'introduction': { bg: '#f472b6', text: '#831843' },
  'follow-up': { bg: '#db2777', text: '#ffffff' },
  'thank you': { bg: '#fce7f3', text: '#9d174d' },
  'announcement': { bg: '#be185d', text: '#ffffff' },
  'newsletter': { bg: '#8b5cf6', text: '#ffffff' },
  'marketing': { bg: '#f97316', text: '#ffffff' },
  'promotion': { bg: '#fb923c', text: '#7c2d12' },
  'survey': { bg: '#fdba74', text: '#7c2d12' },
  'invitation': { bg: '#ea580c', text: '#ffffff' },
  'support ticket': { bg: '#14b8a6', text: '#ffffff' },
  'bug report': { bg: '#0d9488', text: '#ffffff' },
  'feature request': { bg: '#2dd4bf', text: '#134e4a' },
  'complaint': { bg: '#0f766e', text: '#ffffff' },
  'resolution': { bg: '#5eead4', text: '#134e4a' },
  'contract': { bg: '#d97706', text: '#ffffff' },
  'legal notice': { bg: '#b45309', text: '#ffffff' },
  'policy update': { bg: '#fbbf24', text: '#78350f' },
  'hr notice': { bg: '#f59e0b', text: '#78350f' },
  'compliance': { bg: '#92400e', text: '#ffffff' },
  'bank statement': { bg: '#047857', text: '#ffffff' },
  'tax document': { bg: '#065f46', text: '#ffffff' },
  'financial report': { bg: '#10b981', text: '#ffffff' },
  'investment update': { bg: '#34d399', text: '#064e3b' },
  'social notification': { bg: '#8b5cf6', text: '#ffffff' },
  'connection request': { bg: '#a78bfa', text: '#2e1065' },
  'mention': { bg: '#7c3aed', text: '#ffffff' },
  'comment': { bg: '#c4b5fd', text: '#2e1065' },
  'satire/joke': { bg: '#fbbf24', text: '#78350f' },
  'spam': { bg: '#991b1b', text: '#ffffff' },
  'phishing attempt': { bg: '#7f1d1d', text: '#fef2f2' },
  'auto-reply': { bg: '#9ca3af', text: '#1f2937' },
  'out of office': { bg: '#6b7280', text: '#ffffff' },
  'forwarded': { bg: '#a1a1aa', text: '#27272a' },
  'thread reply': { bg: '#d4d4d8', text: '#3f3f46' },
  'digest': { bg: '#71717a', text: '#ffffff' },
  'notification': { bg: '#6366f1', text: '#ffffff' },
  'confirmation': { bg: '#06b6d4', text: '#ffffff' },
  'other': { bg: '#52525b', text: '#ffffff' }
}

const CONFIDENCE_COLORS: Record<string, string> = {
  high: '#10b981',
  medium: '#f59e0b',
  low: '#ef4444'
}

export class UIService {
  static currentThemeName: ThemeName = 'default'
  static currentTheme: Theme = THEME_COLORS.default
  static listeners: ThemeListener[] = []

  static ANIMATION_STYLE_ID = 'metldr-animations'
  static POPUP_ANIMATION_STYLE_ID = 'metldr-word-popup-animations'

  static async loadFromStorage(): Promise<string> {
    try {
      const result = await chrome.storage.local.get('theme') as { theme?: string }
      const themeName = (result.theme || 'default') as ThemeName
      this.setTheme(themeName)
      return themeName
    } catch (error) {
      console.error('metldr: failed to load theme from storage:', error)
      this.currentThemeName = 'default'
      this.currentTheme = THEME_COLORS.default
      return 'default'
    }
  }

  static setTheme(themeName: string): void {
    const validName = (THEME_COLORS[themeName as ThemeName] ? themeName : 'default') as ThemeName
    this.currentThemeName = validName
    this.currentTheme = THEME_COLORS[validName]
    this.notifyListeners()
  }

  static onChange(callback: ThemeListener): () => void {
    this.listeners.push(callback)
    return () => {
      this.listeners = this.listeners.filter(cb => cb !== callback)
    }
  }

  static notifyListeners(): void {
    this.listeners.forEach(cb => cb(this.currentThemeName, this.currentTheme))
  }

  static hexToRgb(hex: string): string {
    const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex)
    return result 
      ? `${parseInt(result[1], 16)}, ${parseInt(result[2], 16)}, ${parseInt(result[3], 16)}`
      : hex
  }

  static init(): void {
    if (document.head) {
      this.injectSummaryAnimations()
      this.injectPopupAnimations()
    } else {
      document.addEventListener('DOMContentLoaded', () => {
        this.injectSummaryAnimations()
        this.injectPopupAnimations()
      })
    }
  }

  static injectSummaryAnimations(): void {
    if (!document.head) return
    if (document.getElementById(this.ANIMATION_STYLE_ID)) return

    const style = document.createElement('style')
    style.id = this.ANIMATION_STYLE_ID
    style.textContent = `
      @keyframes statusPulse {
        0%, 100% {
          opacity: 0.85;
          box-shadow: 0 0 8px currentColor, 0 0 12px currentColor;
        }
        50% {
          opacity: 1;
          box-shadow: 0 0 16px currentColor, 0 0 24px currentColor, 0 0 32px currentColor;
        }
      }
    `
    document.head.appendChild(style)
  }

  static injectPopupAnimations(): void {
    if (!document.head) return
    if (document.getElementById(this.POPUP_ANIMATION_STYLE_ID)) return

    const style = document.createElement('style')
    style.id = this.POPUP_ANIMATION_STYLE_ID
    style.textContent = `
      @keyframes metldr-popup-enter {
        from { opacity: 0; transform: scale(0.85) translateY(-6px); }
        to { opacity: 1; transform: scale(1) translateY(0); }
      }
      @keyframes metldr-fade-in {
        from { opacity: 0; transform: translateY(-8px); }
        to { opacity: 1; transform: translateY(0); }
      }
      @keyframes metldr-spin { to { transform: rotate(360deg); } }
      .metldr-definitions-scroll::-webkit-scrollbar { width: 4px; }
      .metldr-definitions-scroll::-webkit-scrollbar-track { background: transparent; }
      .metldr-definitions-scroll::-webkit-scrollbar-thumb { background: var(--metldr-primary, #00f0ff); border-radius: 2px; opacity: 0.5; }
      .metldr-definitions-scroll::-webkit-scrollbar-thumb:hover { opacity: 0.8; }
      .metldr-definitions-scroll { scrollbar-width: thin; scrollbar-color: var(--metldr-primary) transparent; }
    `
    document.head.appendChild(style)
  }

  static escapeHtml(text: string): string {
    const div = document.createElement('div')
    div.textContent = text
    return div.innerHTML
  }

  static formatTime(ms: number): string {
    return formatTime(ms)
  }

  static createLoadingIndicator(_threadElement: HTMLElement | Document): HTMLElement {
    const theme = this.currentTheme
    
    const loading = document.createElement('div')
    loading.className = 'metldr-loading'
    loading.style.cssText = `
      position: relative;
      margin: 16px 0;
      padding: 12px 16px;
      background: ${theme.bgSecondary};
      border: 1.5px solid ${theme.border};
      border-radius: 12px;
      box-shadow: 0 4px 12px ${theme.shadow};
      opacity: 0;
      transform: scale(0.985);
    `
    
    loading.innerHTML = `
      <div style="display: flex; align-items: center; gap: 12px;">
        <div class="metldr-spinner" style="
          width: 16px; height: 16px;
          border: 2.5px solid ${theme.border};
          border-top-color: ${theme.primary};
          border-radius: 50%;
          animation: spin 0.7s linear infinite;
        "></div>
        <span style="color: ${theme.text}; font-size: 13px; font-weight: 600; letter-spacing: 0.01em; -webkit-font-smoothing: antialiased;">generating summary...</span>
      </div>
      <style>@keyframes spin { to { transform: rotate(360deg); } }</style>
    `

    return loading
  }

  static createSummarizeButton(threadId: string, onClick: () => void): HTMLElement {
    const theme = this.currentTheme
    
    const button = document.createElement('div')
    button.className = 'metldr-summarize-btn'
    button.setAttribute('data-metldr-thread', threadId)
    button.style.cssText = `
      display: inline-flex;
      align-items: center;
      gap: 8px;
      margin: 12px 0;
      padding: 8px 14px;
      background: ${theme.bgSecondary};
      border: 1px solid ${theme.border};
      border-radius: 8px;
      box-shadow: 0 2px 6px ${theme.shadow};
      cursor: pointer;
      opacity: 0;
      transform: scale(0.985);
      transition: all 0.2s ease;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    `
    
    button.innerHTML = `
      <span style="
        color: ${theme.primary};
        font-size: 12px;
        font-weight: 600;
        letter-spacing: 0.01em;
      ">summarise email</span>
      <style>
        .metldr-summarize-btn:hover {
          border-color: ${theme.primary} !important;
          background: ${theme.bg} !important;
        }
        .metldr-summarize-btn:active {
          transform: scale(0.97) !important;
        }
      </style>
    `
    
    button.addEventListener('click', (e) => {
      e.preventDefault()
      e.stopPropagation()
      onClick()
    })
    
    return button
  }


  // shared dom injection for gmail thread elements
  private static _injectElement(emailHeader: Element, element: HTMLElement): boolean {
    if (!emailHeader) return false
    
    let emailContainer = emailHeader.parentNode as HTMLElement | null
    while (emailContainer && !emailContainer.classList?.contains('ii') && 
           !emailContainer.classList?.contains('nH') && 
           !emailContainer.classList?.contains('adn')) {
      emailContainer = emailContainer.parentNode as HTMLElement | null
      if (!emailContainer || emailContainer === document.body) {
        emailContainer = emailHeader.parentNode as HTMLElement
        break
      }
    }
    
    const bodyElement = emailContainer?.querySelector('.a3s.aiL') || 
                       emailContainer?.querySelector('.ii.gt') ||
                       emailContainer?.querySelector('[dir="ltr"]') ||
                       emailHeader.nextElementSibling
    
    let targetParent: HTMLElement | null = null
    if (bodyElement && bodyElement.parentNode) {
      targetParent = bodyElement.parentNode as HTMLElement
      targetParent.insertBefore(element, bodyElement)
    } else {
      targetParent = emailHeader.parentNode as HTMLElement
      targetParent.insertBefore(element, emailHeader.nextSibling)
    }
    
    if (targetParent) {
      targetParent.style.setProperty('overflow', 'visible', 'important')
    }
    
    return true
  }

  static injectLoading(emailHeader: Element, element: HTMLElement): HTMLElement {
    if (!this._injectElement(emailHeader, element)) {
      console.error('metldr: could not find email header for injection')
      return element
    }
    
    gsap.to(element, { opacity: 1, scale: 1, duration: 0.4, ease: 'back.out(1.7)' })
    
    // pulsing glow for loading states only
    if (element.classList.contains('metldr-loading')) {
      const primaryRgb = this.hexToRgb(this.currentTheme.primary)
      gsap.to(element, {
        boxShadow: `0 0 8px rgba(${primaryRgb}, 0.6), 0 0 16px rgba(${primaryRgb}, 0.4), 0 0 24px rgba(${primaryRgb}, 0.2)`,
        duration: 1.5, repeat: -1, yoyo: true, ease: 'sine.inOut'
      })
    }

    return element
  }

  static createSummaryCard(summary: Summary, threadId: string): HTMLElement {
    const theme = this.currentTheme
    const summaryText = summary.summary || ''
    const actions = (summary.action_items || [])
      .map(a => (typeof a === 'string' ? a.trim() : ''))
      .filter(a => a && a.toLowerCase() !== 'none')
    const dates = summary.dates || []
    const confidence = summary.confidence || 'medium'
    const modelName = summary.model || 'unknown'
    const intent = summary.intent || null

    const confColor = CONFIDENCE_COLORS[confidence] || '#6b7280'

    const summaryDiv = document.createElement('div')
    summaryDiv.className = 'metldr-summary'
    summaryDiv.setAttribute('data-metldr-thread', threadId)
    summaryDiv.setAttribute('data-metldr-injected', 'true')
    summaryDiv.setAttribute('data-metldr-persistent', 'true')
    summaryDiv.style.cssText = `
      opacity: 0 !important; transform: scale(0.985) !important; will-change: opacity, transform !important;
      transition: opacity 0.45s cubic-bezier(0.25, 0.46, 0.45, 0.94), transform 0.45s cubic-bezier(0.25, 0.46, 0.45, 0.94) !important;
      position: relative !important; z-index: 1 !important;
    `

    summaryDiv.innerHTML = this._buildSummaryHTML(theme, summaryText, actions, dates, confidence, confColor, modelName, summary, threadId, intent)

    return summaryDiv
  }

  static _buildSummaryHTML(theme: Theme, summaryText: string, actions: string[], dates: string[], confidence: string, confColor: string, modelName: string, summary: Summary, threadId: string, intent: string | null): string {
    const intentKey = intent?.toLowerCase()
    const intentStyle = (intentKey && INTENT_COLORS[intentKey]) ? INTENT_COLORS[intentKey] : { bg: theme.bgSecondary, text: theme.textMuted }
    const intentBadge = intent ? `
      <span style="color: ${theme.borderSubtle}; font-size: 11px; opacity: 0.5;">•</span>
      <span title="email type: ${this.escapeHtml(intent)}" style="font-size: 10px; color: ${intentStyle.text}; background: ${intentStyle.bg}; padding: 2px 6px; border-radius: 4px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.02em; cursor: help;">${this.escapeHtml(intent)}</span>
    ` : ''
    
    return `
      <div style="position: relative; width: 100%; box-sizing: border-box; margin: 24px 0 16px 0;">
        <div style="position: absolute; top: -12px; left: 16px; display: flex; align-items: center; gap: 8px; background: ${theme.bgSecondary}; padding: 6px 14px; border: 0.5px solid ${theme.borderSubtle}; border-radius: 12px; box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}; z-index: 1; -webkit-font-smoothing: antialiased;">
          <div style="display: flex; align-items: center; gap: 5px;">
            <div title="confidence: ${confidence}" style="width: 6px; height: 6px; background: ${confColor}; border-radius: 50%; box-shadow: 0 0 8px ${confColor}; cursor: help;"></div>
            <strong style="font-size: 12px; font-weight: 600; color: ${theme.primary}; letter-spacing: 0.01em;">metldr</strong>
          </div>
          <span style="color: ${theme.borderSubtle}; font-size: 11px; opacity: 0.5;">•</span>
          <span title="model used" style="font-size: 11px; color: ${theme.textMuted}; font-weight: 500; font-family: 'SF Mono', 'Courier New', monospace; cursor: help;">${this.escapeHtml(modelName)}</span>
          ${summary.time_ms ? `<span style="color: ${theme.borderSubtle}; font-size: 11px; opacity: 0.5;">•</span><span title="time taken: ${this.formatTime(summary.time_ms)}" style="font-size: 11px; color: ${theme.textMuted}; font-weight: 500; cursor: help;">${this.formatTime(summary.time_ms)}</span>` : ''}
          ${intentBadge}
        </div>
        <button class="metldr-regenerate-btn" data-thread-id="${threadId}" title="regenerate summary" style="position: absolute; top: -14px; right: 16px; width: 28px; height: 28px; display: flex; align-items: center; justify-content: center; background: ${theme.bgSecondary}; border: 0.5px solid ${theme.borderSubtle}; border-radius: 50%; color: ${theme.primary}; font-size: 15px; font-weight: 600; cursor: pointer; transition: all 0.2s cubic-bezier(0.22, 1, 0.36, 1); box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}; z-index: 1; padding: 0; -webkit-font-smoothing: antialiased;">↻</button>
        <div style="width: 100%; box-sizing: border-box; background: ${theme.bg}; border: 0.5px solid ${theme.border}; border-radius: 16px; padding: 16px; padding-top: 20px; font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif; box-shadow: 0 8px 32px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}; position: relative; -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; transition: all 0.3s cubic-bezier(0.22, 1, 0.36, 1);">
          ${summaryText ? `<div class="metldr-summary-item" style="background: ${theme.bgSecondary}; border-radius: 8px; padding: 10px; margin-bottom: ${actions.length > 0 || dates.length > 0 ? '6px' : '0'}; font-size: 13px; line-height: 1.5; color: ${theme.text}; font-weight: 400; -webkit-font-smoothing: antialiased;">${this.escapeHtml(summaryText)}</div>` : ''}
          ${actions.length > 0 ? `
            <div class="metldr-summary-item" style="margin-bottom: ${dates.length > 0 ? '6px' : '0'}; padding: 10px; background: ${theme.bgSecondary}; border-radius: 8px;">
              <div style="font-size: 10px; color: ${theme.secondary}; text-transform: uppercase; letter-spacing: 0.06em; font-weight: 600; margin-bottom: 8px;">action items</div>
              <ul style="margin: 0; padding-left: 0; list-style: none; color: ${theme.text}; line-height: 1.5; font-size: 13px;">
                ${actions.map(action => `<li style="padding-left: 14px; position: relative; margin-bottom: 5px; font-weight: 400;"><span style="position: absolute; left: 0; top: 4px; color: ${theme.secondary}; font-size: 7px;">▸</span>${this.escapeHtml(action)}</li>`).join('')}
              </ul>
            </div>
          ` : ''}
          ${dates.length > 0 ? `
            <div class="metldr-summary-item" style="font-size: 11px; color: ${theme.accent}; display: flex; flex-wrap: wrap; align-items: center; gap: 5px; font-weight: 500; -webkit-font-smoothing: antialiased;">
              ${dates.map(d => `<span style="background: ${theme.bgSecondary}; padding: 3px 8px; border-radius: 5px; font-size: 11px;">${this.escapeHtml(d)}</span>`).join('')}
            </div>
          ` : ''}
        </div>
      </div>
      <style>
        .metldr-regenerate-btn:hover { background: ${theme.bgSecondary} !important; border-color: ${theme.primary} !important; color: ${theme.primary} !important; transform: scale(1.15) rotate(90deg); box-shadow: 0 3px 8px ${theme.shadow}; }
        .metldr-regenerate-btn:active { background: ${theme.bg} !important; transform: scale(0.9) rotate(90deg); box-shadow: 0 1px 2px ${theme.shadow}; transition: all 0.1s cubic-bezier(0.4, 0, 1, 1); }
      </style>
    `
  }

  static injectSummary(emailHeader: Element, summaryDiv: HTMLElement): void {
    if (!this._injectElement(emailHeader, summaryDiv)) {
      console.error('metldr: could not find email header for summary')
      return
    }
    
    summaryDiv.style.setProperty('opacity', '0', 'important')
    summaryDiv.style.setProperty('transform', 'scale(0.985)', 'important')
    
    void summaryDiv.offsetHeight
    
    summaryDiv.style.setProperty('opacity', '1', 'important')
    summaryDiv.style.setProperty('transform', 'scale(1)', 'important')
    
    setTimeout(() => {
      summaryDiv.style.setProperty('will-change', 'auto', 'important')
    }, 500)
  }

  static updatePopupTheme(container: HTMLElement | null): void {
    if (!container) return

    const popup = container.querySelector('.metldr-popup-body') as HTMLElement | null
    if (!popup) return

    const theme = this.currentTheme
    const popupBg = theme.bgSecondary

    popup.style.setProperty('--metldr-primary', theme.primary, 'important')
    popup.style.setProperty('--metldr-secondary', theme.secondary, 'important')
    popup.style.setProperty('--metldr-text', theme.text, 'important')
    popup.style.setProperty('--metldr-bg-secondary', popupBg, 'important')
    popup.style.setProperty('--metldr-border', theme.border, 'important')

    popup.style.setProperty('background', popupBg, 'important')
    popup.style.setProperty('background-color', popupBg, 'important')
    popup.style.setProperty('border-color', theme.border, 'important')
    popup.style.setProperty('box-shadow', `0 8px 24px ${theme.shadow}, 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important')

    this._updatePopupHeader(popup, theme, popupBg)
    this._updatePopupContent(popup, theme, popupBg)
    this._updateDefinitions(popup, theme, popupBg)
    this._updateAiBadges(popup, theme)

    console.log('[popup] theme updated in real-time:', this.currentThemeName, 'using bg:', popupBg)
  }

  static _updatePopupHeader(popup: HTMLElement, theme: Theme, popupBg: string): void {
    const header = popup.querySelector('.metldr-popup-header') as HTMLElement | null
    if (header) {
      header.style.setProperty('background', popupBg, 'important')
      header.style.setProperty('background-color', popupBg, 'important')
    }

    const wordSpan = header?.querySelector('span') as HTMLElement | null
    if (wordSpan && wordSpan.textContent && wordSpan.textContent.length < 50 && !wordSpan.textContent.includes('ai')) {
      wordSpan.style.setProperty('color', theme.primary, 'important')
    }
  }

  static _updatePopupContent(popup: HTMLElement, theme: Theme, popupBg: string): void {
    const content = popup.querySelector('.metldr-popup-content') as HTMLElement | null
    if (content) {
      content.style.setProperty('background', popupBg, 'important')
      content.style.setProperty('background-color', popupBg, 'important')
      content.style.setProperty('color', theme.text, 'important')
    }
  }

  static _updateDefinitions(popup: HTMLElement, theme: Theme, popupBg: string): void {
    const defsContainer = popup.querySelector('.metldr-definitions-scroll') as HTMLElement | null
    if (defsContainer) {
      defsContainer.style.setProperty('background', popupBg, 'important')
      defsContainer.style.setProperty('background-color', popupBg, 'important')
    }

    const content = popup.querySelector('.metldr-popup-content')
    if (content) {
      const posElements = content.querySelectorAll('[data-element-type="pos"]')
      posElements.forEach(el => {
        const htmlEl = el as HTMLElement
        htmlEl.style.setProperty('color', theme.secondary, 'important')
        htmlEl.style.setProperty('border-bottom-color', theme.border, 'important')
      })

      const defElements = content.querySelectorAll('[data-element-type="definition"]')
      defElements.forEach(el => {
        const htmlEl = el as HTMLElement
        htmlEl.style.setProperty('color', theme.text, 'important')
      })

      const defBlocks = content.querySelectorAll('[data-element-type="def-block"]')
      defBlocks.forEach(block => {
        const htmlEl = block as HTMLElement
        htmlEl.style.setProperty('background', popupBg, 'important')
        htmlEl.style.setProperty('background-color', popupBg, 'important')
        htmlEl.style.setProperty('border-bottom-color', theme.border, 'important')
      })
    }
  }

  static _updateAiBadges(popup: HTMLElement, theme: Theme): void {
    const header = popup.querySelector('.metldr-popup-header')
    const aiBadge = header?.querySelector('span[title="ai generated definition"]') as HTMLElement | null
    if (aiBadge) {
      aiBadge.style.setProperty('color', theme.accent, 'important')
    }
  }

  static updateSummaryTheme(): void {
    const theme = this.currentTheme
    console.log('metldr: updating summary theme:', this.currentThemeName)

    this._updateSummarizeButton(theme)
    this._updateLoadingElement(theme)
    this._updateSummaryCard(theme)

    console.log('metldr: summary theme update completed')
  }

  static _updateLoadingElement(theme: Theme): void {
    const loadingElement = document.querySelector('.metldr-loading') as HTMLElement | null
    if (loadingElement) {
      loadingElement.style.setProperty('background', theme.bgSecondary, 'important')
      loadingElement.style.setProperty('background-color', theme.bgSecondary, 'important')
      loadingElement.style.setProperty('border-color', theme.border, 'important')
      loadingElement.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}`, 'important')

      const spinner = loadingElement.querySelector('.metldr-spinner') as HTMLElement | null
      if (spinner) {
        spinner.style.setProperty('border-color', theme.border, 'important')
        spinner.style.setProperty('border-top-color', theme.primary, 'important')
      }

      const loadingText = loadingElement.querySelector('span') as HTMLElement | null
      if (loadingText) {
        loadingText.style.setProperty('color', theme.text, 'important')
      }
    }
  }

  static _updateSummarizeButton(theme: Theme): void {
    const button = document.querySelector('.metldr-summarize-btn') as HTMLElement | null
    if (button) {
      button.style.setProperty('background', theme.bgSecondary, 'important')
      button.style.setProperty('border-color', theme.border, 'important')
      button.style.setProperty('box-shadow', `0 2px 6px ${theme.shadow}`, 'important')
      
      const span = button.querySelector('span') as HTMLElement | null
      if (span) {
        span.style.setProperty('color', theme.primary, 'important')
      }
    }
  }

  static _updateSummaryCard(theme: Theme): void {
    const existingSummary = document.querySelector('.metldr-summary')
    if (!existingSummary) return

    this._updateStatusBadge(existingSummary as HTMLElement, theme)
    this._updateRegenerateButton(existingSummary as HTMLElement, theme)
    this._updateMainCard(existingSummary as HTMLElement, theme)
  }

  static _updateStatusBadge(summaryCard: HTMLElement, theme: Theme): void {
    const statusBadge = summaryCard.querySelector('div[style*="position: absolute"]') as HTMLElement | null
    if (statusBadge) {
      statusBadge.style.setProperty('background', theme.bgSecondary, 'important')
      statusBadge.style.setProperty('background-color', theme.bgSecondary, 'important')
      statusBadge.style.setProperty('border-color', theme.borderSubtle, 'important')
      statusBadge.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important')

      const brandingText = statusBadge.querySelector('strong') as HTMLElement | null
      if (brandingText) {
        brandingText.style.setProperty('color', theme.primary, 'important')
      }

      const modelText = statusBadge.querySelector('span[title*="model used"]') as HTMLElement | null
      if (modelText) {
        modelText.style.setProperty('color', theme.textMuted, 'important')
      }

      const timeText = statusBadge.querySelector('span[title*="time taken"]') as HTMLElement | null
      if (timeText) {
        timeText.style.setProperty('color', theme.textMuted, 'important')
      }

      const separators = statusBadge.querySelectorAll('span[style*="opacity: 0.5"]')
      separators.forEach(sep => (sep as HTMLElement).style.setProperty('color', theme.borderSubtle, 'important'))
    }
  }

  static _updateRegenerateButton(summaryCard: HTMLElement, theme: Theme): void {
    const regenerateBtn = summaryCard.querySelector('.metldr-regenerate-btn') as HTMLElement | null
    if (regenerateBtn) {
      regenerateBtn.style.setProperty('background', theme.bgSecondary, 'important')
      regenerateBtn.style.setProperty('background-color', theme.bgSecondary, 'important')
      regenerateBtn.style.setProperty('border-color', theme.borderSubtle, 'important')
      regenerateBtn.style.setProperty('color', theme.primary, 'important')
      regenerateBtn.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important')
    }
  }

  static _updateMainCard(summaryCard: HTMLElement, theme: Theme): void {
    const mainCard = summaryCard.querySelector('div[style*="border-radius: 16px"]') as HTMLElement | null
    if (mainCard) {
      mainCard.style.setProperty('background', theme.bg, 'important')
      mainCard.style.setProperty('background-color', theme.bg, 'important')
      mainCard.style.setProperty('border-color', theme.border, 'important')
      mainCard.style.setProperty('box-shadow', `0 8px 32px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important')

      const summaryItems = summaryCard.querySelectorAll('.metldr-summary-item')
      summaryItems.forEach((item) => {
        this._updateSummaryItem(item as HTMLElement, theme)
      })
    }
  }

  static _updateSummaryItem(item: HTMLElement, theme: Theme): void {
    item.style.setProperty('background', theme.bgSecondary, 'important')
    item.style.setProperty('background-color', theme.bgSecondary, 'important')
    item.style.setProperty('color', theme.text, 'important')

    const actionHeader = item.querySelector('div[style*="text-transform: uppercase"]') as HTMLElement | null
    if (actionHeader) {
      actionHeader.style.setProperty('color', theme.secondary, 'important')
    }

    const bullets = item.querySelectorAll('span[style*="position: absolute"][style*="left: 0"]')
    bullets.forEach(bullet => {
      (bullet as HTMLElement).style.setProperty('color', theme.secondary, 'important')
    })

    const listItems = item.querySelectorAll('li')
    listItems.forEach(li => (li as HTMLElement).style.setProperty('color', theme.text, 'important'))

    const dateTags = item.querySelectorAll('span[style*="background:"]')
    dateTags.forEach(tag => {
      const htmlTag = tag as HTMLElement
      htmlTag.style.setProperty('background', theme.bgSecondary, 'important')
      htmlTag.style.setProperty('background-color', theme.bgSecondary, 'important')
      htmlTag.style.setProperty('color', theme.accent, 'important')
    })
  }
}

export const uiService = UIService
export const themeManager = UIService
