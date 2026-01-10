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
      // broadcast progress to side panel
      const broadcastProgress = (percent: number) => {
        chrome.runtime.sendMessage({
          type: 'INDEXING_PROGRESS',
          sourceId: url,
          percent
        }).catch(() => {})
      }
      
      ragService.indexChunks(content, {
        sourceId: url,
        sourceUrl: url,
        sourceType: 'article',
        title: title || 'Untitled Article'
      }, broadcastProgress).catch(err => console.warn('[PageService] Chunk indexing failed', err))
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

    console.log('[PageService] summarizing, length:', fullContent.length)

    let summaryText: string
    let usedMapReduce = false

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

    if (url) {
      await cacheService.setPageSummary(url, summary, 3600)
      // index summary for global search
      ragService.indexSummary(
        bullets.join('\n'),
        { sourceId: url, sourceUrl: url, sourceType: 'article', title: title || url }
      ).catch(() => {})
    }

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
    
    const bullets = lines
      .filter(l => /^[-•*]|^\d+\./.test(l))
      .map(l => l.replace(/^[-•*]\s*/, '').replace(/^\d+\.\s*/, '').trim())
      .filter(l => l.length > 10)
      .slice(0, 5)

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

  static async globalChat(
    messages: ChatMessage[]
  ): Promise<ChatResult & { 
    sources?: Array<{ index: number; title: string; url: string; type: 'email' | 'page' | 'pdf'; score: number; snippet: string }>
  }> {
    const startTime = performance.now()
    const lastUserMsg = messages.filter(m => m.role === 'user').pop()?.content || ''
    
    if (!lastUserMsg) {
      return { ok: false, error: 'no user message' }
    }
    
    const { context, sources } = await ragService.searchWithSources(lastUserMsg, 5)
    console.log('[PageService.globalChat] sources from RAG:', sources?.length, sources)
    
    if (!context) {
      return { 
        ok: true, 
        content: "I don't have any indexed content to search. Try summarizing some emails, articles, or PDFs first.",
        sources: [],
        timing: { total: Math.round(performance.now() - startTime), model: 'none' }
      }
    }
    
    const systemPrompt = `You are metldr, a helpful assistant with access to the user's saved emails, articles, and documents.

SOURCES:
${context}

RULES:
- Give a SHORT, concise answer (2-4 sentences per source max)
- Only cite ONCE at the end of relevant info using [1], [2], etc. - NOT on every line
- Focus on the most important details (dates, amounts, names, IDs)
- If multiple sources, summarize each briefly
- If info not found, say so briefly
- Use plain text, minimal formatting`

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
        sources,
        timing: { 
          total: Math.round(performance.now() - startTime), 
          model: isChrome ? 'gemini-nano' : 'ollama'
        }
      }
    }
    
    // fallback to ollama
    const selectedModel = await OllamaService.selectBest('page_summary')
    if (!selectedModel) {
      return { ok: false, error: result.error || 'no ai available', sources }
    }
    
    try {
      const ollamaResult = await OllamaService.complete(
        selectedModel,
        [{ role: 'system', content: systemPrompt }, ...messages.slice(-6)],
        { temperature: 0.2 }
      )
      
      return {
        ...ollamaResult,
        sources,
        timing: { total: Math.round(performance.now() - startTime), model: selectedModel }
      }
    } catch (err) {
      console.error('[PageService.globalChat]', (err as Error).message)
      return { ok: false, error: (err as Error).message, sources }
    }
  }
}

