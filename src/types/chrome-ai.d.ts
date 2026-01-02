/* eslint-disable @typescript-eslint/no-explicit-any */

export {}

declare global {
  // summarizer api
  interface SummarizerInstance {
    summarize(input: string, options?: { context?: string; signal?: AbortSignal }): Promise<string>
    summarizeStreaming(input: string, options?: { context?: string }): ReadableStream<string>
    destroy(): void
  }

  interface SummarizerConstructor {
    availability(options?: {
      type?: string
      format?: string
      length?: string
    }): Promise<'available' | 'downloadable' | 'downloading' | 'unavailable'>
    create(options?: {
      type?: 'key-points' | 'tldr' | 'teaser' | 'headline'
      format?: 'markdown' | 'plain-text'
      length?: 'short' | 'medium' | 'long'
      sharedContext?: string
      expectedInputLanguages?: string[]
      outputLanguage?: string
      expectedContextLanguages?: string[]
      monitor?: (m: { addEventListener: (event: string, cb: (e: { loaded: number }) => void) => void }) => void
    }): Promise<SummarizerInstance>
  }

  var Summarizer: SummarizerConstructor | undefined

  // translator api
  interface TranslatorInstance {
    translate(text: string): Promise<string>
    translateStreaming(text: string): ReadableStream<string>
    destroy(): void
  }

  interface TranslatorConstructor {
    availability(options: { sourceLanguage: string; targetLanguage: string }): Promise<'available' | 'downloadable' | 'downloading' | 'unavailable'>
    create(options: {
      sourceLanguage: string
      targetLanguage: string
      monitor?: (m: { addEventListener: (event: string, cb: (e: { loaded: number }) => void) => void }) => void
    }): Promise<TranslatorInstance>
  }

  var Translator: TranslatorConstructor | undefined

  // language detector api
  interface LanguageDetectorResult {
    detectedLanguage: string
    confidence: number
  }

  interface LanguageDetectorInstance {
    detect(text: string): Promise<LanguageDetectorResult[]>
    destroy(): void
  }

  interface LanguageDetectorConstructor {
    availability(): Promise<'available' | 'downloadable' | 'downloading' | 'unavailable'>
    create(options?: {
      monitor?: (m: { addEventListener: (event: string, cb: (e: { loaded: number }) => void) => void }) => void
    }): Promise<LanguageDetectorInstance>
  }

  var LanguageDetector: LanguageDetectorConstructor | undefined

  // writer api
  interface WriterInstance {
    write(prompt: string, options?: { context?: string; signal?: AbortSignal }): Promise<string>
    writeStreaming(prompt: string, options?: { context?: string }): ReadableStream<string>
    destroy(): void
  }

  interface WriterConstructor {
    availability(): Promise<'available' | 'downloadable' | 'downloading' | 'unavailable'>
    create(options?: {
      tone?: 'formal' | 'neutral' | 'casual'
      format?: 'plain-text' | 'markdown'
      length?: 'short' | 'medium' | 'long'
      sharedContext?: string
      expectedInputLanguages?: string[]
      expectedContextLanguages?: string[]
      outputLanguage?: string
      monitor?: (m: { addEventListener: (event: string, cb: (e: { loaded: number }) => void) => void }) => void
    }): Promise<WriterInstance>
  }

  var Writer: WriterConstructor | undefined

  interface RewriterInstance {
    rewrite(text: string, options?: { context?: string; signal?: AbortSignal }): Promise<string>
    rewriteStreaming(text: string, options?: { context?: string }): ReadableStream<string>
    destroy(): void
  }

  interface RewriterConstructor {
    availability(): Promise<'available' | 'downloadable' | 'downloading' | 'unavailable'>
    create(options?: {
      tone?: 'as-is' | 'more-formal' | 'more-casual'
      format?: 'as-is' | 'plain-text' | 'markdown'
      length?: 'as-is' | 'shorter' | 'longer'
      sharedContext?: string
      expectedInputLanguages?: string[]
      expectedContextLanguages?: string[]
      outputLanguage?: string
      monitor?: (m: { addEventListener: (event: string, cb: (e: { loaded: number }) => void) => void }) => void
    }): Promise<RewriterInstance>
  }

  var Rewriter: RewriterConstructor | undefined

  // language model (prompt) api
  interface LanguageModelSession {
    prompt(input: string, options?: { signal?: AbortSignal }): Promise<string>
    promptStreaming(input: string, options?: { signal?: AbortSignal }): ReadableStream<string>
    clone(): Promise<LanguageModelSession>
    destroy(): void
    readonly inputUsage: number
    readonly inputQuota: number
  }

  interface LanguageModelAvailability {
    available: 'available' | 'downloadable' | 'downloading' | 'unavailable' | 'no' | 'readily' | 'after-download'
    defaultTopK?: number
    maxTopK?: number
    defaultTemperature?: number
  }

  interface LanguageModelConstructor {
    availability(options?: { languages?: string[] }): Promise<string | LanguageModelAvailability>
    params(): Promise<{ defaultTopK: number; maxTopK: number; defaultTemperature: number; maxTemperature: number }>
    create(options?: {
      initialPrompts?: Array<{ role: 'system' | 'user' | 'assistant'; content: string }>
      systemPrompt?: string
      temperature?: number
      topK?: number
      expectedInputs?: Array<{ type: 'text' | 'image' | 'audio'; languages?: string[] }>
      expectedOutputs?: Array<{ type: 'text'; languages: string[] }>
      expectedInputLanguages?: string[]
      outputLanguage?: string
      signal?: AbortSignal
      monitor?: (m: { addEventListener: (event: string, cb: (e: { loaded: number }) => void) => void }) => void
    }): Promise<LanguageModelSession>
  }

  var LanguageModel: LanguageModelConstructor | undefined
}
