import { OllamaService } from './OllamaService'
import { cacheService } from './CacheService'
import { dictionaryService } from './DictionaryService'
import { WordService } from './WordService'
import { EmailService } from './EmailService'
import { PageService } from './PageService'
import { pdfService } from './pdf/PdfService'
import { SummaryPrefs } from '../utils/summaryPrefs'
import { aiGateway } from './ai'
import { logger } from './LoggerService'
import { ragService } from './rag/RagService'
import { concurrencyManager } from './ConcurrencyManager'
import { analyticsService } from './AnalyticsService'
import type {
  EmailSummaryMessage,
  GetReplySuggestionsMessage,
  GenerateReplySuggestionsMessage,
  ExtractAndSummarizeMessage,
  WordLookupMessage,
  ChatMessageRequest,
  BackgroundMessage,
  ResponseCallback,
  SummaryPrefsConfig,
  ClassifiedPage,
  ClassifyResult,
  SummaryResult,
  ChatMessage,
  ExtractedData,
  GetEmailCacheMessage,
  SetEmailCacheMessage
} from '../types'

const log = logger.createScoped('BackgroundBootstrap')

export class BackgroundBootstrap {
  static isInitialized = false
  static initPromise: Promise<void> | null = null
  static summaryQueue: Promise<unknown> = Promise.resolve()
  
  static async init(): Promise<void> {
    if (this.initPromise) return this.initPromise
    
    this.initPromise = this._doInit()
    return this.initPromise
  }
  
  static async _doInit(): Promise<void> {
    log.log('starting initialization...')
    
    chrome.sidePanel
      .setPanelBehavior({ openPanelOnActionClick: true })
      .catch(err => log.error('side panel error', err))

    log.log('cache service ready (lazy init)')
    
    try {
      await dictionaryService.init()
      log.log('dictionary service ready')
    } catch (err) {
      log.error('dictionary init failed', (err as Error).message)
    }
    
    // preload embedding model on startup via offscreen document
    // reduces first-indexing latency from ~150s to ~5s
    this._preloadEmbeddingModel()
    
    this.isInitialized = true

    // Listen for provider preference changes
    chrome.storage.onChanged.addListener((changes, area) => {
      if (area === 'local' && changes.preferredProvider?.newValue) {
        const pref = changes.preferredProvider.newValue as string
        if (pref === 'chrome-ai' || pref === 'ollama') {
          aiGateway.setPreference(pref)
        }
      }
    })

    chrome.runtime.onMessage.addListener((msg: BackgroundMessage, _sender, respond: ResponseCallback) => {
      if (msg.type === 'SUMMARIZE_EMAIL') {
        this._onEmailSummary(msg, respond)
        return true
      }

      if (msg.type === 'GET_REPLY_SUGGESTIONS') {
        this._onGetReplySuggestions(msg, respond)
        return true
      }

      if (msg.type === 'GENERATE_REPLY_SUGGESTIONS') {
        this._onGenerateReplySuggestions(msg as GenerateReplySuggestionsMessage, respond)
        return true
      }

      if (msg.type === 'EXTRACT_ONLY') {
        this._onExtractOnly(msg, respond)
        return true
      }

      if (msg.type === 'EXTRACT_AND_SUMMARIZE') {
        this._onExtractAndSummarize(msg, respond)
        return true
      }

      if (msg.type === 'WORD_LOOKUP') {
        this._onWordLookup(msg, respond)
        return true
      }

      if (msg.type === 'CHECK_OLLAMA_HEALTH') {
        this._onHealthCheck(respond)
        return true
      }

      if (msg.type === 'CHAT_MESSAGE') {
        this._onChatMessage(msg as ChatMessageRequest, respond)
        return true
      }

      if (msg.type === 'GLOBAL_CHAT') {
        this._onGlobalChat(msg as { type: string; messages: import('../types').ChatMessage[] }, respond)
        return true
      }

      if (msg.type === 'GET_EMAIL_CACHE') {
        this._onGetEmailCache(msg as GetEmailCacheMessage, respond)
        return true
      }

      if (msg.type === 'SET_EMAIL_CACHE') {
        this._onSetEmailCache(msg as SetEmailCacheMessage, respond)
        return true
      }

      if (msg.type === 'RAG_INDEX') {
        this._onRagIndex(msg as { type: string; entry: import('../types').VectorEntry }, respond)
        return true
      }

      if (msg.type === 'RAG_INDEX_CHUNKS') {
        this._onRagIndexChunks(msg as { type: string; text: string; metadata: any }, respond)
        return true
      }

      if (msg.type === 'RAG_SEARCH') {
        this._onRagSearch(msg as { type: string; query: string; limit?: number }, respond)
        return true
      }

      if (msg.type === 'RAG_HAS_INDEXED_CONTENT') {
        this._onRagHasIndexedContent(msg as { type: string; sourceUrl: string }, respond)
        return true
      }

      if (msg.type === 'RAG_SEARCH_WITH_CONTEXT') {
        this._onRagSearchWithContext(msg as { type: string; query: string; limit?: number; sourceUrl?: string }, respond)
        return true
      }

      if (msg.type === 'RAG_IS_INDEXING') {
        const { sourceId } = msg as { type: string; sourceId: string }
        respond({ success: true, isIndexing: ragService.isIndexing(sourceId) })
        return true
      }

      if (msg.type === 'RAG_ENSURE_INDEXED') {
        this._onRagEnsureIndexed(msg as { type: string; text: string; metadata: any }, respond)
        return true
      }

      if (msg.type === 'RAG_INDEXING_STATUS') {
        this._onRagIndexingStatus(msg as { type: string; sourceId: string }, respond)
        return true
      }

      if (msg.type === 'RAG_HAS_INDEXED_CONTENT') {
        const { sourceUrl } = msg as { type: string; sourceUrl: string }
        ragService.hasIndexedContent(sourceUrl)
          .then(hasContent => respond({ success: true, hasContent }))
          .catch(err => respond({ success: false, error: (err as Error).message }))
        return true
      }

      // pdf toolbar handlers
      if (msg.type === 'PDF_SUMMARIZE') {
        this._onPdfSummarize(msg as { type: string; url: string }, respond)
        return true
      }

      if (msg.type === 'PDF_EXTRACT_TEXT') {
        this._onPdfExtractText(msg as { type: string; url: string }, respond)
        return true
      }

      if (msg.type === 'OPEN_SIDE_PANEL') {
        this._onOpenSidePanel(msg as { type: string; focus?: string }, _sender, respond)
        return true
      }

      if (msg.type === 'TOGGLE_SIDE_PANEL') {
        this._onToggleSidePanel(msg as { type: string; focus?: string }, _sender, respond)
        return true
      }

      // handler for processing PDFs from ArrayBuffer (local file picker)
      if (msg.type === 'PDF_PROCESS_ARRAYBUFFER') {
        this._onPdfProcessArrayBuffer(msg as { type: string; data: number[]; filename: string; action: 'summarize' | 'copy' }, respond)
        return true
      }

      // handler for checking page cache (used by PdfToolbar)
      if (msg.type === 'GET_PAGE_CACHE') {
        this._onGetPageCache(msg as { type: string; url: string }, respond)
        return true
      }

      return false
    })

    log.log('initialized')
  }

