import { OllamaService } from './OllamaService'
import { cacheService } from './CacheService'
import { dictionaryService } from './DictionaryService'
import { WordService } from './WordService'
import { EmailService } from './EmailService'
import { PageService } from './PageService'
import { SummaryPrefs } from '../lib/summaryPrefs'
import type {
  EmailSummaryMessage,
  GetReplySuggestionsMessage,
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
  ExtractedData
} from '../types'

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
    console.log('[BackgroundBootstrap] starting initialization...')
    
    chrome.sidePanel
      .setPanelBehavior({ openPanelOnActionClick: true })
      .catch(err => console.error('[BackgroundBootstrap] side panel error:', err))

    try {
      await cacheService.init()
      console.log('[BackgroundBootstrap] cache service ready')
    } catch (err) {
      console.error('[BackgroundBootstrap] cache init failed:', (err as Error).message)
    }
    
    try {
      await dictionaryService.init()
      console.log('[BackgroundBootstrap] dictionary service ready')
    } catch (err) {
      console.error('[BackgroundBootstrap] dictionary init failed:', (err as Error).message)
    }
    
    this.isInitialized = true

    chrome.runtime.onMessage.addListener((msg: BackgroundMessage, _sender, respond: ResponseCallback) => {
      if (msg.type === 'SUMMARIZE_EMAIL') {
        this._onEmailSummary(msg, respond)
        return true
      }

      if (msg.type === 'GET_REPLY_SUGGESTIONS') {
        this._onGetReplySuggestions(msg, respond)
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

      return false
    })

    console.log('[BackgroundBootstrap] initialized')
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
        console.error('[BackgroundBootstrap._onEmailSummary]', (err as Error).message)
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
        console.error('[BackgroundBootstrap._onGetReplySuggestions]', (err as Error).message)
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
          respond({ success: true, result })
        } else {
          console.log('[BackgroundBootstrap._onWordLookup] no result found')
          respond({ success: false, error: 'word not found' })
        }
      } catch (err) {
        console.error('[BackgroundBootstrap._onWordLookup]', (err as Error).message)
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
        console.error('[BackgroundBootstrap._onHealthCheck]', (err as Error).message)
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
          console.error('[BackgroundBootstrap._onChatMessage] no result from PageService')
          respond({ ok: false, error: 'no response from chat service' })
          return
        }
        
        respond({
          ...result,
          timing: result.timing || null
        })
        
      } catch (err) {
        console.error('[BackgroundBootstrap._onChatMessage] error:', (err as Error).message)
        respond({ ok: false, error: (err as Error).message || 'chat processing failed' })
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
      console.error('[BackgroundBootstrap] summary queue error:', (err as Error).message)
      return { success: false, error: (err as Error).message }
    })
    return this.summaryQueue as Promise<T | { success: false; error: string }>
  }

  static async _performExtractAndSummarize(tabId: number, force: boolean, trigger = 'auto'): Promise<SummaryResult> {
    try {
      console.log('[BackgroundBootstrap] _performExtractAndSummarize:', { tabId, force, trigger })
      const prefs = await this._getSummaryPrefs()
      let extracted: ExtractedData | undefined
      try {
        const response = await chrome.tabs.sendMessage(tabId, { type: 'EXTRACT_ARTICLE' }) as { success: boolean; data?: ExtractedData; error?: string }
        console.log('[BackgroundBootstrap] EXTRACT_ARTICLE response:', { success: response?.success, hasData: !!response?.data, error: response?.error })
        if (!response?.success) {
          return { success: false, error: response?.error || 'extraction failed' }
        }
        extracted = response.data
      } catch (err) {
        console.error('[BackgroundBootstrap] content script error:', (err as Error).message)
        return { success: false, error: 'content script not ready - please refresh the page' }
      }

      if (!extracted) return { success: false, error: 'extraction failed' }
      if (extracted.skip) return { success: false, skip: true, reason: extracted.reason }
      const gating = this._classifyPage(extracted, prefs, trigger, force)
      console.log('[BackgroundBootstrap] classification:', { action: gating.action, reason: gating.reason, wordCount: extracted?.wordCount })
      if (gating.action === 'skip') return { success: false, skip: true, reason: gating.reason }
      if (gating.action === 'prompt') return { success: false, prompt: true, reason: gating.reason }

      const summary = await PageService.summarize(extracted as Parameters<typeof PageService.summarize>[0], force)
      return { success: true, summary }
    } catch (err) {
      console.error('[BackgroundBootstrap._performExtractAndSummarize]', (err as Error).message)
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
}
