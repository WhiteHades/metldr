import { gsap } from 'gsap'
import { UIService } from './UIService'
import { emailExtractor } from './EmailExtractor'
import { languageService } from '../services/LanguageService'
import type { 
  InboxSDK, 
  ComposeView, 
  ThreadView, 
  ReplySuggestion, 
  FetchResponse, 
  Theme 
} from '../types'

export class ReplyPanel {
  private sdk: InboxSDK | null = null
  private panel: HTMLElement | null = null
  private triggerButton: HTMLElement | null = null
  private currentComposeView: ComposeView | null = null
  private currentThreadId: string | null = null
  private suggestions: ReplySuggestion[] = []
  private isVisible = false
  private themeUnsubscribe: (() => void) | null = null
  private isLoading = false
  private pollInterval: ReturnType<typeof setInterval> | null = null
  private isPopupMode = false
  private anchorElement: HTMLElement | null = null
  private scrollListener: (() => void) | null = null
  private resizeListener: (() => void) | null = null
  private isClosing = false
  private lastCloseTime = 0
  private escapeHandler: ((e: KeyboardEvent) => void) | null = null
  private outsideClickHandler: ((e: Event) => void) | null = null
  private lastKnownUrl: string = ''
  private urlCheckInterval: ReturnType<typeof setInterval> | null = null

  init(sdk: InboxSDK | null): void {
    this.sdk = sdk

    if (!sdk) {
      console.warn('metldr: no sdk provided, reply panel disabled')
      return
    }

    sdk.Compose.registerComposeViewHandler((composeView) => {
      this.handleComposeView(composeView)
    })

    // monitor url changes to detect thread navigation
    this.lastKnownUrl = window.location.href
    this.urlCheckInterval = setInterval(() => {
      const currentUrl = window.location.href
      if (currentUrl !== this.lastKnownUrl) {
        this.lastKnownUrl = currentUrl
        this.handleUrlChange()
      }
    }, 500)

    console.log('metldr: reply panel initialized with inboxsdk')
  }

  handleUrlChange(): void {
    const newThreadId = this.getThreadIdFromUrl()
    if (this.currentThreadId && newThreadId !== this.currentThreadId) {
      console.log('metldr: thread navigation detected, clearing suggestions')
      this.suggestions = []
      this.stopPolling()
      this.currentThreadId = null
      if (this.isVisible) {
        this.hidePanel()
      }
    }
  }

  async handleComposeView(composeView: ComposeView): Promise<void> {
    console.log('metldr: compose view detected')

    const isInline = composeView.isInlineReplyForm?.() ?? false
    const isPopup = !isInline
    console.log('metldr: compose mode:', isPopup ? 'popup' : 'inline')

    let threadId = this.getThreadIdFromUrl()
    console.log('metldr: url-based thread id:', threadId)

    if (!threadId) {
      const threadView = composeView.getThreadView?.()
      if (threadView) {
        threadId = await this.getThreadId(threadView)
      }
    }

    if (!threadId && composeView.getThreadID) {
      threadId = composeView.getThreadID()
      console.log('metldr: sdk thread id (fallback):', threadId)
    }
    
    if (!threadId) {
      console.log('metldr: new compose, skipping suggestions')
      return
    }

    // detect thread change and clear stale state
    if (this.currentThreadId && this.currentThreadId !== threadId) {
      console.log('metldr: thread changed from', this.currentThreadId, 'to', threadId)
      this.suggestions = []
      this.stopPolling()
    }

    console.log('metldr: compose thread id:', threadId)

    this.currentComposeView = composeView
    this.currentThreadId = threadId
    this.isPopupMode = isPopup

    this.addComposeButton(composeView)

    // generate replies in background (don't block UI)
    this.generateRepliesAsync(threadId)

    composeView.on('destroy', () => {
      if (this.currentComposeView === composeView) {
        this.stopPolling()
        this.hide()
        this.currentComposeView = null
        this.isPopupMode = false
      }
    })
  }