  static _onEmailSummary(msg: EmailSummaryMessage, respond: ResponseCallback): void {
    (async () => {
      try {
        const { emailContent, emailId, metadata, forceRegenerate } = msg
        const summary = await EmailService.summarize(
          emailContent, 
          emailId || null, 
          metadata as Record<string, unknown> | null, 
          forceRegenerate || false
        )
        respond({ summary })
      } catch (err) {
        log.error('onEmailSummary', (err as Error).message)
        const errMsg = (err as Error)?.message || 'unknown error'
        const needsOllama = /ollama/i.test(errMsg)
        respond({
          error: errMsg,
          needsOllama,
          summary: {
            summary: `error: ${errMsg}`,
            action_items: [],
            dates: [],
            confidence: 'low'
          }
        })
      }
    })()
  }

  static _onGetReplySuggestions(msg: GetReplySuggestionsMessage, respond: ResponseCallback): void {
    (async () => {
      try {
        const { emailId } = msg
        if (!emailId) {
          respond({ success: false, error: 'no emailId' })
          return
        }

        const suggestions = await EmailService.getCachedReplies(emailId)
        if (suggestions && suggestions.length > 0) {
          respond({ success: true, suggestions })
        } else {
          respond({ success: false, error: 'no suggestions available' })
        }
      } catch (err) {
        log.error('onGetReplySuggestions', (err as Error).message)
        respond({ success: false, error: (err as Error).message })
      }
    })()
  }

