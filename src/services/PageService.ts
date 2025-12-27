import { OllamaService } from './OllamaService'
import { cacheService } from './CacheService'
import { aiGateway, AIPrompts } from './ai'
import type { ExtractedData, SummaryTiming, PageSummary, ChatMessage, PageContext, ChatResult } from '../types'

export class PageService {

  static async summarize(extractedData: ExtractedData, force = false): Promise<PageSummary> {
    const startTotal = performance.now()
    const timing: SummaryTiming = { extraction: 0, llm: 0, total: 0 }
    const { title, url, content, author, publishDate, publication, wordCount, readTime, extractionTime } = extractedData    
    timing.extraction = extractionTime || 0
    
    if (!force && url) {
      const cached = await cacheService.getPageSummary(url) as PageSummary | null
      if (cached) {
        cached.timing = { ...cached.timing, cached: true, total: Math.round(performance.now() - startTotal) }
        return cached
      }
    }

    let metadata = `ARTICLE METADATA:\n- Title: "${title}"\n`
    if (author) metadata += `- Author: ${author}\n`
    if (publication) metadata += `- Publication: ${publication}\n`
    if (publishDate) metadata += `- Published: ${publishDate}\n`
    metadata += '\n---\nARTICLE CONTENT:\n\n'

    const fullContent = metadata + content
    const llmStart = performance.now()

    console.log('[PageService] summarizing, length:', fullContent.length)

    const result = await aiGateway.summarize({
      content: fullContent,
      context: `This article is titled "${title}"`,
      type: 'key-points',
      length: 'medium'
    })

    timing.llm = Math.round(performance.now() - llmStart)

    console.log('[PageService] result:', { ok: result.ok, provider: result.provider, len: result.summary?.length })

    if (!result.ok) throw new Error(result.error || 'summarization failed')

    const bullets = this._parseBullets(result.summary || '')

    timing.total = Math.round(performance.now() - startTotal)
    timing.cached = false
    timing.model = result.provider === 'chrome-ai' ? 'gemini-nano' : 'ollama'
    timing.provider = result.provider

    const summary: PageSummary = {
      title, author, publishDate, publication,
      bullets: bullets.length ? bullets : ['could not generate summary'],
      readTime, fullContent, wordCount,
      timestamp: Date.now(),
      timing
    }

    if (url) await cacheService.setPageSummary(url, summary, 3600)
    return summary
  }

  private static _parseBullets(text: string): string[] {
    const lines = text.split('\n').map(l => l.trim()).filter(l => l.length > 0)
    
    // match: - bullet, • bullet, * bullet, 1. numbered
    const bullets = lines
      .filter(l => /^[-•*]|^\d+\./.test(l))
      .map(l => l.replace(/^[-•*]\s*/, '').replace(/^\d+\.\s*/, '').trim())
      .filter(l => l.length > 10)
      .slice(0, 5)

    // fallback: use raw lines if no bullets found
    if (bullets.length === 0 && lines.length > 0) {
      return lines.filter(l => l.length > 15).slice(0, 3)
    }

    return bullets
  }

  static async chat(messages: ChatMessage[], pageContext: PageContext | null, model: string | null): Promise<ChatResult> {
    const startTime = performance.now()
    
    let contextText = pageContext?.fullContent || ''

    const MAX_CONTEXT = 50000
    if (contextText.length > MAX_CONTEXT) {
      const headLen = Math.floor(MAX_CONTEXT * 0.6)
      const tailLen = MAX_CONTEXT - headLen
      contextText = contextText.slice(0, headLen) + 
        '\n\n[...content truncated for brevity...]\n\n' + 
        contextText.slice(-tailLen)
    }

    const systemPrompt = contextText 
      ? AIPrompts.chat.withContext({ content: contextText })
      : AIPrompts.chat.noContext

    const lastUserMsg = messages.filter(m => m.role === 'user').pop()?.content || ''

    // use aiGateway.complete which respects provider preference
    const result = await aiGateway.complete({
      systemPrompt,
      userPrompt: lastUserMsg,
      temperature: 0.3
    })
    
    if (result.ok) {
      const isChrome = aiGateway.getPreference() === 'chrome-ai'
      return {
        ok: true,
        content: result.content,
        timing: { 
          total: Math.round(performance.now() - startTime), 
          model: isChrome ? 'gemini-nano' : (model || 'ollama')
        }
      }
    }

    // fallback to ollama directly if gateway failed
    let selectedModel = model
    if (!selectedModel) selectedModel = await OllamaService.selectBest('page_summary')
    
    if (!selectedModel) {
      return { ok: false, error: result.error || 'no ai available' }
    }

    const useLongTimeout = contextText.length > 2000

    try {
      const ollamaResult = await OllamaService.complete(
        selectedModel,
        [{ role: 'system', content: systemPrompt }, ...messages.slice(-6)],
        { temperature: 0.2, longContext: useLongTimeout }
      )
      
      return {
        ...ollamaResult,
        timing: { total: Math.round(performance.now() - startTime), model: selectedModel }
      }
    } catch (err) {
      console.error('[PageService.chat]', (err as Error).message)
      return { ok: false, error: (err as Error).message || 'chat failed' }
    }
  }
}

