import { UIService } from './UIService'
import { replyPanel } from './ReplyPanel'
import { languageService } from '../services/LanguageService'
import type { 
  ThreadView, 
  MessageView, 
  Contact, 
  EmailMetadata as BaseEmailMetadata,
  SummaryResponse,
  HealthResponse,
  EmailProcessCallback 
} from '../types'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type InboxSDK = any

interface EmailMetadata extends Omit<BaseEmailMetadata, 'participants' | 'emailCount'> {
  subject: string
  participants: string[]
  emailCount: number
  timestamp: string
  toList: string[]
  ccList: string[]
  bccList: string[]
}

export class EmailExtractor {
  private sdk: InboxSDK | null = null
  private onEmailProcess: EmailProcessCallback | null = null
  private isProcessing = false
  private isRegenerating = false
  private processingTimeout = 30000
  private processedThreads: Set<string> = new Set()
  private activeRequestToken: string | null = null
  private lastExtractedContent: { threadId: string; content: string; metadata: EmailMetadata } | null = null

  // get last extracted content (used by background for regeneration)
  getLastExtracted(): { threadId: string; content: string; metadata: EmailMetadata } | null {
    return this.lastExtractedContent
  }

  async getCachedSummary(emailId: string): Promise<unknown> {
    if (!chrome?.runtime?.sendMessage) return null
    try {
      return await new Promise((resolve) => {
        chrome.runtime.sendMessage({ type: 'GET_EMAIL_CACHE', emailId }, (resp) => {
          if (chrome.runtime.lastError) {
            resolve(null)
          } else {
            resolve(resp?.cached || null)
          }
        })
      })
    } catch {
      return null
    }
  }

  async cacheSummary(emailId: string, summary: unknown): Promise<void> {
    if (!chrome?.runtime?.sendMessage) return
    try {
      await new Promise<void>((resolve) => {
        chrome.runtime.sendMessage({ type: 'SET_EMAIL_CACHE', emailId, summary }, () => {
          resolve()
        })
      })
    } catch {
    }
  }

  async tryChromeSummary(emailText: string, metadata: EmailMetadata): Promise<unknown> {
    const startTime = Date.now()
    try {
      if (typeof Summarizer === 'undefined') {
        console.log('metldr: Summarizer API not available')
        return null
      }

      const avail = await Summarizer.availability()
      if (avail === 'unavailable') {
        console.log('metldr: Summarizer not available')
        return null
      }

      console.log('metldr: using Chrome AI for email summary')

      const detectedLang = await languageService.detect(emailText.substring(0, 500))
      console.log('metldr: detected email language:', detectedLang)

      let context = ''
      if (metadata.subject) context += `Subject: ${metadata.subject}\n`
      if (metadata.from) context += `From: ${metadata.from}\n`
      if (metadata.to) context += `To: ${metadata.to}\n`
      if (metadata.date) context += `Date: ${metadata.date}\n`

      const supportedLanguages = languageService.getSupportedLanguages()

      const summarizer = await Summarizer.create({
        type: 'key-points',
        length: 'medium',
        format: 'markdown',
        expectedInputLanguages: supportedLanguages,
        expectedContextLanguages: supportedLanguages,
        outputLanguage: detectedLang
      })

      try {
        const result = await summarizer.summarize(emailText, { context })
        const elapsed = Date.now() - startTime
        
        // parse chrome ai result into our format
        const bullets = result
          .split('\n')
          .map((l: string) => l.trim())
          .filter((l: string) => /^[-•*]|^\d+\./.test(l))
          .map((l: string) => l.replace(/^[-•*]\s*/, '').replace(/^\d+\.\s*/, '').trim())
          .filter((l: string) => l.length > 10)
          .slice(0, 5)

        // determine intent from content heuristics
        let intent = 'Other'
        const text = emailText.toLowerCase()
        if (text.includes('invoice') || text.includes('receipt')) intent = 'Invoice'
        else if (text.includes('booking') || text.includes('reservation') || text.includes('flight')) intent = 'Travel Itinerary'
        else if (text.includes('meeting') || text.includes('calendar')) intent = 'Meeting Request'
        else if (text.includes('order') || text.includes('shipped') || text.includes('tracking')) intent = 'Order Confirmation'
        else if (text.includes('password') || text.includes('security') || text.includes('login')) intent = 'Security Alert'

        return {
          summary: bullets.length > 0 ? bullets.join(' ') : result.substring(0, 200),
          action_items: [],
          dates: [],
          key_facts: { booking_reference: null, amount: null, sender_org: null },
          intent,
          urgency: 'normal',
          model: 'gemini-nano',
          provider: 'chrome-ai',
          time_ms: elapsed
        }
      } finally {
        summarizer.destroy()
      }
    } catch (err) {
      console.log('metldr: chrome ai summary failed:', (err as Error).message)
      return null
    }
  }