  static _onGenerateReplySuggestions(msg: GenerateReplySuggestionsMessage, respond: ResponseCallback): void {
    (async () => {
      try {
        const { emailId, forceRegenerate } = msg
        if (!emailId) {
          respond({ success: false, error: 'no emailId' })
          return
        }

        log.log('generating reply suggestions', { emailId, force: forceRegenerate })

        // delete cached suggestions if force regenerating
        if (forceRegenerate) {
          try {
            await cacheService.deleteReplySuggestions(emailId)
          } catch (err) {
            log.warn('failed to delete cached suggestions', (err as Error).message)
          }
        }

        // check if we already have suggestions
        if (!forceRegenerate) {
          const cached = await EmailService.getCachedReplies(emailId)
          if (cached && cached.length > 0) {
            respond({ success: true, suggestions: cached })
            return
          }
        }

        // get cached summary to build context
        const cachedSummary = await cacheService.getEmailSummary(emailId)
        if (!cachedSummary) {
          log.log('no cached summary for thread, cannot generate suggestions')
          respond({ success: false, error: 'no email summary available' })
          return
        }

        // request email content from current tab
        const [tab] = await chrome.tabs.query({ active: true, currentWindow: true })
        if (!tab?.id) {
          respond({ success: false, error: 'no active tab' })
          return
        }

        // get email content from content script
        let emailContent = ''
        let metadata = null
        try {
          const extractResponse = await chrome.tabs.sendMessage(tab.id, { 
            type: 'GET_EMAIL_CONTENT',
            emailId 
          }) as { success: boolean; content?: string; metadata?: Record<string, unknown> }
          
          if (extractResponse?.success && extractResponse.content) {
            emailContent = extractResponse.content
            metadata = extractResponse.metadata || null
          }
        } catch (err) {
          log.log('could not extract email content', (err as Error).message)
        }

        // if we couldn't get email content, try to generate with just the summary
        if (!emailContent) {
          log.log('no email content, generating with summary context only')
          // create a minimal email content from summary
          const summary = cachedSummary as { summary?: string; action_items?: string[] }
          emailContent = `Summary: ${summary.summary || ''}\nAction items: ${(summary.action_items || []).join(', ')}`
        }

        const suggestions = await EmailService.generateReplySuggestions(
          emailId, 
          emailContent, 
          cachedSummary as Parameters<typeof EmailService.generateReplySuggestions>[2],
          metadata
        )

        if (suggestions && suggestions.length > 0) {
          respond({ success: true, suggestions })
        } else {
          respond({ success: false, error: 'failed to generate suggestions' })
        }
      } catch (err) {
        log.error('onGenerateReplySuggestions', (err as Error).message)
        respond({ success: false, error: (err as Error).message })
      }
    })()
  }

  static _onExtractOnly(msg: { tabId: number }, respond: ResponseCallback): void {
    (async () => {
      try {
        const { tabId } = msg
        if (!tabId) {
          respond({ success: false, error: 'no tabId' })
          return
        }
        const response = await chrome.tabs.sendMessage(tabId, { type: 'EXTRACT_ARTICLE' }) as { success: boolean; data?: ExtractedData; error?: string }
        if (!response?.success || !response.data) {
          respond({ success: false, error: response?.error || 'extraction failed' })
          return
        }
        respond({ success: true, data: response.data })
      } catch (err) {
        respond({ success: false, error: (err as Error).message })
      }
    })()
  }

  static _onExtractAndSummarize(msg: ExtractAndSummarizeMessage, respond: ResponseCallback): void {
    const { tabId, force, trigger } = msg
    this._enqueueSummary(() => this._performExtractAndSummarize(tabId, force || false, trigger || 'auto'))
      .then(result => respond(result))
  }

