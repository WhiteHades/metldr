import type {
  AICompleteRequest,
  AICompleteResponse,
  AISummarizeRequest,
  AISummarizeResponse,
  AITranslateRequest,
  AITranslateResponse,
  AIDetectLanguageRequest,
  AIDetectLanguageResponse,
  AIWriteRequest,
  AIWriteResponse,
  AIRewriteRequest,
  AIRewriteResponse,
  ChromeAIAvailability
} from '../../types/chrome-ai'

export type ProviderName = 'chrome-ai' | 'ollama'

export interface ProviderCapabilities {
  complete: boolean
  summarize: boolean
  translate: boolean
  detectLanguage: boolean
  write: boolean
  rewrite: boolean
}

export abstract class AIProvider {
  abstract readonly name: ProviderName
  abstract readonly priority: number // lower = higher priority for fallback ordering

  abstract isAvailable(): Promise<boolean>
  abstract getCapabilities(): Promise<ProviderCapabilities>
  abstract complete(request: AICompleteRequest): Promise<AICompleteResponse>
  abstract summarize(request: AISummarizeRequest): Promise<AISummarizeResponse>
  abstract translate(request: AITranslateRequest): Promise<AITranslateResponse>
  abstract detectLanguage(request: AIDetectLanguageRequest): Promise<AIDetectLanguageResponse>
  abstract write(request: AIWriteRequest): Promise<AIWriteResponse>
  abstract rewrite(request: AIRewriteRequest): Promise<AIRewriteResponse>
}

export class NullProvider extends AIProvider {
  readonly name: ProviderName = 'ollama'
  readonly priority = 999

  async isAvailable(): Promise<boolean> {
    return false
  }

  async getCapabilities(): Promise<ProviderCapabilities> {
    return {
      complete: false,
      summarize: false,
      translate: false,
      detectLanguage: false,
      write: false,
      rewrite: false
    }
  }

  async complete(): Promise<AICompleteResponse> {
    return { ok: false, error: 'no ai provider available' }
  }

  async summarize(): Promise<AISummarizeResponse> {
    return { ok: false, error: 'no ai provider available' }
  }

  async translate(): Promise<AITranslateResponse> {
    return { ok: false, error: 'no ai provider available' }
  }

  async detectLanguage(): Promise<AIDetectLanguageResponse> {
    return { ok: false, error: 'no ai provider available' }
  }

  async write(): Promise<AIWriteResponse> {
    return { ok: false, error: 'no ai provider available' }
  }

  async rewrite(): Promise<AIRewriteResponse> {
    return { ok: false, error: 'no ai provider available' }
  }
}
