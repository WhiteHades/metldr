/**
 * ollama provider adapter
 * 
 * wraps existing OllamaService as an AIProvider
 * adapter pattern: makes OllamaService compatible with AIProvider interface
 */

import { AIProvider, type ProviderCapabilities, type ProviderName } from './AIProvider'
import { OllamaService } from '../OllamaService'
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
  AIRewriteResponse
} from '../../types/chrome-ai'

export class OllamaProvider extends AIProvider {
  readonly name: ProviderName = 'ollama'
  readonly priority = 2 // secondary to chrome ai

  private availabilityCache: boolean | null = null
  private cacheTimestamp = 0
  private readonly CACHE_TTL = 10000 // 10 seconds

  /**
   * check if ollama is running and has models
   */
  async isAvailable(): Promise<boolean> {
    const now = Date.now()
    if (this.availabilityCache !== null && now - this.cacheTimestamp < this.CACHE_TTL) {
      return this.availabilityCache
    }

    try {
      const { available, models } = await OllamaService.checkAvailable()
      this.availabilityCache = available && models.length > 0
      this.cacheTimestamp = now
      return this.availabilityCache
    } catch {
      this.availabilityCache = false
      this.cacheTimestamp = now
      return false
    }
  }

  /**
   * ollama can do anything via prompting
   */
  async getCapabilities(): Promise<ProviderCapabilities> {
    const available = await this.isAvailable()
    return {
      complete: available,
      summarize: available,
      translate: available,
      detectLanguage: available,
      write: available,
      rewrite: available
    }
  }


  async complete(request: AICompleteRequest): Promise<AICompleteResponse> {
    const start = performance.now()

    try {
      const model = await OllamaService.getUserSelected() || await OllamaService.selectBest('email_summary')
      if (!model) {
        return { ok: false, error: 'no ollama model available' }
      }

      // build messages array with optional chat history
      const messages: Array<{ role: 'system' | 'user' | 'assistant'; content: string }> = [
        { role: 'system', content: request.systemPrompt }
      ]
      
      // include chat history if provided (for multi-turn conversations)
      if (request.messages?.length) {
        messages.push(...request.messages.slice(-6)) // last 6 messages for context
      }
      
      // always add current user prompt as final message
      messages.push({ role: 'user', content: request.userPrompt })

      const result = await OllamaService.complete(model, messages, {
        temperature: request.temperature ?? 0.5
      })

      if (!result.ok) {
        return { ok: false, error: result.error, provider: 'ollama' }
      }

      return {
        ok: true,
        content: result.content,
        provider: 'ollama',
        model,
        timing: Math.round(performance.now() - start)
      }
    } catch (err) {
      console.error('[OllamaProvider.complete] error:', err)
      return { ok: false, error: (err as Error).message, provider: 'ollama' }
    }
  }

  async summarize(request: AISummarizeRequest): Promise<AISummarizeResponse> {
    const start = performance.now()

    try {
      const model = await OllamaService.getUserSelected() || await OllamaService.selectBest('page_summary')
      if (!model) {
        return { ok: false, error: 'no ollama model available' }
      }

      // map type to prompt instructions
      const typeInstructions: Record<string, string> = {
        'key-points': 'Extract the most important points, presented as a bullet list starting with "-"',
        'tldr': 'Provide a short, concise summary in 1-2 sentences',
        'teaser': 'Write an intriguing teaser that makes the reader want to learn more',
        'headline': 'Create a single headline that captures the main point'
      }

      // map length to target
      const lengthTargets: Record<string, string> = {
        'short': 'Be very brief (1-3 bullets or 1-2 sentences)',
        'medium': 'Use moderate length (3-5 bullets or 3-4 sentences)',
        'long': 'Be comprehensive (5-7 bullets or 5-6 sentences)'
      }

      const typeInstr = typeInstructions[request.type || 'key-points']
      const lengthInstr = lengthTargets[request.length || 'medium']

      const systemPrompt = `You are a factual summarizer. ${typeInstr}. ${lengthInstr}.
Rules:
1. Only use information explicitly stated in the content
2. Never infer or add information not in the text
3. Be specific, not vague`

      const userPrompt = request.context 
        ? `Context: ${request.context}\n\nContent to summarize:\n${request.content}`
        : `Summarize this content:\n${request.content}`

      const result = await OllamaService.complete(model, [
        { role: 'system', content: systemPrompt },
        { role: 'user', content: userPrompt }
      ], { temperature: 0.1 })

      if (!result.ok) {
        return { ok: false, error: result.error, provider: 'ollama' }
      }

      return {
        ok: true,
        summary: result.content,
        provider: 'ollama',
        timing: Math.round(performance.now() - start)
      }
    } catch (err) {
      console.error('[OllamaProvider.summarize] error:', err)
      return { ok: false, error: (err as Error).message, provider: 'ollama' }
    }
  }