  private async generateRepliesAsync(threadId: string): Promise<void> {
    const cachedResponse = await this.fetchSuggestions(threadId)
    if (cachedResponse?.success && cachedResponse.suggestions?.length) {
      this.suggestions = cachedResponse.suggestions
      console.log('metldr: cached suggestions loaded instantly:', this.suggestions.length)
      this.onSuggestionsReady()
      return
    }

    const chromeReplies = await this.tryChromeReplies()
    if (chromeReplies?.length) {
      this.suggestions = chromeReplies
      console.log('metldr: chrome ai generated', chromeReplies.length, 'replies')
      this.onSuggestionsReady()
    } else {
      console.log('metldr: no cached/chrome suggestions, polling for background generation')
      this.startPolling(threadId)
    }
  }

  private onSuggestionsReady(): void {
    this.isLoading = false
    if (this.isVisible && this.panel) {
      this.updatePanelWithSuggestions()
    }
  }

  async tryChromeReplies(): Promise<ReplySuggestion[] | null> {
    try {
      if (typeof Writer === 'undefined') {
        console.log('metldr: Writer API not available')
        return null
      }

      const avail = await Writer.availability()
      if (avail === 'unavailable') {
        console.log('metldr: Writer not available')
        return null
      }

      console.log('metldr: using Chrome AI Writer for replies')

      const emailContext = this.getEmailContext()
      
      const detectedLang = await languageService.detect(emailContext?.body || emailContext?.subject || '')
      console.log('metldr: using language for replies:', detectedLang)
      
      // build context with full email body for better replies
      const emailBody = emailContext?.body || ''
      const sharedContext = emailContext 
        ? `You are writing a REPLY to an email (not a new email). From: ${emailContext.from || 'unknown'}. Subject: ${emailContext.subject || 'unknown'}. NEVER include "Subject:" lines - this is a reply.`
        : 'You are writing an email REPLY. NEVER include Subject lines.'

      const replies: ReplySuggestion[] = []
      const tones: Array<{ tone: 'formal' | 'neutral' | 'casual', label: string }> = [
        { tone: 'formal', label: 'professional' },
        { tone: 'casual', label: 'friendly' },
        { tone: 'neutral', label: 'brief' }
      ]

      const supportedLanguages = languageService.getSupportedLanguages()

      for (const { tone, label } of tones) {
        try {
          const writer = await Writer.create({
            tone,
            length: 'short',
            format: 'plain-text',
            sharedContext,
            expectedInputLanguages: supportedLanguages,
            expectedContextLanguages: supportedLanguages,
            outputLanguage: detectedLang
          })

          try {
            const prompt = emailBody 
              ? `Write a ${label} reply to this email. Do NOT include subject lines.`
              : `Write a ${label} email reply`
            const body = await writer.write(prompt, { context: emailBody })
            
            if (body && body.length > 10) {
              replies.push({
                id: `chrome_${label}`,
                tone: label,
                length: 'short',
                body: body.trim(),
                label
              })
            }
          } finally {
            writer.destroy()
          }
        } catch (err) {
          console.log(`metldr: chrome writer ${tone} failed:`, (err as Error).message)
        }
      }

      return replies.length > 0 ? replies : null
    } catch (err) {
      console.log('metldr: chrome ai replies failed:', (err as Error).message)
      return null
    }
  }

  // get email context from emailExtractor's last extracted content (same data used for summaries)
  private getEmailContext(): { subject?: string; from?: string; body?: string } | null {
    try {
      // emailExtractor stores the last extracted email content used for summaries
      const data = emailExtractor.getLastExtracted()
      if (data) {
        return {
          subject: data.metadata?.subject,
          from: data.metadata?.from,
          body: data.content ?? undefined
        }
      }
    } catch { /* ignore */ }
    return null
  }

  startPolling(threadId: string): void {
    this.stopPolling()
    let attempts = 0
    const maxAttempts = 20
    
    this.pollInterval = setInterval(async () => {
      attempts++
      
      // verify we're still on the same thread before polling
      const currentThreadId = this.getThreadIdFromUrl()
      if (currentThreadId && currentThreadId !== threadId) {
        console.log('metldr: thread changed during polling, stopping')
        this.stopPolling()
        this.suggestions = []
        return
      }
      
      const response = await this.fetchSuggestions(threadId)
      
      if (response?.contextInvalidated) {
        console.log('metldr: stopping poll (extension reloaded)')
        this.stopPolling()
        return
      }
      
      if (response?.success && response.suggestions?.length) {
        // double-check thread hasn't changed while we were waiting
        const nowThreadId = this.getThreadIdFromUrl()
        if (nowThreadId !== threadId) {
          console.log('metldr: thread changed after poll response, discarding')
          return
        }
        
        this.suggestions = response.suggestions
        console.log('metldr: suggestions ready after polling:', this.suggestions.length)
        this.stopPolling()
        
        if (this.isVisible && this.panel && this.isLoading) {
          this.updatePanelWithSuggestions()
        }
      } else if (attempts >= maxAttempts) {
        console.log('metldr: gave up polling for suggestions')
        this.stopPolling()
        if (this.isVisible && this.panel && this.isLoading) {
          this.updatePanelWithEmptyState()
        }
      }
    }, 1000)
  }

