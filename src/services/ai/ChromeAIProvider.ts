/// <reference path="../../types/chrome-ai.d.ts" />
import { AIProvider, type ProviderCapabilities, type ProviderName } from './AIProvider'
import { languageService } from '../LanguageService'
import type {
  AICompleteRequest, AICompleteResponse, AISummarizeRequest, AISummarizeResponse,
  AITranslateRequest, AITranslateResponse, AIDetectLanguageRequest, AIDetectLanguageResponse,
  AIWriteRequest, AIWriteResponse, AIRewriteRequest, AIRewriteResponse
} from '../../types/chrome-ai'

export class ChromeAIProvider extends AIProvider {
  readonly name: ProviderName = 'chrome-ai'
  readonly priority = 1

  private capabilitiesCache: ProviderCapabilities | null = null
  private cacheTimestamp = 0
  private readonly CACHE_TTL = 30000

  // chrome ai apis don't work in service workers directly, but we relay via offscreen
  private isServiceWorker(): boolean {
    return typeof window === 'undefined' && typeof self !== 'undefined' && 'registration' in self
  }

  // relay request through offscreen document (has window context)
  private async relayToOffscreen(action: string, payload: any): Promise<any> {
    try {
      // ensure offscreen doc exists
      const hasDoc = await chrome.offscreen?.hasDocument()
      if (!hasDoc) {
        await chrome.offscreen?.createDocument({
          url: 'offscreen.html',
          reasons: [chrome.offscreen.Reason.DOM_SCRAPING],
          justification: 'Run Chrome AI APIs'
        })
        await new Promise(r => setTimeout(r, 200))
      }
      
      let ready = false
      for (let i = 0; i < 15; i++) {
        try {
          const pong = await chrome.runtime.sendMessage({ target: 'offscreen', type: 'PING' })
          if (pong?.status === 'pong') {
            ready = true
            break
          }
        } catch {
          await new Promise(r => setTimeout(r, 200))
        }
      }
      
      if (!ready) {
        return { ok: false, error: 'Offscreen document not ready after retries' }
      }
      
      return await chrome.runtime.sendMessage({
        target: 'offscreen',
        type: 'CHROME_AI',
        action,
        payload
      })
    } catch (err) {
      console.error('[ChromeAI] Offscreen relay failed:', err)
      return { ok: false, error: (err as Error).message }
    }
  }

  async isAvailable(): Promise<boolean> {
    if (this.isServiceWorker()) {
      // relay to offscreen to check
      const result = await this.relayToOffscreen('capabilities', {})
      if (result?.ok && result.capabilities) {
        return Object.values(result.capabilities).some(v => v)
      }
      return false
    }
    try {
      const apis = {
        Summarizer: typeof Summarizer !== 'undefined',
        LanguageModel: typeof LanguageModel !== 'undefined',
        Translator: typeof Translator !== 'undefined',
        Writer: typeof Writer !== 'undefined',
        LanguageDetector: typeof LanguageDetector !== 'undefined',
        Rewriter: typeof Rewriter !== 'undefined'
      }
      return Object.values(apis).some(v => v)
    } catch {
      return false
    }
  }

  private async checkAPIAvailability(api: string): Promise<boolean> {
    try {
      switch (api) {
        case 'summarizer':
          if (typeof Summarizer === 'undefined') return false
          const sumAvail = await Summarizer.availability()
          return sumAvail === 'available' || sumAvail === 'downloadable'
        case 'translator':
          return typeof Translator !== 'undefined'
        case 'languageDetector':
          if (typeof LanguageDetector === 'undefined') return false
          const ldAvail = await LanguageDetector.availability()
          return ldAvail === 'available' || ldAvail === 'downloadable'
        case 'writer':
          if (typeof Writer === 'undefined') return false
          const wrAvail = await Writer.availability()
          return wrAvail === 'available' || wrAvail === 'downloadable'
        case 'rewriter':
          if (typeof Rewriter === 'undefined') return false
          const rwAvail = await Rewriter.availability()
          return rwAvail === 'available' || rwAvail === 'downloadable'
        case 'prompt':
          if (typeof LanguageModel === 'undefined') return false
          const lmAvail = await LanguageModel.availability({ languages: ['en', 'es', 'ja'] })
          const lmStatus = typeof lmAvail === 'string' ? lmAvail : lmAvail?.available
          return lmStatus === 'available' || lmStatus === 'downloadable' || lmStatus === 'readily'
        default:
          return false
      }
    } catch {
      return false
    }
  }

