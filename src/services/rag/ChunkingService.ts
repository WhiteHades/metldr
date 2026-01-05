import { split, getChunk } from 'llm-splitter'

// content-type configs based on 2025 industry research
// sources: arXiv, NVIDIA RAG best practices, Milvus
const CHUNK_CONFIGS = {
  email: {
    maxTokens: 200,    // ~800 chars - fact-based retrieval
    overlap: 30,
    maxChars: 800
  },
  article: {
    maxTokens: 300,    // ~1200 chars - balanced context
    overlap: 50,
    maxChars: 1200
  },
  pdf: {
    maxTokens: 400,    // ~1600 chars - broader context
    overlap: 60,
    maxChars: 1600
  }
} as const

export type ContentType = 'email' | 'article' | 'pdf'

export interface ChunkOptions {
  contentType?: ContentType
  chunkSize?: number
  chunkOverlap?: number
}

export interface ChunkResult {
  text: string
  index: number
  tokenCount: number
  start: number
  end: number
}

function estimateTokens(text: string): number {
  if (!text) return 0
  return Math.ceil(text.length / 4)
}

function sanitizeText(text: string): string {
  if (!text || typeof text !== 'string') return ''
  return text
    .replace(/\r\n/g, '\n')
    .replace(/\t/g, ' ')
    .replace(/ +/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

export async function chunk(rawText: string, options: ChunkOptions = {}): Promise<ChunkResult[]> {
  const text = sanitizeText(rawText)
  if (!text || text.length < 50) {
    if (text.length > 0) {
      return [{
        text,
        index: 0,
        tokenCount: estimateTokens(text),
        start: 0,
        end: text.length
      }]
    }
    return []
  }
  
  const contentType = options.contentType || 'article'
  const config = CHUNK_CONFIGS[contentType]
  
  const maxChars = options.chunkSize || config.maxChars
  const overlapChars = options.chunkOverlap || Math.floor(config.overlap * 4)
  
  if (overlapChars >= maxChars) {
    throw new Error('Overlap must be less than chunk size')
  }
  
  try {
    const chunks = split(text, {
      chunkSize: maxChars,
      chunkOverlap: overlapChars
    })
    
    const results: ChunkResult[] = chunks.map((c, i) => {
      const chunkText = typeof c.text === 'string' ? c.text : (c.text?.join('\n\n') || '')
      return {
        text: chunkText,
        index: i,
        tokenCount: estimateTokens(chunkText),
        start: c.start,
        end: c.end
      }
    }).filter(c => c.text.length >= 50)
    
    // stats
    const avgChars = results.reduce((sum, c) => sum + c.text.length, 0) / results.length
    const avgTokens = results.reduce((sum, c) => sum + c.tokenCount, 0) / results.length
    
    console.log(
      `[ChunkingService] ${text.length} chars → ${results.length} chunks ` +
      `(avg: ${Math.round(avgChars)} chars, ${Math.round(avgTokens)} tokens) ` +
      `type: ${contentType}`
    )
    
    // warn if any chunk exceeds 1.5x target
    results.forEach((chunk, i) => {
      if (chunk.tokenCount > config.maxTokens * 1.5) {
        console.warn(`[ChunkingService] Chunk ${i} large: ${chunk.tokenCount} tokens`)
      }
    })
    
    return results
  } catch (err) {
    console.error('[ChunkingService] Failed:', err)
    return fallbackChunk(text, maxChars)
  }
}

function fallbackChunk(text: string, targetChars: number): ChunkResult[] {
  const sentences = text.split(/(?<=[.!?])\s+/).filter(s => s.length > 0)
  const results: ChunkResult[] = []
  let current = ''
  let start = 0
  
  for (const sentence of sentences) {
    if (current.length + sentence.length > targetChars && current.length > 0) {
      results.push({
        text: current.trim(),
        index: results.length,
        tokenCount: estimateTokens(current),
        start,
        end: start + current.length
      })
      start += current.length
      current = ''
    }
    current += (current ? ' ' : '') + sentence
  }
  
  if (current.trim()) {
    results.push({
      text: current.trim(),
      index: results.length,
      tokenCount: estimateTokens(current),
      start,
      end: start + current.length
    })
  }
  
  return results
}

export async function chunkForEmbedding(text: string, contentType: ContentType = 'article'): Promise<ChunkResult[]> {
  return chunk(text, { contentType })
}

export function countWords(text: string): number {
  return text.split(/\s+/).filter(w => w.length > 0).length
}

export async function countTokens(texts: string[]): Promise<number[]> {
  return texts.map(estimateTokens)
}

export { getChunk }

export const chunkingService = {
  chunk,
  chunkForEmbedding,
  countWords,
  countTokens,
  getChunk,
  estimateTokens,
  sanitizeText
}