  setProcessCallback(callback: EmailProcessCallback): void {
    this.onEmailProcess = callback
  }

  init(sdk: InboxSDK | null, onEmailProcess: EmailProcessCallback): void {
    this.sdk = sdk
    this.onEmailProcess = onEmailProcess

    if (!sdk) {
      console.warn('metldr: no sdk provided, email extraction disabled')
      return
    }

    sdk.Conversations.registerThreadViewHandler((threadView: ThreadView) => {
      this.handleThreadView(threadView)
    })

    replyPanel.init(sdk)

    console.log('metldr: email extractor initialized with inboxsdk')
  }

  async handleThreadView(threadView: ThreadView): Promise<void> {
    let threadId = this.getThreadIdFromUrl()
    
    if (!threadId && threadView.getThreadIDAsync) {
      try {
        threadId = await threadView.getThreadIDAsync()
      } catch { // ignore
      }
    }

    if (!threadId) {
      console.log('metldr: no thread id available')
      return
    }

    if (this.isProcessing) {
      console.log('metldr: already processing, skipping')
      return
    }

  
    const existingSummary = document.querySelector('.metldr-summary')
    if (existingSummary) {
      const existingThreadId = existingSummary.getAttribute('data-metldr-thread')
      if (existingThreadId === threadId) {
        console.log('metldr: summary already exists for this thread')
        return
      }
      existingSummary.remove()
    }

    this.isProcessing = true

    const timeoutId = setTimeout(() => {
      this.isProcessing = false
    }, this.processingTimeout)

    try {
      await this.processThread(threadView, threadId)
    } finally {
      clearTimeout(timeoutId)
      this.isProcessing = false
    }
  }