  static _onWordLookup(msg: WordLookupMessage, respond: ResponseCallback): void {
    (async () => {
      try {
        const { word, context } = msg
        
        if (!word || typeof word !== 'string') {
          respond({ success: false, error: 'invalid word' })
          return
        }
        
        if (!this.isInitialized) {
          console.log('[BackgroundBootstrap._onWordLookup] waiting for init...')
          await this.init()
        }

        const settings = await chrome.storage.local.get(['selectedLanguages', 'dictionarySource'])
        const languages = this._normalizeLanguages(settings.selectedLanguages)
        
        console.log('[BackgroundBootstrap._onWordLookup] looking up:', word, 'langs:', languages)

        const isEnglish = /^[a-zA-Z]+$/.test(word)
        let detectedLang = 'en'

        if (!isEnglish) {
          try {
            detectedLang = await WordService.detectLanguage(word, context?.fullSentence || '')
          } catch {
            detectedLang = 'en'
          }
        }

        const priorityLangs = [detectedLang]
        for (const lang of languages) {
          if (lang !== detectedLang) priorityLangs.push(lang)
        }
        if (detectedLang !== 'en' && !languages.includes('en')) {
          priorityLangs.push('en')
        }

        const result = await WordService.lookup(word, { ...context, languages: priorityLangs })

        if (result) {
          console.log('[BackgroundBootstrap._onWordLookup] found result, source:', result.source)
          // track analytics for real-time stat sync
          const cached = result.source === 'local' || result.source === 'cache'
          analyticsService.trackWordLookup(word, cached).catch(() => {})
          respond({ success: true, result })
        } else {
          console.log('[BackgroundBootstrap._onWordLookup] no result found')
          respond({ success: false, error: 'word not found' })
        }
      } catch (err) {
        log.error('onWordLookup', (err as Error).message)
        respond({ success: false, error: (err as Error).message || 'lookup failed' })
      }
    })()
  }

  static _onHealthCheck(respond: ResponseCallback): void {
    (async () => {
      try {
        const { available, models } = await OllamaService.checkAvailable()
        respond({ success: true, connected: available, models })
      } catch (err) {
        log.error('onHealthCheck', (err as Error).message)
        respond({ success: true, connected: false, models: [] })
      }
    })()
  }

  static _onChatMessage(msg: ChatMessageRequest, respond: ResponseCallback): void {
    (async () => {
      try {
        const { model, messages, pageContext } = msg

        if (!messages?.length) {
          respond({ ok: false, error: 'no messages provided' })
          return
        }

        console.log('[BackgroundBootstrap._onChatMessage] processing', messages.length, 'messages, model:', model || 'auto')
        
        const result = await PageService.chat(messages, pageContext || null, model || null)
        
        if (!result) {
          log.error('onChatMessage no result from PageService')
          respond({ ok: false, error: 'no response from chat service' })
          return
        }
        
        respond({
          ...result,
          timing: result.timing || null
        })
        
      } catch (err) {
        log.error('onChatMessage error:', (err as Error).message)
        respond({ ok: false, error: (err as Error).message || 'chat processing failed' })
      }
    })()
  }

  static _onGlobalChat(msg: { type: string; messages: import('../types').ChatMessage[] }, respond: ResponseCallback): void {
    (async () => {
      try {
        const { messages } = msg
        if (!messages?.length) {
          respond({ ok: false, error: 'no messages provided' })
          return
        }
        
        log.log(`onGlobalChat processing ${messages.length} messages`)
        const result = await PageService.globalChat(messages)
        
        log.log(`onGlobalChat result has ${result.sources?.length || 0} sources`)
        
        respond({
          ...result,
          timing: result.timing || null
        })
      } catch (err) {
        log.error('onGlobalChat error:', (err as Error).message)
        respond({ ok: false, error: (err as Error).message || 'global chat failed' })
      }
    })()
  }

  static _normalizeLanguages(langs: unknown): string[] {
    if (!langs) return ['en']

    if (!Array.isArray(langs)) {
      if (typeof langs === 'object' && langs !== null) {
        return Object.values(langs).filter((l): l is string => typeof l === 'string')
      }
      return [String(langs)]
    }

    return langs.length > 0 ? (langs as string[]) : ['en']
  }

  static _enqueueSummary<T>(task: () => Promise<T>): Promise<T | { success: false; error: string }> {
    this.summaryQueue = this.summaryQueue.then(() => task()).catch(err => {
      log.error('summary queue error', (err as Error).message)
      return { success: false, error: (err as Error).message }
    })
    return this.summaryQueue as Promise<T | { success: false; error: string }>
  }