  updatePanelWithEmptyState(): void {
    if (!this.panel) return
    this.isLoading = false
    const content = this.panel.querySelector('.metldr-reply-content')
    if (content) {
      const theme = UIService.currentTheme as Theme
      content.innerHTML = `
        <div style="
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          padding: 24px 12px;
          gap: 12px;
        ">
          <span style="font-size: 12px; color: ${theme.textMuted};">
            no suggestions available yet
          </span>
          <button class="metldr-retry-btn" style="
            font-size: 11px;
            color: ${theme.primary};
            background: ${theme.bgSecondary};
            border: 1px solid ${theme.borderSubtle};
            padding: 6px 12px;
            border-radius: 6px;
            cursor: pointer;
          ">retry</button>
        </div>
      `
      const retryBtn = content.querySelector('.metldr-retry-btn')
      if (retryBtn) {
        retryBtn.addEventListener('click', () => {
          if (this.currentThreadId) {
            this.forceRegenerateSuggestions(this.currentThreadId)
          }
        })
      }
    }
  }

  refreshForThread(threadId: string): void {
    console.log('metldr: refreshing suggestions for thread:', threadId)
    this.stopPolling()
    this.suggestions = []
    this.isLoading = true

    if (this.isVisible && this.panel) {
      const content = this.panel.querySelector('.metldr-reply-content')
      if (content) {
        content.innerHTML = this.buildSuggestionsHTML(UIService.currentTheme as Theme)
      }
    }

    if (threadId) {
      // request force regeneration from background
      this.forceRegenerateSuggestions(threadId)
    }
  }

  async forceRegenerateSuggestions(threadId: string): Promise<void> {
    try {
      const response = await new Promise<FetchResponse | undefined>((resolve, reject) => {
        chrome.runtime.sendMessage({
          type: 'GENERATE_REPLY_SUGGESTIONS',
          emailId: threadId,
          forceRegenerate: true
        }, (resp) => {
          if (chrome.runtime.lastError) {
            reject(new Error(chrome.runtime.lastError.message))
          } else {
            resolve(resp as FetchResponse | undefined)
          }
        })
      })

      if (response?.success && response.suggestions?.length) {
        // verify thread hasn't changed
        const currentThreadId = this.getThreadIdFromUrl()
        if (currentThreadId === threadId) {
          this.suggestions = response.suggestions
          this.isLoading = false
          console.log('metldr: force regenerated suggestions:', this.suggestions.length)
          if (this.isVisible && this.panel) {
            this.updatePanelWithSuggestions()
          }
        }
      } else {
        // fall back to polling
        console.log('metldr: force regen returned no suggestions, polling...')
        this.startPolling(threadId)
      }
    } catch (err) {
      console.error('metldr: force regeneration failed:', (err as Error).message)
      this.startPolling(threadId)
    }
  }

  stopPolling(): void {
    if (this.pollInterval) {
      clearInterval(this.pollInterval)
      this.pollInterval = null
    }
  }

  async getThreadId(threadView: ThreadView): Promise<string | null> {
    if (threadView.getThreadIDAsync) {
      try {
        return await threadView.getThreadIDAsync()
      } catch {
        // fallback
      }
    }
    return this.getThreadIdFromUrl()
  }