  async processThread(threadView: ThreadView, threadId: string): Promise<void> {
    console.log('metldr: processing thread:', threadId)

    const requestToken = `${threadId}-${Date.now()}`
    this.activeRequestToken = requestToken

    const subject = threadView.getSubject()
    const messages = threadView.getMessageViewsAll()

    if (!messages || messages.length === 0) {
      console.log('metldr: no messages in thread')
      return
    }

    let fullText = ''
    const participants = new Set<string>()
    let latestDate: string | null = null
    let fromName: string | null = null
    let fromEmail: string | null = null
    let replyTo: string | null = null
    let mailedBy: string | null = null
    let signedBy: string | null = null
    const toRecipients = new Set<string>()
    const ccRecipients = new Set<string>()
    const bccRecipients = new Set<string>()

    const formatContact = (contact: Contact | null): string | null => {
      if (!contact) return null
      const name = contact.name || contact.fullName || contact.displayName
      const email = contact.emailAddress || contact.address || contact.email
      if (name && email) return `${name} <${email}>`
      return email || name || null
    }

    const addContacts = (list: Contact[] | undefined, target: Set<string>): void => {
      if (!Array.isArray(list)) return
      list.forEach((c) => {
        const formatted = formatContact(c)
        if (formatted) target.add(formatted)
      })
    }

    for (const msgView of messages) {
      try {
        const sender = msgView.getSender()
        const bodyElement = msgView.getBodyElement()
        const body = bodyElement ? bodyElement.innerText : ''

        if (sender) {
          if (!fromName && sender.name) fromName = sender.name
          if (!fromEmail && sender.emailAddress) fromEmail = sender.emailAddress

          const senderStr = sender.name ? 
            `${sender.name} <${sender.emailAddress}>` : 
            sender.emailAddress || ''
          if (senderStr) participants.add(senderStr)
        }

        if (!replyTo && typeof msgView.getReplyTo === 'function') {
          try {
            const rt = msgView.getReplyTo()
            if (rt) {
              if (Array.isArray(rt)) {
                const formatted = formatContact(rt[0])
                if (formatted) replyTo = formatted
              } else {
                const formatted = formatContact(rt)
                if (formatted) replyTo = formatted
              }
            }
          } catch { /* ignore */ }
        }

        if ((!mailedBy || !signedBy) && typeof msgView.getSMTPHeaders === 'function') {
          try {
            const headers = msgView.getSMTPHeaders()
            if (headers?.['mailed-by'] && !mailedBy) mailedBy = headers['mailed-by']
            if (headers?.['signed-by'] && !signedBy) signedBy = headers['signed-by']
          } catch { /* ignore */ }
        }

        try {
          const recipientContacts = await msgView.getRecipientsFull()
          addContacts(recipientContacts, toRecipients)
        } catch {
          // fallback to sync email only method
          const emailAddrs = msgView.getRecipientEmailAddresses()
          emailAddrs.forEach(email => toRecipients.add(email))
        }

        if (body && body.length > 20) {
          const senderName = sender?.name || sender?.emailAddress || 'Unknown'
          fullText += `From: ${senderName}\n${body}\n\n`
        }

        const dateStr = msgView.getDateString?.()
        if (dateStr) latestDate = dateStr
      } catch (err) {
        console.warn('metldr: error extracting message:', err)
      }
    }

    fullText = fullText.split('\n').filter(line => line.trim().length > 0).join('\n')

    if (!fullText || fullText.length < 50) {
      console.log('metldr: email text too short, skipping')
      return
    }

    const metadata: EmailMetadata = {
      subject,
      participants: Array.from(participants),
      emailCount: messages.length,
      date: latestDate,
      timestamp: new Date().toISOString(),
      sender: fromName || null,
      senderEmail: fromEmail || null,
      from: (fromName || fromEmail) ? [fromName, fromEmail ? `<${fromEmail}>` : null].filter(Boolean).join(' ') : null,
      replyTo: replyTo || null,
      to: toRecipients.size ? Array.from(toRecipients).join(', ') : null,
      cc: ccRecipients.size ? Array.from(ccRecipients).join(', ') : null,
      bcc: bccRecipients.size ? Array.from(bccRecipients).join(', ') : null,
      toList: Array.from(toRecipients),
      ccList: Array.from(ccRecipients),
      bccList: Array.from(bccRecipients),
      mailedBy: mailedBy || null,
      signedBy: signedBy || null
    }

    console.log('metldr: extracted metadata:', metadata)
    
    this.lastExtractedContent = { threadId, content: fullText, metadata }

    const threadElement = threadView.getElement?.()

    const cached = await this.getCachedSummary(threadId)
    if (cached) {
      console.log('metldr: using cached summary')
      const summaryCard = UIService.createSummaryCard(cached, threadId)
      const header = this.findInjectionPoint(threadElement || null)
      if (header) {
        UIService.injectSummary(header, summaryCard)
        this.attachRegenerateListener(summaryCard, threadId, fullText, metadata)
      }
      return
    }

    // show button for manual summarization instead of auto-triggering
    const header = this.findInjectionPoint(threadElement || null)
    if (!header) {
      console.log('metldr: no injection point found')
      return
    }

    const summarizeButton = UIService.createSummarizeButton(threadId, async () => {
      // remove button
      summarizeButton.remove()
      
      // show loading indicator
      const loadingDiv = UIService.createLoadingIndicator(threadElement || document.body)
      UIService.injectLoading(header, loadingDiv)
      
      console.log('metldr: requesting summary...')
      
      let summary = await this.tryChromeSummary(fullText, metadata)
      
      if (summary) {
        console.log('metldr: using chrome ai summary')
        await this.cacheSummary(threadId, summary)
      } else {
        console.log('metldr: chrome ai unavailable, trying background (ollama)...')
        summary = await this.getSummaryFromBackground(fullText, threadId, metadata)
      }
      
      console.log('metldr: summary response:', summary)
      
      // check if thread changed during summarization
      const currentThreadId = this.getThreadIdFromUrl()
      if (currentThreadId && currentThreadId !== threadId) {
        console.log('metldr: thread changed before injection, skipping summary insert')
        loadingDiv.remove()
        return
      }
      
      loadingDiv.remove()
      
      if (summary) {
        console.log('metldr: creating summary card')
        const summaryCard = UIService.createSummaryCard(summary, threadId)
        UIService.injectSummary(header, summaryCard)
        this.attachRegenerateListener(summaryCard, threadId, fullText, metadata)
      }
    })
    
    UIService.injectLoading(header, summarizeButton)
  }

  findInjectionPoint(threadElement: HTMLElement | null): Element | null {
    if (threadElement) {
      const header = threadElement.querySelector('.gH') || 
                     threadElement.querySelector('.gE') ||
                     threadElement.querySelector('[data-thread-perm-id]')
      if (header) return header
    }

    return document.querySelector('.gH') || 
           document.querySelector('.gE') ||
           document.querySelector('[data-thread-perm-id]') ||
           document.querySelector('div[role="main"]')
  }

