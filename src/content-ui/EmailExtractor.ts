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

  // expand collapsed messages with success-based retry
  // keeps trying different methods until all messages are loaded
  private async expandAndWaitForMessages(
    threadView: ThreadView, 
    messages: MessageView[], 
    timeoutMs = 10000
  ): Promise<void> {
    const totalMessages = messages.length
    const getLoadedCount = () => messages.filter(m => m.isLoaded?.() !== false).length
    
    let loadedCount = getLoadedCount()
    if (loadedCount >= totalMessages) {
      console.log('metldr: all', totalMessages, 'messages already loaded')
      return
    }

    console.log('metldr: need to expand', totalMessages - loadedCount, 'of', totalMessages, 'messages')
    const methodsUsed: string[] = []

    // DOM DISCOVERY: log what selectors actually exist in Gmail's current DOM
    const domScan = {
      expandAllBtn: !!document.querySelector('[data-tooltip="Expand all"]'),
      kx: document.querySelectorAll('.kx').length,
      h7: document.querySelectorAll('.h7[role="listitem"]').length,
      h7Collapsed: document.querySelectorAll('.h7[role="listitem"][aria-expanded="false"]').length,
      gs: document.querySelectorAll('.gs').length,
      adnCls: document.querySelectorAll('.adn').length,
      collapsedAttr: document.querySelectorAll('[data-collapsed]').length,
      msgHeaders: document.querySelectorAll('[data-legacy-message-id]').length
    }
    console.log('metldr: DOM scan BEFORE expansion:', domScan)

    // set up InboxSDK load event listeners
    const loadPromises: Promise<void>[] = []
    const unloaded = messages.filter(m => m.isLoaded?.() === false)
    for (const msgView of unloaded) {
      if (msgView.on) {
        loadPromises.push(new Promise<void>((resolve) => {
          const timeout = setTimeout(() => resolve(), timeoutMs)
          msgView.on!('load', () => {
            clearTimeout(timeout)
            resolve()
          })
        }))
      }
    }

    // EXPANSION METHODS - try each and check success after
    const methods = [
      // method 1: InboxSDK getElement + click header
      async () => {
        let clicked = 0
        for (const msgView of unloaded) {
          const el = (msgView as { getElement?: () => HTMLElement }).getElement?.()
          if (el) {
            const header = el.querySelector('.gE, .gs, [data-legacy-message-id]') as HTMLElement || el
            console.log('metldr: [M1] clicking:', header.tagName, header.className?.slice(0, 30))
            header.click()
            clicked++
            await new Promise(r => setTimeout(r, 50))
          }
        }
        return clicked > 0 ? `InboxSDK(${clicked})` : null
      },

      // method 2: native "Expand all" button
      async () => {
        const btn = document.querySelector('[data-tooltip="Expand all"], [aria-label*="Expand"]') as HTMLElement
        if (btn && this.isElementVisible(btn)) {
          console.log('metldr: [M2] clicking Expand All button')
          btn.click()
          await new Promise(r => setTimeout(r, 150))
          return 'ExpandAllBtn'
        }
        return null
      },

      // method 3: .kx collapsed groups ("X more messages")
      async () => {
        const groups = Array.from(document.querySelectorAll('.kx, .kQ')) as HTMLElement[]
        let clicked = 0
        for (const g of groups) {
          if (this.isElementVisible(g)) {
            console.log('metldr: [M3] clicking collapsed group')
            g.click()
            clicked++
            await new Promise(r => setTimeout(r, 80))
          }
        }
        return clicked > 0 ? `CollapsedGroup(${clicked})` : null
      },

      // method 4: .h7 collapsed message rows
      async () => {
        const rows = Array.from(document.querySelectorAll('.h7[role="listitem"][aria-expanded="false"]')) as HTMLElement[]
        let clicked = 0
        for (const row of rows) {
          if (this.isElementVisible(row)) {
            console.log('metldr: [M4] clicking collapsed row')
            row.click()
            clicked++
            await new Promise(r => setTimeout(r, 50))
          }
        }
        return clicked > 0 ? `H7Rows(${clicked})` : null
      },

      // method 5: click any element with data-legacy-message-id
      async () => {
        const msgElements = Array.from(document.querySelectorAll('[data-legacy-message-id]')) as HTMLElement[]
        let clicked = 0
        // only click ones that look collapsed (small height)
        for (const el of msgElements) {
          if (el.clientHeight < 100 && this.isElementVisible(el)) {
            console.log('metldr: [M5] clicking message-id element')
            el.click()
            clicked++
            await new Promise(r => setTimeout(r, 50))
          }
        }
        return clicked > 0 ? `MsgIdEl(${clicked})` : null
      },

      // method 6: .adn message containers
      async () => {
        const adns = Array.from(document.querySelectorAll('.adn.ads')) as HTMLElement[]
        let clicked = 0
        for (const adn of adns) {
          // check if collapsed by looking for body content
          const hasBody = adn.querySelector('.a3s')
          if (!hasBody && this.isElementVisible(adn)) {
            console.log('metldr: [M6] clicking .adn container')
            adn.click()
            clicked++
            await new Promise(r => setTimeout(r, 50))
          }
        }
        return clicked > 0 ? `AdnContainers(${clicked})` : null
      }
    ]

    const MAX_PASSES = 5
    let previousLoaded = 0
    let pass = 0

    while (pass < MAX_PASSES) {
      pass++
      loadedCount = getLoadedCount()
      
      console.log('metldr: === EXPANSION PASS', pass, '===', loadedCount, '/', totalMessages, 'loaded')
      
      if (loadedCount >= totalMessages) {
        console.log('metldr: all messages loaded!')
        break
      }
      
      if (loadedCount === previousLoaded && pass > 1) {
        console.log('metldr: no new messages loaded since last pass, stopping')
        break
      }
      previousLoaded = loadedCount

      // run all methods each pass - they re-scan DOM fresh each time
      for (let i = 0; i < methods.length; i++) {
        try {
          const result = await methods[i]()
          if (result) {
            methodsUsed.push(`P${pass}:${result}`)
            await new Promise(r => setTimeout(r, 100)) // wait for DOM update
          }
        } catch (e) {
          console.warn('metldr: method', i + 1, 'failed:', e)
        }
      }
      
      await new Promise(r => setTimeout(r, 150))
    }

    // wait for remaining load promises
    if (loadPromises.length > 0) {
      console.log('metldr: waiting for', loadPromises.length, 'InboxSDK load events...')
      await Promise.race([
        Promise.all(loadPromises),
        new Promise(r => setTimeout(r, timeoutMs))
      ])
    }

    // final status
    const finalLoaded = getLoadedCount()
    console.log('metldr: ===== EXPANSION COMPLETE =====')
    console.log('metldr: result:', finalLoaded, '/', totalMessages, 'messages loaded')
    console.log('metldr: methods used:', methodsUsed.join(' → ') || 'none')
    if (finalLoaded < totalMessages) {
      console.warn('metldr: warning:', totalMessages - finalLoaded, 'messages still not loaded')
    }

    // immediately scroll to the last message after expansion (before extraction)
    this.scrollToLatestMessage(threadView)
  }

  // check if element is visible in viewport
  private isElementVisible(element: HTMLElement): boolean {
    if (!element) return false
    const style = window.getComputedStyle(element)
    return (
      style.display !== 'none' &&
      style.visibility !== 'hidden' &&
      style.opacity !== '0' &&
      element.offsetWidth > 0 &&
      element.offsetHeight > 0
    )
  }

  private scrollToLatestMessage(threadView: ThreadView): void {
    const threadElement = (threadView as { getElement?: () => HTMLElement }).getElement?.()
    const container = threadElement || document.querySelector('div[role="main"]')
    if (!container) return

    const allMessages = container.querySelectorAll('[data-message-id], .gs, .adn')
    if (allMessages.length === 0) return
    
    const lastMessage = allMessages[allMessages.length - 1] as HTMLElement
    const header = lastMessage.querySelector('.gE, .gH, .iw') as HTMLElement || lastMessage
    header.scrollIntoView({ behavior: 'smooth', block: 'start' })
    console.log('metldr: scrolled to last message header')
  }

  private async extractThreadContent(threadView: ThreadView, threadId: string): Promise<void> {
    const subject = threadView.getSubject()
    const messages = threadView.getMessageViewsAll()
    if (!messages || messages.length === 0) return

    await this.expandAndWaitForMessages(threadView, messages)

    let fullText = ''
    const participants = new Set<string>()
    let latestDate: string | null = null
    let fromName: string | null = null
    let fromEmail: string | null = null
    const toRecipients = new Set<string>()
    let loadedCount = 0

    for (const msgView of messages) {
      try {
        const isLoaded = msgView.isLoaded?.() !== false
        if (!isLoaded) continue // skip unloaded messages - their data throws SelectorError

        loadedCount++

        // extract sender metadata
        const sender = msgView.getSender()
        if (sender) {
          if (!fromName && sender.name) fromName = sender.name
          if (!fromEmail && sender.emailAddress) fromEmail = sender.emailAddress
          const senderStr = sender.name ? `${sender.name} <${sender.emailAddress}>` : sender.emailAddress || ''
          if (senderStr) participants.add(senderStr)
        }

        // extract date
        const dateStr = msgView.getDateString?.()
        if (dateStr) latestDate = dateStr

        // extract recipients
        try {
          const recipientContacts = await msgView.getRecipientsFull()
          if (Array.isArray(recipientContacts)) {
            recipientContacts.forEach((c) => {
              const email = c.emailAddress || c.address || c.email
              if (email) toRecipients.add(email)
            })
          }
        } catch {
          try {
            const emailAddrs = msgView.getRecipientEmailAddresses()
            emailAddrs.forEach(email => toRecipients.add(email))
          } catch { /* ignore */ }
        }

        // extract body
        const bodyElement = msgView.getBodyElement()
        const body = bodyElement ? bodyElement.innerText : ''
        if (body && body.length > 20) {
          const senderName = sender?.name || sender?.emailAddress || 'Unknown'
          const msgDate = dateStr || ''
          fullText += `--- Message from ${senderName}${msgDate ? ` (${msgDate})` : ''} ---\n${body}\n\n`
        }
      } catch { /* ignore individual message errors */ }
    }

    fullText = fullText.split('\n').filter(line => line.trim().length > 0).join('\n')
    if (!fullText || fullText.length < 50) return

    const metadata: EmailMetadata = {
      subject,
      participants: Array.from(participants),
      emailCount: messages.length,
      date: latestDate,
      timestamp: new Date().toISOString(),
      sender: fromName || null,
      senderEmail: fromEmail || null,
      from: (fromName || fromEmail) ? [fromName, fromEmail ? `<${fromEmail}>` : null].filter(Boolean).join(' ') : null,
      replyTo: null,
      to: toRecipients.size ? Array.from(toRecipients).join(', ') : null,
      cc: null,
      bcc: null,
      toList: Array.from(toRecipients),
      ccList: [],
      bccList: [],
      mailedBy: null,
      signedBy: null
    }

    this.lastExtractedContent = { threadId, content: fullText, metadata }
    console.log('metldr: extracted email content for chat', { 
      threadId: threadId.slice(0, 20), 
      len: fullText.length,
      totalMessages: messages.length,
      loadedMessages: loadedCount,
      participants: participants.size
    })
  }

  // cache format: { summary, emailCount }
  async getCachedSummary(emailId: string, currentEmailCount?: number): Promise<{ summary: unknown; needsRegeneration: boolean } | null> {
    if (!chrome?.runtime?.sendMessage) return null
    try {
      const result = await new Promise<{ cached: unknown; emailCount?: number } | null>((resolve) => {
        chrome.runtime.sendMessage({ type: 'GET_EMAIL_CACHE', emailId }, (resp) => {
          if (chrome.runtime.lastError) {
            resolve(null)
          } else {
            resolve(resp || null)
          }
        })
      })
      
      if (!result?.cached) return null
      
      const cachedEmailCount = (result as any).emailCount || 0
      const needsRegeneration = currentEmailCount !== undefined && 
                                cachedEmailCount > 0 && 
                                currentEmailCount !== cachedEmailCount
      
      if (needsRegeneration) {
        console.log(`metldr: cache stale - email count changed ${cachedEmailCount} -> ${currentEmailCount}`)
      }
      
      return { summary: result.cached, needsRegeneration }
    } catch {
      return null
    }
  }

  async cacheSummary(emailId: string, summary: unknown, emailCount?: number): Promise<void> {
    if (!chrome?.runtime?.sendMessage) return
    try {
      await new Promise<void>((resolve) => {
        chrome.runtime.sendMessage({ type: 'SET_EMAIL_CACHE', emailId, summary, emailCount }, () => {
          resolve()
        })
      })
    } catch {
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
        // still extract content for chat context
        await this.extractThreadContent(threadView, threadId)
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

    // auto-expand collapsed messages and wait for them to load
    await this.expandAndWaitForMessages(threadView, messages)

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

    let loadedCount = 0

    for (const msgView of messages) {
      try {
        const isLoaded = msgView.isLoaded?.() !== false
        if (!isLoaded) continue // skip unloaded messages - their data throws SelectorError

        loadedCount++

        const sender = msgView.getSender()
        if (sender) {
          if (!fromName && sender.name) fromName = sender.name
          if (!fromEmail && sender.emailAddress) fromEmail = sender.emailAddress

          const senderStr = sender.name ? 
            `${sender.name} <${sender.emailAddress}>` : 
            sender.emailAddress || ''
          if (senderStr) participants.add(senderStr)
        }

        const dateStr = msgView.getDateString?.()
        if (dateStr) latestDate = dateStr

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
          try {
            const emailAddrs = msgView.getRecipientEmailAddresses()
            emailAddrs.forEach(email => toRecipients.add(email))
          } catch { /* ignore */ }
        }

        const bodyElement = msgView.getBodyElement()
        const body = bodyElement ? bodyElement.innerText : ''
        if (body && body.length > 20) {
          const senderName = sender?.name || sender?.emailAddress || 'Unknown'
          const msgDate = dateStr || ''
          fullText += `--- Message from ${senderName}${msgDate ? ` (${msgDate})` : ''} ---\n${body}\n\n`
        }
      } catch { /* ignore individual message errors */ }
    }

    console.log('metldr: thread extraction complete', { 
      total: messages.length, 
      loaded: loadedCount, 
      participants: participants.size 
    })

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
    const currentEmailCount = metadata.emailCount

    const cacheResult = await this.getCachedSummary(threadId, currentEmailCount)
    const cached = cacheResult?.summary
    const needsRegeneration = cacheResult?.needsRegeneration || false
    
    if (cached) {
      if (needsRegeneration) {
        // new emails arrived - auto-regenerate since we had a previous summary
        console.log('metldr: new emails detected, auto-regenerating summary')
        await this.autoRegenerateSummary(threadView, threadId, fullText, metadata, threadElement || null)
        return
      }
      
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
      
      // always use background service for consistent quality with fact extraction
      const summary = await this.getSummaryFromBackground(fullText, threadId, metadata)
      
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
    const container = threadElement || document.querySelector('div[role="main"]')
    if (!container) return null

    // find all message elements in the thread and get the LAST one (most recent)
    const allMessages = container.querySelectorAll('[data-message-id], .gs, .adn')
    if (allMessages.length > 0) {
      const lastMessage = allMessages[allMessages.length - 1]
      // inject after the last message's header area
      const header = lastMessage.querySelector('.gE, .gH, .iw') || lastMessage
      return header
    }

    // fallback to thread header if no messages found
    if (threadElement) {
      const header = threadElement.querySelector('.gH') || 
                     threadElement.querySelector('.gE') ||
                     threadElement.querySelector('[data-thread-perm-id]')
      if (header) return header
    }

    return document.querySelector('.gH') || 
           document.querySelector('.gE') ||
           document.querySelector('[data-thread-perm-id]') ||
           container
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

  async autoRegenerateSummary(
    _threadView: ThreadView, 
    threadId: string, 
    emailText: string, 
    metadata: EmailMetadata, 
    threadElement: HTMLElement | null
  ): Promise<void> {
    const header = this.findInjectionPoint(threadElement)
    if (!header) return

    // show loading
    const loadingDiv = UIService.createLoadingIndicator(threadElement || document.body)
    UIService.injectLoading(header, loadingDiv)
    
    console.log('metldr: auto-regenerating summary due to new emails...')
    
    const summary = await this.getSummaryFromBackground(emailText, threadId, metadata, true)
    
    loadingDiv.remove()
    
    if (summary) {
      const summaryCard = UIService.createSummaryCard(summary, threadId)
      UIService.injectSummary(header, summaryCard)
      this.attachRegenerateListener(summaryCard, threadId, emailText, metadata)
    }
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