  static async _performExtractAndSummarize(tabId: number, force: boolean, trigger = 'auto'): Promise<SummaryResult> {
    try {
      log.log('_performExtractAndSummarize:', { tabId, force, trigger })
      const prefs = await this._getSummaryPrefs()
      let extracted: ExtractedData | undefined
      try {
        const response = await chrome.tabs.sendMessage(tabId, { type: 'EXTRACT_ARTICLE' }) as { success: boolean; data?: ExtractedData; error?: string }
        log.log('EXTRACT_ARTICLE response:', { success: response?.success, hasData: !!response?.data, error: response?.error })
        if (!response?.success) {
          return { success: false, error: response?.error || 'extraction failed' }
        }
        extracted = response.data
      } catch (err) {
        log.error('content script error:', (err as Error).message)
        return { success: false, error: 'content script not ready - please refresh the page' }
      }

      if (!extracted) return { success: false, error: 'extraction failed' }
      if (extracted.skip) return { success: false, skip: true, reason: extracted.reason }
      const gating = this._classifyPage(extracted, prefs, trigger, force)
      log.log('classification:', { action: gating.action, reason: gating.reason, wordCount: extracted?.wordCount })
      if (gating.action === 'skip') return { success: false, skip: true, reason: gating.reason }
      if (gating.action === 'prompt') return { success: false, prompt: true, reason: gating.reason }

      const summary = await PageService.summarize(extracted as Parameters<typeof PageService.summarize>[0], force)
      return { success: true, summary }
    } catch (err) {
      log.error('performExtractAndSummarize]', (err as Error).message)
      return { success: false, error: (err as Error).message }
    }
  }

  static async _getSummaryPrefs(): Promise<SummaryPrefsConfig> {
    try {
      const stored = await chrome.storage.local.get(['summaryPrefs'])
      return SummaryPrefs.buildPrefs(stored?.summaryPrefs || {})
    } catch {
      return SummaryPrefs.DEFAULT_PREFS
    }
  }

  static _classifyPage(extracted: ExtractedData, prefs: SummaryPrefsConfig, trigger: string, force: boolean): ClassifyResult {
    const url = extracted?.url || ''
    const contentType = extracted?.contentType || 'text/html'
    const signals = extracted?.pageSignals || {}
    const wordCount = extracted?.wordCount || 0

    if (prefs.denylist.some(d => url.includes(d))) {
      return { action: 'skip', reason: 'denylist' }
    }

    if (!contentType.startsWith('text/')) {
      return { action: 'skip', reason: 'non_text' }
    }

    if (trigger === 'manual' || force) {
      if (wordCount < 80) return { action: 'skip', reason: 'too_short' }
      return { action: 'auto', reason: 'manual_trigger' }
    }

    const nonReader =
      signals.isSPA ||
      signals.isDashboard ||
      signals.isSearch ||
      signals.isCart ||
      signals.isFeed ||
      signals.isSocialFeed ||
      signals.canvasHeavy ||
      (signals.buttonToParagraphRatio || 0) > 2 ||
      (signals.linkDensity || 0) > 0.55

    if (nonReader) {
      return { action: 'skip', reason: 'non_reader' }
    }

    if (prefs.mode === 'manual') {
      return { action: 'wait', reason: 'manual_mode' }
    }

    const allowHit = prefs.allowlist.some(d => url.includes(d))
    const strongArticle = (signals.hasArticleTag || (signals.h1Count || 0) > 0) && (signals.textDensity || 0) > 0.4 && wordCount >= prefs.minAutoWords
    const highConfidence = allowHit ? wordCount >= prefs.minPromptWords : strongArticle

    if (highConfidence) {
      return { action: 'auto', reason: allowHit ? 'allowlist' : 'article_confident' }
    }

    const mediumConfidence = (signals.hasMain || signals.hasArticleTag || (signals.h1Count || 0) > 0) &&
      wordCount >= prefs.minPromptWords &&
      (signals.textDensity || 0) > 0.25

    if (prefs.mode === 'smart' && mediumConfidence) {
      return { action: 'prompt', reason: 'medium_confidence' }
    }

    return { action: 'wait', reason: 'low_confidence' }
  }

  static _onGetEmailCache(msg: GetEmailCacheMessage, respond: ResponseCallback): void {
    (async () => {
      try {
        const session = await cacheService.getEmailSession(msg.emailId)
        respond({ 
          cached: session?.summary || null,
          emailCount: (session as any)?.emailCount || 0
        })
      } catch (err) {
        log.error('onGetEmailCache]', (err as Error).message)
        respond({ cached: null, emailCount: 0 })
      }
    })()
  }

  static _onSetEmailCache(msg: SetEmailCacheMessage & { emailCount?: number }, respond: ResponseCallback): void {
    (async () => {
      try {
        // store summary with emailCount for staleness detection
        await cacheService.updateEmailSession(msg.emailId, { 
          summary: msg.summary,
          ...(msg.emailCount !== undefined && { emailCount: msg.emailCount } as any)
        })
        respond({ success: true })
      } catch (err) {
        log.error('onSetEmailCache]', (err as Error).message)
        respond({ success: false })
      }
    })()
  }