  getThreadIdFromUrl(): string | null {
    const hash = window.location.hash
    const match = hash.match(/#[^/]+\/(?:[^/]+\/)?([A-Za-z0-9_-]{16,})$/)
    return match ? match[1] : null
  }

  addComposeButton(composeView: ComposeView): void {
    composeView.addButton({
      title: 'metldr replies',
      iconUrl: this.getIconDataUrl(),
      orderHint: 100,
      onClick: () => {
        console.log('metldr: compose button clicked')
        this.showSuggestionsPanel(composeView)
      }
    })

    console.log('metldr: added compose button')
  }

  getIconDataUrl(): string {
    return chrome.runtime.getURL('assets/logo/metldr-dark-lavender-256x256.png')
  }

  showSuggestionsPanel(composeView: ComposeView): void {
    if (this.isClosing) return
    
    const timeSinceClose = Date.now() - this.lastCloseTime
    if (timeSinceClose < 200) {
      console.log('metldr: ignoring reopen (too soon after close)')
      return
    }
    
    if (this.isVisible && this.panel) {
      this.animateOut()
      this.isVisible = false
      return
    }

    this.isLoading = !this.suggestions?.length
    console.log('metldr: creating suggestions panel, isPopup:', this.isPopupMode, 'loading:', this.isLoading)
    
    try {
      this.anchorElement = composeView.getElement?.() || null
    } catch {
      this.anchorElement = null
    }
    
    this.createPanel(composeView)
    this.updatePosition()
    this.setupPositionListeners()
    this.animateIn()
    this.subscribeToTheme()
    this.isVisible = true
    console.log('metldr: panel shown, position:', this.panel?.style.left, this.panel?.style.top)
  }

  updatePanelWithSuggestions(): void {
    if (!this.panel || !this.suggestions?.length) return
    
    this.isLoading = false
    const content = this.panel.querySelector('.metldr-reply-content')
    if (content) {
      content.innerHTML = this.buildSuggestionsHTML(UIService.currentTheme as Theme)
      this.attachOptionListeners(content as HTMLElement, this.currentComposeView)
    }
  }

  async fetchSuggestions(emailId: string, retries = 2): Promise<FetchResponse> {
    for (let attempt = 0; attempt <= retries; attempt++) {
      try {
        const resp = await new Promise<FetchResponse | undefined>((resolve, reject) => {
          chrome.runtime.sendMessage({
            type: 'GET_REPLY_SUGGESTIONS',
            emailId
          }, (resp) => {
            if (chrome.runtime.lastError) {
              reject(new Error(chrome.runtime.lastError.message))
            } else {
              resolve(resp as FetchResponse | undefined)
            }
          })
        })
        
        if (resp === undefined && attempt < retries) {
          console.log('metldr: empty response, retrying...', attempt + 1)
          await new Promise(r => setTimeout(r, 200))
          continue
        }
        
        return resp || { success: false }
      } catch (err) {
        const error = err as Error
        if (error.message.includes('Extension context invalidated')) {
          console.log('metldr: extension reloaded, stopping suggestions fetch')
          return { success: false }
        }
        
        const isRetryable = error.message.includes('Receiving end does not exist') ||
                            error.message.includes('message port closed')
        if (attempt < retries && isRetryable) {
          console.log('metldr: background not ready, retrying...', attempt + 1)
          await new Promise(r => setTimeout(r, 200))
          continue
        }
        console.error('metldr: fetch suggestions failed:', error.message)
        return { success: false }
      }
    }
    return { success: false }
  }

  createPanel(composeView: ComposeView): void {
    const theme = UIService.currentTheme as Theme

    this.panel = document.createElement('div')
    this.panel.className = 'metldr-reply-panel'
    
    this.panel.style.cssText = `
      position: fixed !important;
      z-index: 999999 !important;
      opacity: 0;
      transform-origin: top left;
      will-change: opacity, transform;
    `

    const card = document.createElement('div')
    card.className = 'metldr-reply-card'
    card.style.cssText = `
      position: relative;
      background: ${theme.bg};
      border: 0.5px solid ${theme.border};
      border-radius: 14px;
      box-shadow: 0 8px 32px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
      font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
      min-width: 280px;
      max-width: 320px;
      -webkit-font-smoothing: antialiased;
    `

    // badge
    const badge = document.createElement('div')
    badge.className = 'metldr-reply-badge'
    badge.style.cssText = `
      position: absolute;
      top: -11px;
      left: 14px;
      display: flex;
      align-items: center;
      gap: 6px;
      background: ${theme.bgSecondary};
      padding: 5px 12px;
      border: 0.5px solid ${theme.borderSubtle};
      border-radius: 10px;
      box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
      z-index: 1;
    `
    badge.innerHTML = `
      <div style="width: 6px; height: 6px; background: ${theme.primary}; border-radius: 50%; box-shadow: 0 0 6px ${theme.primary};"></div>
      <span style="font-size: 11px; font-weight: 700; color: ${theme.primary}; letter-spacing: 0.01em;">metldr</span>
      <span style="font-size: 11px; color: ${theme.textMuted}; opacity: 0.5;">/</span>
      <span style="font-size: 11px; color: ${theme.textMuted};">replies</span>
    `

    // close button
    const closeBtn = document.createElement('button')
    closeBtn.className = 'metldr-reply-close'
    closeBtn.textContent = '×'
    closeBtn.style.cssText = `
      position: absolute;
      top: -12px;
      right: 14px;
      width: 24px;
      height: 24px;
      display: flex;
      align-items: center;
      justify-content: center;
      background: ${theme.bgSecondary};
      border: 0.5px solid ${theme.borderSubtle};
      border-radius: 50%;
      color: ${theme.primary};
      font-size: 14px;
      font-weight: 500;
      cursor: pointer;
      z-index: 1;
      transition: all 0.2s cubic-bezier(0.22, 1, 0.36, 1);
      box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
      line-height: 1;
    `
    closeBtn.addEventListener('click', (e) => {
      e.stopPropagation()
      this.hidePanel()
    })

    // content
    const content = document.createElement('div')
    content.className = 'metldr-reply-content'
    content.style.cssText = `
      padding: 20px 12px 10px 12px;
      max-height: 240px;
      overflow-y: auto;
    `

    // styles
    const style = document.createElement('style')
    style.textContent = `
      .metldr-reply-content::-webkit-scrollbar { width: 4px; }
      .metldr-reply-content::-webkit-scrollbar-track { background: transparent; }
      .metldr-reply-content::-webkit-scrollbar-thumb { background: ${theme.border}; border-radius: 2px; }
      .metldr-reply-content::-webkit-scrollbar-thumb:hover { background: ${theme.textMuted}; }
      .metldr-reply-close:hover {
        background: ${theme.bgSecondary} !important;
        border-color: ${theme.primary} !important;
        color: ${theme.primary} !important;
        transform: scale(1.1);
      }
    `

    content.innerHTML = this.buildSuggestionsHTML(theme)

    card.appendChild(badge)
    card.appendChild(closeBtn)
    card.appendChild(style)
    card.appendChild(content)
    this.panel.appendChild(card)

    this.attachOptionListeners(content, composeView)
    this.setupEventHandlers()

    document.body.appendChild(this.panel)
  }

  buildSuggestionsHTML(theme: Theme): string {
    if (this.isLoading || !this.suggestions?.length) {
      return `
        <div style="
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          padding: 24px 12px;
          gap: 12px;
        ">
          <div style="
            width: 24px;
            height: 24px;
            border: 2px solid ${theme.borderSubtle};
            border-top-color: ${theme.primary};
            border-radius: 50%;
            animation: metldr-spin 0.8s linear infinite;
          "></div>
          <span style="
            font-size: 12px;
            color: ${theme.textMuted};
          ">generating replies...</span>
        </div>
        <style>
          @keyframes metldr-spin {
            0% { transform: rotate(0deg); }
            100% { transform: rotate(360deg); }
          }
        </style>
      `
    }
    
    const limited = this.suggestions.slice(0, 4)
    return limited.map((s, idx) => `
      <div class="metldr-reply-option" data-reply-idx="${idx}" style="
        position: relative;
        padding: 10px;
        padding-top: 14px;
        margin-bottom: ${idx < limited.length - 1 ? '8px' : '0'};
        background: ${theme.bgSecondary};
        border: 0.5px solid ${theme.borderSubtle};
        border-radius: 8px;
        cursor: pointer;
        transition: border-color 0.15s ease, box-shadow 0.15s ease, transform 0.15s ease;
      ">
        <span style="
          position: absolute;
          top: -8px;
          left: 10px;
          padding: 2px 8px;
          background: ${theme.bgSecondary};
          border: 0.5px solid ${theme.borderSubtle};
          color: ${theme.primary};
          font-size: 9px;
          font-weight: 600;
          border-radius: 4px;
          text-transform: lowercase;
          letter-spacing: 0.02em;
        ">${UIService.escapeHtml(s.tone || 'reply')}</span>
        <span style="
          display: block;
          color: ${theme.text};
          font-size: 12px;
          line-height: 1.5;
          display: -webkit-box;
          -webkit-line-clamp: 2;
          -webkit-box-orient: vertical;
          overflow: hidden;
        ">${UIService.escapeHtml(this.truncatePreview(s.body, 100))}</span>
      </div>
    `).join('')
  }

  attachOptionListeners(content: HTMLElement, composeView: ComposeView | null): void {
    const options = content.querySelectorAll('.metldr-reply-option')

    options.forEach(option => {
      const el = option as HTMLElement
      const handleEnter = () => {
        const t = UIService.currentTheme as Theme
        el.style.borderColor = t.primary
        el.style.boxShadow = `0 2px 8px ${t.shadow}`
        el.style.transform = 'translateY(-1px)'
      }

      const handleLeave = () => {
        const t = UIService.currentTheme as Theme
        el.style.borderColor = t.borderSubtle
        el.style.boxShadow = 'none'
        el.style.transform = 'none'
      }

      el.addEventListener('mouseenter', handleEnter)
      el.addEventListener('mouseleave', handleLeave)
      el.addEventListener('pointerleave', handleLeave)

      el.addEventListener('click', (e) => {
        e.stopPropagation()
        const idx = parseInt(el.getAttribute('data-reply-idx') || '0', 10)
        const suggestion = this.suggestions[idx]
        if (suggestion && composeView) {
          this.insertReply(composeView, suggestion.body)
        }
      })
    })
  }

  insertReply(composeView: ComposeView, text: string): void {
    if (!composeView) {
      console.error('metldr: no compose view available')
      return
    }

    try {
      const htmlContent = text
        .split('\n')
        .map(line => `<div>${line || '<br>'}</div>`)
        .join('')

      composeView.setBodyHTML(htmlContent)
      console.log('metldr: inserted reply via sdk')
    } catch (err) {
      console.error('metldr: failed to insert reply:', err)
      try {
        composeView.insertTextIntoBodyAtCursor(text)
      } catch (e) {
        console.error('metldr: fallback insert also failed:', e)
      }
    }
  }

  updatePosition(): void {
    if (!this.panel || this.isClosing) return

    try {
      const panelRect = this.panel.getBoundingClientRect()
      const panelWidth = panelRect.width || 320
      const panelHeight = panelRect.height || 240
      
      const viewportWidth = window.innerWidth
      const viewportHeight = window.innerHeight
      const padding = 12

      let finalX: number, finalY: number

      if (this.anchorElement) {
        const anchorRect = this.anchorElement.getBoundingClientRect()
        
        // find compose body for better alignment
        const bodyEl = this.anchorElement.querySelector('[role="textbox"], [contenteditable="true"], .editable')
        const bodyRect = bodyEl ? bodyEl.getBoundingClientRect() : anchorRect
        
        // center horizontally on the compose body
        const anchorCenterX = bodyRect.left + (bodyRect.width / 2)
        finalX = anchorCenterX - (panelWidth / 2)
        
        // try above compose first
        const aboveY = anchorRect.top - panelHeight - padding
        const belowY = anchorRect.bottom + padding

        if (aboveY >= padding) {
          finalY = aboveY
        } else if (belowY + panelHeight < viewportHeight - padding) {
          finalY = belowY
        } else if (this.isPopupMode) {
          const leftX = anchorRect.left - panelWidth - padding
          const rightX = anchorRect.right + padding
          
          if (leftX >= padding) {
            finalX = leftX
            finalY = Math.max(padding, anchorRect.top)
          } else if (rightX + panelWidth < viewportWidth - padding) {
            finalX = rightX
            finalY = Math.max(padding, anchorRect.top)
          } else {
            finalX = anchorRect.left + 20
            finalY = anchorRect.top + 20
          }
        } else {
          finalY = Math.max(padding, Math.min(aboveY, viewportHeight - panelHeight - padding))
        }
      } else {
        finalX = (viewportWidth - panelWidth) / 2
        finalY = (viewportHeight - panelHeight) / 2
      }

      // viewport bounds clamping
      if (finalX < padding) {
        finalX = padding
      } else if (finalX + panelWidth > viewportWidth - padding) {
        finalX = viewportWidth - panelWidth - padding
      }

      if (finalY < padding) {
        finalY = padding
      } else if (finalY + panelHeight > viewportHeight - padding) {
        finalY = viewportHeight - panelHeight - padding
      }

      this.panel.style.position = 'fixed'
      this.panel.style.left = finalX + 'px'
      this.panel.style.top = finalY + 'px'
    } catch (error) {
      console.log('metldr: error updating panel position:', (error as Error).message)
    }
  }

  setupPositionListeners(): void {
    this.scrollListener = () => this.updatePosition()
    this.resizeListener = () => this.updatePosition()

    window.addEventListener('scroll', this.scrollListener, { capture: true, passive: true })
    window.addEventListener('resize', this.resizeListener)
  }

  removePositionListeners(): void {
    if (this.scrollListener) {
      window.removeEventListener('scroll', this.scrollListener, true)
      this.scrollListener = null
    }
    if (this.resizeListener) {
      window.removeEventListener('resize', this.resizeListener)
      this.resizeListener = null
    }
  }

  setupEventHandlers(): void {
    this.escapeHandler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') this.hidePanel()
    }
    document.addEventListener('keydown', this.escapeHandler)

    this.outsideClickHandler = (e: Event) => {
      if (!this.panel || !this.isVisible) return
      
      if (!this.panel.contains(e.target as Node)) {
        requestAnimationFrame(() => {
          if (this.isVisible && this.panel) this.hidePanel()
        })
      }
    }
    setTimeout(() => {
      document.addEventListener('pointerdown', this.outsideClickHandler!, true)
    }, 150)
  }

