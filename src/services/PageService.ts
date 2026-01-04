import { OllamaService } from './OllamaService'
import { cacheService } from './CacheService'
import { aiGateway, AIPrompts, mapReduceService } from './ai'
import { analyticsService } from './AnalyticsService'
import type { ExtractedData, SummaryTiming, PageSummary, ChatMessage, PageContext, ChatResult } from '../types'

import { ragService } from './rag/RagService'

export class PageService {

  static async summarize(extractedData: ExtractedData, force = false): Promise<PageSummary> {
    const startTotal = performance.now()
    const timing: SummaryTiming = { extraction: 0, llm: 0, total: 0 }
    const { title, url, content, author, publishDate, publication, wordCount, readTime, extractionTime } = extractedData    
    timing.extraction = extractionTime || 0
    
    if (url && content) {
      ragService.indexChunks(content, {
        sourceId: url,
        sourceUrl: url,
        sourceType: 'article',
        title: title || 'Untitled Article'
      }).catch(err => console.warn('[PageService] Chunk indexing failed', err))
    }

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
// ... existing code ...

    console.log('[PageService] summarizing, length:', fullContent.length)

    let summaryText: string
    let usedMapReduce = false

    // use map-reduce for long content that exceeds LLM context
    if (mapReduceService.needsMapReduce(fullContent)) {
      console.log('[PageService] using map-reduce for long content')
      const mrResult = await mapReduceService.summarize(content, `Article titled "${title}"`)
      summaryText = mrResult.summary
      usedMapReduce = true
      console.log('[PageService] map-reduce complete:', { chunks: mrResult.chunkCount, timing: mrResult.timing })
    } else {
      const result = await aiGateway.summarize({
        content: fullContent,
        context: `This article is titled "${title}"`,
        type: 'key-points',
        length: 'medium'
      })
      
      if (!result.ok) throw new Error(result.error || 'summarization failed')
      summaryText = result.summary || ''
      console.log('[PageService] direct summarization result:', { ok: result.ok, provider: result.provider, len: summaryText.length })
    }

    timing.llm = Math.round(performance.now() - llmStart)

    const bullets = this._parseBullets(summaryText)

    timing.total = Math.round(performance.now() - startTotal)
    timing.cached = false
    timing.model = usedMapReduce ? 'map-reduce' : 'gemini-nano'

    const summary: PageSummary = {
      title, author, publishDate, publication,
      bullets: bullets.length ? bullets : ['could not generate summary'],
      readTime, fullContent, wordCount,
      timestamp: Date.now(),
      timing
    }

    if (url) await cacheService.setPageSummary(url, summary, 3600)

    analyticsService.trackSummary({
      type: 'page',
      cached: false,
      responseTimeMs: timing.llm || 0,
      wordsIn: wordCount || 0,
      wordsOut: bullets.join(' ').split(/\s+/).length,
      tokensOut: Math.round(summaryText.length / 4),
      model: timing.model,
      url
    }).catch(() => {})

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

    // RAG: Perform Hybrid Search across *all* indexed knowledge (emails, pdfs, other pages)
    // We do this if we have a user message (which we do)
    const lastUserMsg = messages.filter(m => m.role === 'user').pop()?.content || ''
    
    let ragContext = ''
    if (lastUserMsg) {
      try {
        ragContext = await ragService.searchWithContext(lastUserMsg, 3)
        if (ragContext) {
          ragContext = '\n\nRELEVANT KNOWLEDGE FROM OTHER DOCUMENTS:\n' + ragContext
        }
      } catch (err) {
        console.warn('[PageService] RAG search failed', err)
      }
    }

    const systemPrompt = (contextText 
      ? AIPrompts.chat.withContext({ content: contextText }) // Context from *current* page
      : AIPrompts.chat.noContext) + ragContext // + Context from *other* docs

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