  static _onRagIndex(msg: { type: string; entry: import('../types').VectorEntry }, respond: ResponseCallback): void {
    (async () => {
      try {
        await ragService.index(msg.entry)
        respond({ success: true })
      } catch (err) {
        log.error('onRagIndex', (err as Error).message)
        respond({ success: false, error: (err as Error).message })
      }
    })()
  }

  static _onRagIndexChunks(msg: { type: string; text: string; metadata: any }, respond: ResponseCallback): void {
    (async () => {
      try {
        const sourceUrl = msg.metadata?.sourceUrl || msg.metadata?.sourceId || 'unknown'
        const sourceId = msg.metadata?.sourceId || sourceUrl
        
        // broadcast progress to side panel
        const broadcastProgress = (percent: number) => {
          console.log('[BackgroundBootstrap] Broadcasting progress:', percent, 'for', sourceId.slice(0, 40))
          chrome.runtime.sendMessage({
            type: 'INDEXING_PROGRESS',
            sourceId,
            percent
          }).catch(() => {}) // ignore if side panel closed
        }
        
        // wrap in concurrency manager to prevent duplicate indexing on rapid tab switches
        await concurrencyManager.execute('indexing', sourceUrl, async (signal) => {
          if (signal.aborted) throw new Error('Indexing aborted')
          await ragService.indexChunks(msg.text, msg.metadata, broadcastProgress)
        })
        respond({ success: true })
      } catch (err) {
        const errMsg = (err as Error).message
        if (errMsg !== 'Indexing aborted' && errMsg !== 'Operation cancelled') {
          log.error('onRagIndexChunks', errMsg)
        }
        respond({ success: false, error: errMsg })
      }
    })()
  }

  static _onRagSearch(msg: { type: string; query: string; limit?: number }, respond: ResponseCallback): void {
    (async () => {
      try {
        const results = await ragService.search(msg.query, msg.limit || 10)
        respond({ success: true, results })
      } catch (err) {
        log.error('onRagSearch', (err as Error).message)
        respond({ success: false, error: (err as Error).message, results: [] })
      }
    })()
  }

  static _onRagHasIndexedContent(msg: { type: string; sourceUrl: string }, respond: ResponseCallback): void {
    (async () => {
      try {
        const hasContent = await ragService.hasIndexedContent(msg.sourceUrl)
        respond({ success: true, hasContent })
      } catch (err) {
        log.error('onRagHasIndexedContent', (err as Error).message)
        respond({ success: false, error: (err as Error).message, hasContent: false })
      }
    })()
  }

  static _onRagSearchWithContext(msg: { type: string; query: string; limit?: number; sourceUrl?: string }, respond: ResponseCallback): void {
    (async () => {
      try {
        const context = await ragService.searchWithContext(msg.query, msg.limit || 5, msg.sourceUrl)
        respond({ success: true, context })
      } catch (err) {
        log.error('onRagSearchWithContext', (err as Error).message)
        respond({ success: false, error: (err as Error).message, context: '' })
      }
    })()
  }

  static _onRagEnsureIndexed(msg: { type: string; text: string; metadata: any }, respond: ResponseCallback): void {
    (async () => {
      try {
        const sourceUrl = msg.metadata?.sourceUrl || msg.metadata?.sourceId || 'unknown'
        const sourceId = msg.metadata?.sourceId || sourceUrl
        
        // broadcast progress to side panel
        const broadcastProgress = (percent: number) => {
          console.log('[BackgroundBootstrap] Broadcasting progress:', percent, 'for', sourceId.slice(0, 40))
          chrome.runtime.sendMessage({
            type: 'INDEXING_PROGRESS',
            sourceId,
            percent
          }).catch(() => {}) // ignore if side panel closed
        }
        
        // wrap in concurrency manager to prevent duplicate indexing
        const result = await concurrencyManager.execute('indexing', sourceUrl, async (signal) => {
          if (signal.aborted) throw new Error('Indexing aborted')
          return await ragService.ensureIndexed(msg.text, msg.metadata, broadcastProgress)
        })
        
        respond({ success: true, result })
      } catch (err) {
        const errMsg = (err as Error).message
        if (errMsg !== 'Indexing aborted' && errMsg !== 'Operation cancelled') {
          log.error('onRagEnsureIndexed', errMsg)
        }
        respond({ success: false, error: errMsg })
      }
    })()
  }