  truncatePreview(text: string, maxLen = 60): string {
    if (!text) return ''
    const cleaned = text.replace(/\n+/g, ' ').trim()
    return cleaned.length > maxLen ? cleaned.substring(0, maxLen - 3) + '...' : cleaned
  }

  hide(): void {
    if (this.panel) this.animateOut()
    if (this.themeUnsubscribe) {
      this.themeUnsubscribe()
      this.themeUnsubscribe = null
    }
    this.isVisible = false
    this.currentThreadId = null
  }

  hidePanel(): void {
    if (this.panel) this.animateOut()
    this.isVisible = false
    this.lastCloseTime = Date.now()
  }

  animateIn(): void {
    if (!this.panel) return
    
    gsap.fromTo(this.panel,
      { opacity: 0, scale: 0.95 },
      { opacity: 1, scale: 1, duration: 0.15, ease: 'power2.out' }
    )
  }

  animateOut(): void {
    if (!this.panel || this.isClosing) return
    
    this.isClosing = true
    this.removePositionListeners()
    this.anchorElement = null
    
    this.panel.style.pointerEvents = 'none'
    
    gsap.to(this.panel, {
      opacity: 0,
      duration: 0.1,
      ease: 'power2.in',
      onComplete: () => this.cleanup()
    })
  }