  async getCapabilities(): Promise<ProviderCapabilities> {
    // in service worker: relay to offscreen
    if (this.isServiceWorker()) {
      const result = await this.relayToOffscreen('capabilities', {})
      if (result?.ok && result.capabilities) {
        return result.capabilities
      }
      return { complete: false, summarize: false, translate: false, detectLanguage: false, write: false, rewrite: false }
    }

    const now = Date.now()
    if (this.capabilitiesCache && now - this.cacheTimestamp < this.CACHE_TTL) {
      return this.capabilitiesCache
    }

    const [complete, summarize, translate, detectLanguage, write, rewrite] = await Promise.all([
      this.checkAPIAvailability('prompt'),
      this.checkAPIAvailability('summarizer'),
      this.checkAPIAvailability('translator'),
      this.checkAPIAvailability('languageDetector'),
      this.checkAPIAvailability('writer'),
      this.checkAPIAvailability('rewriter')
    ])

    this.capabilitiesCache = { complete, summarize, translate, detectLanguage, write, rewrite }
    this.cacheTimestamp = now
    return this.capabilitiesCache
  }

  async complete(request: AICompleteRequest): Promise<AICompleteResponse> {
    const start = performance.now()
    
    // relay to offscreen in service worker context
    if (this.isServiceWorker()) {
      const result = await this.relayToOffscreen('complete', {
        systemPrompt: request.systemPrompt,
        userPrompt: request.userPrompt,
        messages: request.messages,
        temperature: request.temperature,
        responseConstraint: request.responseConstraint
      })
      return { ...result, timing: Math.round(performance.now() - start) }
    }
    
    try {
      if (typeof LanguageModel === 'undefined') {
        return { ok: false, error: 'LanguageModel API not available' }
      }

      const supportedLanguages = languageService.getSupportedLanguages()
      const avail = await LanguageModel.availability({ languages: supportedLanguages })
      const availStatus = typeof avail === 'string' ? avail : avail?.available
      if (availStatus === 'unavailable' || availStatus === 'no') {
        return { ok: false, error: 'LanguageModel not available' }
      }

      const detectedLang = await languageService.detect(request.userPrompt.substring(0, 500))

      const initialPrompts: Array<{ role: 'system' | 'user' | 'assistant'; content: string }> = [
        { role: 'system', content: request.systemPrompt }
      ]
      
      if (request.messages?.length) {
        initialPrompts.push(...request.messages.slice(-6))
      }

      const session = await LanguageModel.create({
        initialPrompts,
        temperature: request.temperature ?? 0.7,
        topK: 3,
        expectedInputLanguages: supportedLanguages,
        outputLanguage: detectedLang
      })

      try {
        const promptOptions: { responseConstraint?: object } = {}
        if (request.responseConstraint) {
          promptOptions.responseConstraint = request.responseConstraint
        }
        
        const response = await session.prompt(request.userPrompt, promptOptions)
        return {
          ok: true,
          content: response,
          provider: 'chrome-ai',
          model: 'gemini-nano',
          timing: Math.round(performance.now() - start)
        }
      } finally {
        session.destroy()
      }
    } catch (err) {
      return { ok: false, error: (err as Error).message, provider: 'chrome-ai' }
    }
  }

  async summarize(request: AISummarizeRequest): Promise<AISummarizeResponse> {
    const start = performance.now()

    // relay to offscreen in service worker context
    if (this.isServiceWorker()) {
      const result = await this.relayToOffscreen('summarize', {
        content: request.content,
        context: request.context,
        type: request.type,
        length: request.length
      })
      return { ...result, timing: Math.round(performance.now() - start) }
    }

    try {
      if (typeof Summarizer === 'undefined') {
        return { ok: false, error: 'Summarizer API not available' }
      }

      const avail = await Summarizer.availability()
      if (avail === 'unavailable') {
        return { ok: false, error: 'Summarizer not available' }
      }

      const detectedLang = await languageService.detect(request.content.substring(0, 500))
      const supportedLanguages = languageService.getSupportedLanguages()

      const summarizer = await Summarizer.create({
        type: request.type || 'key-points',
        length: request.length || 'medium',
        format: 'markdown',
        expectedInputLanguages: supportedLanguages,
        expectedContextLanguages: supportedLanguages,
        outputLanguage: detectedLang
      })

      try {
        const summary = await summarizer.summarize(request.content, { context: request.context })
        return { ok: true, summary, provider: 'chrome-ai', timing: Math.round(performance.now() - start) }
      } finally {
        summarizer.destroy()
      }
    } catch (err) {
      return { ok: false, error: (err as Error).message, provider: 'chrome-ai' }
    }
  }