  static _onRagIndexingStatus(msg: { type: string; sourceId: string }, respond: ResponseCallback): void {
    (async () => {
      try {
        const status = await ragService.getIndexingStatus(msg.sourceId)
        respond({ success: true, status })
      } catch (err) {
        log.error('onRagIndexingStatus', (err as Error).message)
        respond({ success: false, error: (err as Error).message, status: 'needed' })
      }
    })()
  }

  static _preloadEmbeddingModel(): void {
    // preload embedding model via LocalModelProvider (creates sandbox iframe automatically)
    aiGateway.initializeLocalModels(['embed'])
      .then(() => log.log('Embedding model preloaded'))
      .catch(err => log.warn('Model preload failed', err.message))
  }

  // pdf toolbar handlers
  static _onPdfSummarize(msg: { type: string; url: string }, respond: ResponseCallback): void {
    (async () => {
      try {
        const { url } = msg
        if (!url) {
          respond({ success: false, error: 'no url provided' })
          return
        }

        log.log('pdf summarize request:', url.slice(0, 80))

        // use same queue as email/page summarization
        const result = await this._enqueueSummary(() => pdfService.summarize(url))
        
        if (typeof result === 'object' && 'success' in result && !result.success) {
          respond({ success: false, error: result.error })
          return
        }
        
        const summary = result as string
        
        // parse bullets from summary text
        const bullets = summary
          .split('\n')
          .filter(line => line.trim().startsWith('•') || line.trim().startsWith('-') || line.trim().startsWith('*'))
          .map(line => line.replace(/^[•\-*]\s*/, '').trim())
          .filter(Boolean)

        // if no bullets found, split by sentences
        const finalBullets = bullets.length > 0 
          ? bullets 
          : summary.split(/[.!?]+/).filter(s => s.trim().length > 20).slice(0, 5)

        respond({ success: true, summary: { bullets: finalBullets, raw: summary } })
      } catch (err) {
        log.error('onPdfSummarize', (err as Error).message)
        respond({ success: false, error: (err as Error).message })
      }
    })()
  }

  static _onPdfExtractText(msg: { type: string; url: string }, respond: ResponseCallback): void {
    (async () => {
      try {
        const { url } = msg
        if (!url) {
          respond({ success: false, error: 'no url provided' })
          return
        }

        log.log('pdf extract text request:', url.slice(0, 80))

        const text = await pdfService.extractFromUrl(url)
        const wordCount = text.split(/\s+/).filter(Boolean).length

        respond({ success: true, text, wordCount })
      } catch (err) {
        log.error('onPdfExtractText', (err as Error).message)
        respond({ success: false, error: (err as Error).message })
      }
    })()
  }

  static _onOpenSidePanel(
    msg: { type: string; focus?: string },
    sender: chrome.runtime.MessageSender,
    respond: ResponseCallback
  ): void {

    const windowId = sender?.tab?.windowId
    
    if (!windowId) {
      // fallback: try to get current window
      chrome.windows.getCurrent((win) => {
        if (win?.id) {
          chrome.sidePanel.open({ windowId: win.id })
            .then(() => {
              log.log('side panel opened (fallback)', msg.focus ? `focus: ${msg.focus}` : '')
              respond({ success: true })
            })
            .catch((err) => {
              log.error('onOpenSidePanel', err.message)
              respond({ success: false, error: err.message })
            })
        } else {
          respond({ success: false, error: 'no window available' })
        }
      })
      return
    }
    
    // preferred path: use sender's windowId directly (preserves user gesture)
    chrome.sidePanel.open({ windowId })
      .then(() => {
        log.log('side panel opened', msg.focus ? `focus: ${msg.focus}` : '')
        respond({ success: true })
      })
      .catch((err) => {
        log.error('onOpenSidePanel', err.message)
        respond({ success: false, error: err.message })
      })
  }

  // toggle side panel - if panel is open, broadcast close message; otherwise open it
  static _onToggleSidePanel(
    msg: { type: string; focus?: string },
    sender: chrome.runtime.MessageSender,
    respond: ResponseCallback
  ): void {
    const windowId = sender?.tab?.windowId
    
    if (!windowId) {
      chrome.windows.getCurrent((win) => {
        if (win?.id) {
          // try to open - if it fails, panel might already be open
          chrome.sidePanel.open({ windowId: win.id })
            .then(() => {
              log.log('side panel opened (toggle)', msg.focus ? `focus: ${msg.focus}` : '')
              respond({ success: true, action: 'opened' })
            })
            .catch(() => {
              // panel likely already open, broadcast close message
              chrome.runtime.sendMessage({ type: 'TOGGLE_SIDE_PANEL' }).catch(() => {})
              respond({ success: true, action: 'closed' })
            })
        } else {
          respond({ success: false, error: 'no window available' })
        }
      })
      return
    }
    
    // try to open panel - if already open, this succeeds silently
    // the panel listens for TOGGLE_SIDE_PANEL and closes itself
    chrome.sidePanel.open({ windowId })
      .then(() => {
        // panel opened or was already open
        // broadcast toggle message - panel will close itself if open
        chrome.runtime.sendMessage({ type: 'TOGGLE_SIDE_PANEL' }).catch(() => {})
        log.log('side panel toggle', msg.focus ? `focus: ${msg.focus}` : '')
        respond({ success: true })
      })
      .catch((err) => {
        log.error('onToggleSidePanel', err.message)
        respond({ success: false, error: err.message })
      })
  }

