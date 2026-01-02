export { AIProvider, NullProvider, type ProviderCapabilities, type ProviderName } from './AIProvider'
export { ChromeAIProvider, chromeAIProvider } from './ChromeAIProvider'
export { OllamaProvider, ollamaProvider } from './OllamaProvider'
export { localModels } from './LocalModelProvider'
export { aiGateway } from './AIGateway'
export { mapReduceService } from './MapReduceService'
export { AIPrompts, type EmailContext, type PageChatContext } from './AIPrompts'

export type {
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
  ChromeAIAvailability,
  ChromeAICapabilities
} from '../../types/chrome-ai'