  getThreadIdFromUrl(): string | null {
    const hash = window.location.hash
    const hashMatch = hash.match(/#[^/]+\/(?:[^/]+\/)?([A-Za-z0-9_-]{16,})$/)
    if (hashMatch) return hashMatch[1]

    const url = new URL(window.location.href)
    return url.searchParams.get('msgid') || url.searchParams.get('tid') || null
  }

  attachRegenerateListener(summaryCard: HTMLElement, threadId: string, emailText: string, metadata: EmailMetadata): void {
    const regenerateBtn = summaryCard.querySelector('.metldr-regenerate-btn')
    if (regenerateBtn) {
      regenerateBtn.addEventListener('click', async () => {
        await this.regenerateSummary(threadId, emailText, metadata)
      })
    }
  }

  async getSummaryFromBackground(emailText: string, emailId: string, metadata: EmailMetadata | null = null, forceRegenerate = false, allowRetry = true): Promise<unknown> {
    if (!chrome?.runtime?.sendMessage) return null

    const maxRetries = 3
    const delays = [0, 100, 200]

    for (let attempt = 0; attempt < maxRetries; attempt++) {
      if (delays[attempt] > 0) {
        await new Promise(r => setTimeout(r, delays[attempt]))
      }

      try {
        const response = await new Promise<SummaryResponse>((resolve, reject) => {
          chrome.runtime.sendMessage({
            type: 'SUMMARIZE_EMAIL',
            emailContent: emailText,
            emailId: emailId,
            metadata: metadata,
            forceRegenerate: forceRegenerate
          }, (resp) => {
            if (chrome.runtime.lastError) {
              reject(new Error(chrome.runtime.lastError.message))
            } else {
              resolve(resp as SummaryResponse)
            }
          })
        })

        if (response?.needsOllama && allowRetry) {
          const ready = await this.waitForOllamaReady()
          if (ready) {
            return await this.getSummaryFromBackground(emailText, emailId, metadata, forceRegenerate, false)
          }
        }

        return response?.summary || null
      } catch (err) {
        console.warn(`metldr: email summary attempt ${attempt + 1}/${maxRetries} failed:`, (err as Error).message)
        if (attempt === maxRetries - 1) {
          console.error('metldr: all email summary attempts failed')
          return null
        }
      }
    }

    return null
  }

  async waitForOllamaReady(maxAttempts = 12, delayMs = 1000): Promise<boolean> {
    for (let attempt = 0; attempt < maxAttempts; attempt++) {
      try {
        const response = await new Promise<HealthResponse>((resolve, reject) => {
          chrome.runtime.sendMessage({ type: 'CHECK_OLLAMA_HEALTH' }, (resp) => {
            if (chrome.runtime.lastError) {
              reject(new Error(chrome.runtime.lastError.message))
            } else {
              resolve(resp as HealthResponse)
            }
          })
        })

        if (response?.success && response.connected) {
          return true
        }
      } catch (err) {
        if (attempt === maxAttempts - 1) {
          console.warn('metldr: ollama still unavailable after wait:', (err as Error).message)
        }
      }
      await new Promise(r => setTimeout(r, delayMs))
    }
    return false
  }

  async regenerateSummary(threadId: string, emailText: string, metadata: EmailMetadata): Promise<void> {
    if (this.isRegenerating) return
    this.isRegenerating = true

    const requestToken = `${threadId}-regen-${Date.now()}`
    this.activeRequestToken = requestToken

    try {
      const existing = document.querySelector('.metldr-summary') as HTMLElement | null
      if (existing) {
        existing.style.transition = 'opacity 0.2s, transform 0.2s'
        existing.style.opacity = '0'
        existing.style.transform = 'scale(0.95)'
        await new Promise(resolve => setTimeout(resolve, 200))
        existing.remove()
      }

      const header = this.findInjectionPoint(null)
      if (!header) return

      const loadingDiv = UIService.createLoadingIndicator(document.body)
      UIService.injectLoading(header, loadingDiv)

      const summary = await this.getSummaryFromBackground(emailText, threadId, metadata, true)
      if (loadingDiv) loadingDiv.remove()

      if (this.activeRequestToken !== requestToken) {
        console.log('metldr: stale regen response skipped for thread', threadId)
        return
      }

      const currentThreadId = this.getThreadIdFromUrl()
      if (currentThreadId && currentThreadId !== threadId) {
        console.log('metldr: thread changed during regen, skipping insert')
        return
      }

      if (summary) {
        const summaryCard = UIService.createSummaryCard(summary, threadId)
        UIService.injectSummary(header, summaryCard)
        this.attachRegenerateListener(summaryCard, threadId, emailText, metadata)

        if (replyPanel?.refreshForThread) {
          replyPanel.refreshForThread(threadId)
        }
      }
    } finally {
      this.isRegenerating = false
    }
  }

  destroy(): void {
    this.processedThreads.clear()
    replyPanel.hide()
  }
}

export const emailExtractor = new EmailExtractor()