  async translate(request: AITranslateRequest): Promise<AITranslateResponse> {
    const start = performance.now()

    try {
      if (typeof Translator === 'undefined') {
        return { ok: false, error: 'Translator API not available' }
      }

      const avail = await Translator.availability({
        sourceLanguage: request.sourceLanguage,
        targetLanguage: request.targetLanguage
      })

      if (avail === 'unavailable') {
        return { ok: false, error: `Translation ${request.sourceLanguage}→${request.targetLanguage} not available` }
      }

      const translator = await Translator.create({
        sourceLanguage: request.sourceLanguage,
        targetLanguage: request.targetLanguage
      })

      try {
        const translation = await translator.translate(request.text)
        return { ok: true, translation, provider: 'chrome-ai', timing: Math.round(performance.now() - start) }
      } finally {
        translator.destroy()
      }
    } catch (err) {
      return { ok: false, error: (err as Error).message, provider: 'chrome-ai' }
    }
  }

  async detectLanguage(request: AIDetectLanguageRequest): Promise<AIDetectLanguageResponse> {
    const start = performance.now()

    if (this.isServiceWorker()) {
      const result = await this.relayToOffscreen('detectLanguage', { text: request.text })
      return { ...result, timing: Math.round(performance.now() - start) }
    }

    try {
      if (typeof LanguageDetector === 'undefined') {
        return { ok: false, error: 'LanguageDetector API not available' }
      }

      const avail = await LanguageDetector.availability()
      if (avail === 'unavailable') {
        return { ok: false, error: 'LanguageDetector not available' }
      }

      const detector = await LanguageDetector.create()

      try {
        const results = await detector.detect(request.text)
        if (!results.length) {
          return { ok: false, error: 'no language detected' }
        }
        const top = results[0]
        return { ok: true, language: top.detectedLanguage, confidence: top.confidence, allResults: results, provider: 'chrome-ai' }
      } finally {
        detector.destroy()
      }
    } catch (err) {
      return { ok: false, error: (err as Error).message, provider: 'chrome-ai' }
    }
  }

  async write(request: AIWriteRequest): Promise<AIWriteResponse> {
    const start = performance.now()

    try {
      if (typeof Writer === 'undefined') {
        return { ok: false, error: 'Writer API not available' }
      }

      const avail = await Writer.availability()
      if (avail === 'unavailable') {
        return { ok: false, error: 'Writer not available' }
      }

      const detectedLang = await languageService.detect(request.prompt.substring(0, 500))
      const supportedLanguages = languageService.getSupportedLanguages()

      const writer = await Writer.create({
        tone: request.tone || 'neutral',
        length: request.length || 'medium',
        sharedContext: request.context,
        expectedInputLanguages: supportedLanguages,
        expectedContextLanguages: supportedLanguages,
        outputLanguage: detectedLang
      })

      try {
        const content = await writer.write(request.prompt)
        return { ok: true, content, provider: 'chrome-ai', timing: Math.round(performance.now() - start) }
      } finally {
        writer.destroy()
      }
    } catch (err) {
      return { ok: false, error: (err as Error).message, provider: 'chrome-ai' }
    }
  }

  async rewrite(request: AIRewriteRequest): Promise<AIRewriteResponse> {
    const start = performance.now()

    try {
      if (typeof Rewriter === 'undefined') {
        return { ok: false, error: 'Rewriter API not available' }
      }

      const avail = await Rewriter.availability()
      if (avail === 'unavailable') {
        return { ok: false, error: 'Rewriter not available' }
      }

      const detectedLang = await languageService.detect(request.text.substring(0, 500))
      const supportedLanguages = languageService.getSupportedLanguages()

      const rewriter = await Rewriter.create({
        tone: request.tone || 'as-is',
        length: request.length || 'as-is',
        sharedContext: request.context,
        expectedInputLanguages: supportedLanguages,
        expectedContextLanguages: supportedLanguages,
        outputLanguage: detectedLang
      })

      try {
        const content = await rewriter.rewrite(request.text)
        return { ok: true, content, provider: 'chrome-ai', timing: Math.round(performance.now() - start) }
      } finally {
        rewriter.destroy()
      }
    } catch (err) {
      return { ok: false, error: (err as Error).message, provider: 'chrome-ai' }
    }
  }
}

export const chromeAIProvider = new ChromeAIProvider()
