import { AIProvider, NullProvider, type ProviderCapabilities } from './AIProvider'
import { chromeAIProvider } from './ChromeAIProvider'
import { ollamaProvider } from './OllamaProvider'
import { storageService } from '../StorageService'
import { logger } from '../LoggerService'
import type {
  AICompleteRequest, AICompleteResponse, AISummarizeRequest, AISummarizeResponse,
  AITranslateRequest, AITranslateResponse, AIDetectLanguageRequest, AIDetectLanguageResponse,
  AIWriteRequest, AIWriteResponse, AIRewriteRequest, AIRewriteResponse
} from '../../types/chrome-ai'

const log = logger.createScoped('AIGateway')

type OperationType = 'complete' | 'summarize' | 'translate' | 'detectLanguage' | 'write' | 'rewrite'
export type AIProviderPreference = 'chrome-ai' | 'ollama'

interface GatewayConfig {
  preferChrome: boolean
  fallbackEnabled: boolean
  logOperations: boolean
}

class AIGatewayService {
  private providers: AIProvider[] = []
  private config: GatewayConfig = { preferChrome: true, fallbackEnabled: true, logOperations: true }
  private _initialized = false

  constructor() {
    this.providers = [chromeAIProvider, ollamaProvider].sort((a, b) => a.priority - b.priority)
    this._loadPreference()
  }

  private async _loadPreference(): Promise<void> {
    if (this._initialized) return
    try {
      const preferredProvider = await storageService.get<string>('preferredProvider', 'chrome-ai')
      if (preferredProvider === 'ollama') {
        this.config.preferChrome = false
      } else {
        this.config.preferChrome = true
      }
      this._initialized = true
      log.log('loaded preference: ' + (preferredProvider || 'chrome-ai'))
    } catch {
      this._initialized = true
    }
  }

  setPreference(provider: AIProviderPreference): void {
    this.config.preferChrome = provider === 'chrome-ai'
    log.log('preference set to: ' + provider)
  }

  getPreference(): AIProviderPreference {
    return this.config.preferChrome ? 'chrome-ai' : 'ollama'
  }

  private getProviderOrder(): AIProvider[] {
    if (!this.config.preferChrome) {
      return [...this.providers].sort((a, b) => {
        if (a.name === 'ollama') return -1
        if (b.name === 'ollama') return 1
        return a.priority - b.priority
      })
    }
    return this.providers
  }

  configure(config: Partial<GatewayConfig>): void {
    this.config = { ...this.config, ...config }
  }

  getConfig(): GatewayConfig {
    return { ...this.config }
  }

  async getAvailableProviders(): Promise<Array<{ name: string; available: boolean; capabilities: ProviderCapabilities }>> {
    const results = await Promise.all(
      this.providers.map(async (p) => ({
        name: p.name,
        available: await p.isAvailable(),
        capabilities: await p.getCapabilities()
      }))
    )
    return results
  }

  private async executeWithFallback<TReq, TRes extends { ok: boolean; error?: string }>(
    operation: OperationType,
    request: TReq,
    executor: (provider: AIProvider, req: TReq) => Promise<TRes>
  ): Promise<TRes> {
    const orderedProviders = this.getProviderOrder()
    const errors: string[] = []

    for (const provider of orderedProviders) {
      try {
        const caps = await provider.getCapabilities()
        if (!caps[operation]) continue

        if (this.config.logOperations) {
          log.debug('trying ' + operation + ' with ' + provider.name)
        }

        const result = await executor(provider, request)
        
        if (result.ok) {
          if (this.config.logOperations) {
            log.debug(operation + ' succeeded with ' + provider.name)
          }
          return result
        }

        errors.push(`${provider.name}: ${result.error}`)
        if (!this.config.fallbackEnabled) break
      } catch (err) {
        errors.push(`${provider.name}: ${(err as Error).message}`)
        if (!this.config.fallbackEnabled) break
      }
    }

    return { ok: false, error: `all providers failed: ${errors.join('; ')}` } as TRes
  }

  async complete(request: AICompleteRequest): Promise<AICompleteResponse> {
    return this.executeWithFallback('complete', request, (provider, req) => provider.complete(req))
  }

  async summarize(request: AISummarizeRequest): Promise<AISummarizeResponse> {
    return this.executeWithFallback('summarize', request, (provider, req) => provider.summarize(req))
  }

  async translate(request: AITranslateRequest): Promise<AITranslateResponse> {
    try {
      const chromeAvail = await chromeAIProvider.getCapabilities()
      if (chromeAvail.translate) {
        const result = await chromeAIProvider.translate(request)
        if (result.ok) return result
      }
    } catch { /* fall through */ }

    if (this.config.fallbackEnabled) {
      return ollamaProvider.translate(request)
    }
    return { ok: false, error: 'translation not available' }
  }

  async detectLanguage(request: AIDetectLanguageRequest): Promise<AIDetectLanguageResponse> {
    try {
      const chromeAvail = await chromeAIProvider.getCapabilities()
      if (chromeAvail.detectLanguage) {
        const result = await chromeAIProvider.detectLanguage(request)
        if (result.ok) return result
      }
    } catch { /* fall through */ }

    if (this.config.fallbackEnabled) {
      return ollamaProvider.detectLanguage(request)
    }
    return { ok: false, error: 'language detection not available' }
  }

  async write(request: AIWriteRequest): Promise<AIWriteResponse> {
    return this.executeWithFallback('write', request, (provider, req) => provider.write(req))
  }

  async rewrite(request: AIRewriteRequest): Promise<AIRewriteResponse> {
    return this.executeWithFallback('rewrite', request, (provider, req) => provider.rewrite(req))
  }

  get chrome(): typeof chromeAIProvider { return chromeAIProvider }
  get ollama(): typeof ollamaProvider { return ollamaProvider }
}

export const aiGateway = new AIGatewayService()