  cleanup(): void {
    if (this.panel) {
      this.panel.remove()
      this.panel = null
    }
    if (this.outsideClickHandler) {
      document.removeEventListener('pointerdown', this.outsideClickHandler, true)
      this.outsideClickHandler = null
    }
    if (this.escapeHandler) {
      document.removeEventListener('keydown', this.escapeHandler)
      this.escapeHandler = null
    }
    
    this.isClosing = false
  }

  subscribeToTheme(): void {
    this.themeUnsubscribe = UIService.onChange(() => {
      if (this.panel) this.updateTheme()
    })
  }

  updateTheme(): void {
    if (!this.panel) return
    const theme = UIService.currentTheme as Theme

    const card = this.panel.querySelector('.metldr-reply-card') as HTMLElement | null
    if (card) {
      card.style.background = theme.bg
      card.style.borderColor = theme.border
      card.style.boxShadow = `0 6px 24px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`
    }

    const badge = this.panel.querySelector('.metldr-reply-badge') as HTMLElement | null
    if (badge) {
      badge.style.background = theme.bgSecondary
      badge.style.borderColor = theme.borderSubtle
    }

    const closeBtn = this.panel.querySelector('.metldr-reply-close') as HTMLElement | null
    if (closeBtn) {
      closeBtn.style.background = theme.bgSecondary
      closeBtn.style.borderColor = theme.borderSubtle
      closeBtn.style.color = theme.textMuted
    }

    const options = this.panel.querySelectorAll('.metldr-reply-option')
    options.forEach(opt => {
      const option = opt as HTMLElement
      option.style.background = theme.bgSecondary
      option.style.borderColor = theme.borderSubtle
    })
  }
}

export const replyPanel = new ReplyPanel()