  // process PDF from ArrayBuffer (content script file picker)
  // returns summary immediately, continues indexing in background
  static _onPdfProcessArrayBuffer(
    msg: { type: string; data: number[]; filename: string; action: 'summarize' | 'copy'; sourceUrl?: string },
    respond: ResponseCallback
  ): void {
    (async () => {
      try {
        const { data, filename, action, sourceUrl } = msg
        const pdfUrl = sourceUrl || `file:///${filename}`
        log.log(`pdf process arraybuffer request: ${filename} (${action})`)
        
        const arrayBuffer = new Uint8Array(data).buffer
        
        if (action === 'summarize') {
          const result = await pdfService.summarizeFromArrayBuffer(arrayBuffer, filename)
          
          // parse bullets from summary
          const bullets = result.summary
            .split('\n')
            .filter(line => line.trim().startsWith('•') || line.trim().startsWith('-') || line.trim().startsWith('*'))
            .map(line => line.replace(/^[•\-*]\s*/, '').trim())
            .filter(Boolean)
          
          const finalBullets = bullets.length > 0 
            ? bullets 
            : result.summary.split(/[.!?]+/).filter(s => s.trim().length > 20).slice(0, 5)
          
          respond({ success: true, summary: { bullets: finalBullets }, text: result.fullText })
          
          ;(async () => {
            try {
              // cache summary to IDB
              const wordCount = result.fullText.split(/\s+/).length
              const readTimeMin = Math.max(1, Math.round(wordCount / 200))
              const summaryData = {
                title: filename.replace('.pdf', ''),
                bullets: finalBullets,
                readTime: `${readTimeMin} min`,
                fullContent: result.fullText,
                wordCount,
                timestamp: Date.now(),
                timing: { total: 0, cached: false }
              }
              
              await cacheService.setPageSummary(pdfUrl, summaryData, 3600)
              log.log(`cached PDF summary: ${filename}`)
              
              // broadcast progress for side panel reactivity
              const broadcastProgress = (percent: number) => {
                chrome.runtime.sendMessage({
                  type: 'INDEXING_PROGRESS',
                  sourceId: pdfUrl,
                  percent
                }).catch(() => {}) // ignore if side panel closed
              }
              
              // index to RAG for chat functionality
              await concurrencyManager.execute('indexing', pdfUrl, async (signal) => {
                if (signal.aborted) throw new Error('Indexing aborted')
                await ragService.indexChunks(result.fullText, {
                  sourceId: pdfUrl,
                  sourceUrl: pdfUrl,
                  sourceType: 'pdf',
                  title: filename.replace('.pdf', '')
                }, broadcastProgress)
              })
              
              log.log(`background indexing complete: ${filename}`)
            } catch (bgErr) {
              log.warn('background PDF indexing failed', (bgErr as Error).message)
            }
          })()
          
        } else {
          // extract text only
          const text = await pdfService.extractFromArrayBuffer(arrayBuffer)
          const wordCount = text.split(/\s+/).filter(Boolean).length
          respond({ success: true, text, wordCount })
        }
      } catch (err) {
        log.error('onPdfProcessArrayBuffer', (err as Error).message)
        respond({ success: false, error: (err as Error).message })
      }
    })()
  }

  // get cached page summary
  static _onGetPageCache(msg: { type: string; url: string }, respond: ResponseCallback): void {
    (async () => {
      try {
        const { url } = msg
        if (!url) {
          respond({ summary: null })
          return
        }
        
        const cached = await cacheService.getPageSummary(url)
        respond({ summary: cached })
      } catch (err) {
        log.error('onGetPageCache', (err as Error).message)
        respond({ summary: null })
      }
    })()
  }
}

