// map-reduce summarization for long content that exceeds LLM context limits
// chunks content, summarizes each chunk, then merges summaries

import { aiGateway } from './AIGateway'
import { chunkingService } from '../rag/ChunkingService'
import { logger } from '../LoggerService'

const log = logger.createScoped('MapReduce')

// config - tuned for gemini nano (~4k token context)
const MAX_DIRECT_CHARS = 12000   // ~3k tokens - skip map-reduce below this
const CHUNK_TARGET_CHARS = 2500  // ~625 tokens per chunk - safe for context
const MAX_SUMMARY_WORDS = 150    // target words per chunk summary

interface MapReduceResult {
  summary: string
  chunkCount: number
  timing: {
    map: number
    reduce: number
    total: number
  }
}

export class MapReduceService {
  
  // check if content needs map-reduce (too long for direct summarization)
  needsMapReduce(content: string): boolean {
    return content.length > MAX_DIRECT_CHARS
  }

  // main entry point - summarize long content using map-reduce
  async summarize(content: string, context?: string): Promise<MapReduceResult> {
    const startTotal = performance.now()
    
    if (!this.needsMapReduce(content)) {
      log.log('content short enough for direct summarization')
      const directResult = await aiGateway.summarize({
        content,
        context,
        type: 'key-points',
        length: 'medium'
      })
      
      return {
        summary: directResult.summary || '',
        chunkCount: 1,
        timing: {
          map: 0,
          reduce: 0,
          total: Math.round(performance.now() - startTotal)
        }
      }
    }

    // split into chunks optimized for context window
    const chunks = this.chunkContent(content)
    log.log(`map-reduce: ${chunks.length} chunks from ${content.length} chars`)

    // map phase - summarize each chunk in parallel (limited concurrency)
    const startMap = performance.now()
    const chunkSummaries = await this.mapPhase(chunks, context)
    const mapTime = Math.round(performance.now() - startMap)
    log.log(`map phase complete: ${chunkSummaries.length} summaries in ${mapTime}ms`)

    // reduce phase - merge chunk summaries into final summary
    const startReduce = performance.now()
    const finalSummary = await this.reducePhase(chunkSummaries, context)
    const reduceTime = Math.round(performance.now() - startReduce)
    log.log(`reduce phase complete in ${reduceTime}ms`)

    return {
      summary: finalSummary,
      chunkCount: chunks.length,
      timing: {
        map: mapTime,
        reduce: reduceTime,
        total: Math.round(performance.now() - startTotal)
      }
    }
  }

  // chunk content into segments optimized for LLM context
  private chunkContent(content: string): string[] {
    const chunks: string[] = []
    const paragraphs = content.split(/\n\s*\n/).filter(p => p.trim().length > 0)
    
    let currentChunk = ''
    for (const para of paragraphs) {
      if (currentChunk.length + para.length > CHUNK_TARGET_CHARS && currentChunk.length > 0) {
        chunks.push(currentChunk.trim())
        currentChunk = para
      } else {
        currentChunk += (currentChunk ? '\n\n' : '') + para
      }
    }
    if (currentChunk.trim()) {
      chunks.push(currentChunk.trim())
    }

    // fallback: if no paragraphs found, chunk by character count
    if (chunks.length === 0 && content.length > 0) {
      for (let i = 0; i < content.length; i += CHUNK_TARGET_CHARS) {
        chunks.push(content.slice(i, i + CHUNK_TARGET_CHARS))
      }
    }

    return chunks
  }

  // map phase - summarize each chunk (parallel with limited concurrency)
  private async mapPhase(chunks: string[], context?: string): Promise<string[]> {
    const concurrency = 2 // limit parallel requests to avoid rate limiting
    const summaries: string[] = []

    for (let i = 0; i < chunks.length; i += concurrency) {
      const batch = chunks.slice(i, i + concurrency)
      const batchPromises = batch.map((chunk, idx) => 
        this.summarizeChunk(chunk, i + idx + 1, chunks.length, context)
      )
      const batchResults = await Promise.all(batchPromises)
      summaries.push(...batchResults)
    }

    return summaries
  }

  // summarize a single chunk
  private async summarizeChunk(chunk: string, index: number, total: number, context?: string): Promise<string> {
    const prompt = `this is section ${index} of ${total} from a longer document.${context ? ` context: ${context}` : ''}

summarize this section's key points in ${MAX_SUMMARY_WORDS} words or less. focus on facts and main ideas only.

SECTION ${index}:
${chunk}

SUMMARY:`

    try {
      const result = await aiGateway.complete({
        systemPrompt: 'you summarize text sections concisely. output only the summary, no preamble.',
        userPrompt: prompt,
        temperature: 0.3
      })

      if (result.ok && result.content) {
        return `[Section ${index}/${total}] ${result.content.trim()}`
      }
      return `[Section ${index}/${total}] (summarization failed)`
    } catch (err) {
      log.warn(`chunk ${index} summarization failed:`, (err as Error).message)
      return `[Section ${index}/${total}] (error)`
    }
  }

  // reduce phase - merge chunk summaries into final coherent summary
  private async reducePhase(chunkSummaries: string[], context?: string): Promise<string> {
    const combined = chunkSummaries.join('\n\n')
    
    // if combined summaries are short enough, do final synthesis
    const prompt = `you have summaries from different sections of a document.${context ? ` the document is about: ${context}` : ''}

create a unified summary with 3-5 bullet points covering the most important information across all sections. be concise and factual.

SECTION SUMMARIES:
${combined}

FINAL SUMMARY (bullet points):`

    try {
      const result = await aiGateway.complete({
        systemPrompt: 'you synthesize multiple summaries into one coherent summary. output bullet points only.',
        userPrompt: prompt,
        temperature: 0.3
      })

      if (result.ok && result.content) {
        return result.content.trim()
      }
      
      // fallback: just return combined summaries
      return combined
    } catch (err) {
      log.warn('reduce phase failed:', (err as Error).message)
      return combined
    }
  }
}

export const mapReduceService = new MapReduceService()