  async translate(request: AITranslateRequest): Promise<AITranslateResponse> {
    const start = performance.now()

    try {
      const model = await OllamaService.getUserSelected() || await OllamaService.selectBest('word_lookup')
      if (!model) {
        return { ok: false, error: 'no ollama model available' }
      }

      const systemPrompt = `You are a professional translator. Translate text accurately from ${request.sourceLanguage} to ${request.targetLanguage}. 
Only output the translation, nothing else.`

      const result = await OllamaService.complete(model, [
        { role: 'system', content: systemPrompt },
        { role: 'user', content: request.text }
      ], { temperature: 0.1 })

      if (!result.ok) {
        return { ok: false, error: result.error, provider: 'ollama' }
      }

      return {
        ok: true,
        translation: result.content?.trim(),
        provider: 'ollama' as const,
        timing: Math.round(performance.now() - start)
      }
    } catch (err) {
      console.error('[OllamaProvider.translate] error:', err)
      return { ok: false, error: (err as Error).message }
    }
  }

  async detectLanguage(request: AIDetectLanguageRequest): Promise<AIDetectLanguageResponse> {
    const start = performance.now()

    try {
      const model = await OllamaService.getUserSelected() || await OllamaService.selectBest('word_lookup')
      if (!model) {
        return { ok: false, error: 'no ollama model available' }
      }

      const systemPrompt = `Detect the language of the provided text.
Reply with ONLY the ISO 639-1 two-letter language code (e.g., "en", "de", "fr", "es", "ja", "zh").
Do not explain or add anything else.`

      const result = await OllamaService.complete(model, [
        { role: 'system', content: systemPrompt },
        { role: 'user', content: request.text }
      ], { temperature: 0 })

      if (!result.ok) {
        return { ok: false, error: result.error, provider: 'ollama' }
      }

      // extract language code from response
      const langCode = result.content?.trim().toLowerCase().slice(0, 2) || 'en'

      return {
        ok: true,
        language: langCode,
        confidence: 0.8, // ollama doesn't provide confidence
        provider: 'ollama' as const
      }
    } catch (err) {
      console.error('[OllamaProvider.detectLanguage] error:', err)
      return { ok: false, error: (err as Error).message }
    }
  }

  async write(request: AIWriteRequest): Promise<AIWriteResponse> {
    const start = performance.now()

    try {
      const model = await OllamaService.getUserSelected() || await OllamaService.selectBest('email_summary')
      if (!model) {
        return { ok: false, error: 'no ollama model available' }
      }

      const toneInstructions: Record<string, string> = {
        'formal': 'Use a formal, professional tone',
        'neutral': 'Use a neutral, balanced tone',
        'casual': 'Use a casual, friendly tone'
      }

      const lengthInstructions: Record<string, string> = {
        'short': 'Keep it brief (1-2 paragraphs)',
        'medium': 'Use moderate length (2-3 paragraphs)',
        'long': 'Be comprehensive (3-5 paragraphs)'
      }

      const systemPrompt = `You are a helpful writing assistant.
${toneInstructions[request.tone || 'neutral']}.
${lengthInstructions[request.length || 'medium']}.
${request.context ? `Context: ${request.context}` : ''}
Write clear, well-structured content.`

      const result = await OllamaService.complete(model, [
        { role: 'system', content: systemPrompt },
        { role: 'user', content: request.prompt }
      ], { temperature: 0.7 })

      if (!result.ok) {
        return { ok: false, error: result.error, provider: 'ollama' }
      }

      return {
        ok: true,
        content: result.content,
        provider: 'ollama',
        timing: Math.round(performance.now() - start)
      }
    } catch (err) {
      console.error('[OllamaProvider.write] error:', err)
      return { ok: false, error: (err as Error).message, provider: 'ollama' }
    }
  }

  async rewrite(request: AIRewriteRequest): Promise<AIRewriteResponse> {
    const start = performance.now()

    try {
      const model = await OllamaService.getUserSelected() || await OllamaService.selectBest('email_summary')
      if (!model) {
        return { ok: false, error: 'no ollama model available' }
      }

      const toneInstructions: Record<string, string> = {
        'as-is': 'Maintain the same tone',
        'more-formal': 'Make the tone more formal and professional',
        'more-casual': 'Make the tone more casual and friendly'
      }

      const lengthInstructions: Record<string, string> = {
        'as-is': 'Maintain similar length',
        'shorter': 'Make it more concise',
        'longer': 'Expand with more detail'
      }

      const systemPrompt = `You are an expert editor.
Rewrite the following text.
${toneInstructions[request.tone || 'as-is']}.
${lengthInstructions[request.length || 'as-is']}.
${request.context ? `Context: ${request.context}` : ''}
Only output the rewritten text.`

      const result = await OllamaService.complete(model, [
        { role: 'system', content: systemPrompt },
        { role: 'user', content: request.text }
      ], { temperature: 0.5 })

      if (!result.ok) {
        return { ok: false, error: result.error, provider: 'ollama' }
      }

      return {
        ok: true,
        content: result.content,
        provider: 'ollama',
        timing: Math.round(performance.now() - start)
      }
    } catch (err) {
      console.error('[OllamaProvider.rewrite] error:', err)
      return { ok: false, error: (err as Error).message, provider: 'ollama' }
    }
  }

  /**
   * invalidate cache (useful when user changes ollama settings)
   */
  invalidateCache(): void {
    this.availabilityCache = null
    this.cacheTimestamp = 0
  }
}

// singleton instance
export const ollamaProvider = new OllamaProvider()
