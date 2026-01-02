import { localModels } from '../ai/LocalModelProvider'

const DEFAULT_TARGET_TOKENS = 1500
const DEFAULT_OVERLAP_TOKENS = 200
const MAX_TOKENS = 8000

export interface ChunkOptions {
  targetTokens?: number
  overlapTokens?: number
}

export interface ChunkResult {
  text: string
  index: number
  tokenCount: number
}

async function countTokens(texts: string[]): Promise<number[]> {
  return localModels.tokenize(texts)
}

// fast word-based estimate for initial chunking (avoid round-trip for every paragraph)
function estimateTokens(text: string): number {
  // ~1.3 tokens per word for English text
  const words = text.split(/\s+/).filter(w => w.length > 0).length
  return Math.ceil(words * 1.3)
}

function splitIntoParagraphs(text: string): string[] {
  return text.split(/\n\s*\n/).filter(p => p.trim().length > 0)
}

export async function chunk(text: string, options: ChunkOptions = {}): Promise<ChunkResult[]> {
  const targetTokens = options.targetTokens || DEFAULT_TARGET_TOKENS
  const overlapTokens = options.overlapTokens || DEFAULT_OVERLAP_TOKENS
  
  if (!text || text.trim().length === 0) return []
  
  const paragraphs = splitIntoParagraphs(text)
  const chunks: ChunkResult[] = []
  let currentParagraphs: string[] = []
  let currentEstimate = 0
  
  for (const para of paragraphs) {
    const paraEstimate = estimateTokens(para)
    
    if (currentEstimate + paraEstimate > targetTokens && currentParagraphs.length > 0) {
      const chunkText = currentParagraphs.join('\n\n')
      chunks.push({
        text: chunkText,
        index: chunks.length,
        tokenCount: currentEstimate
      })
      
      // overlap: keep last paragraph(s) up to overlapTokens
      const overlapParas: string[] = []
      let overlapCount = 0
      for (let i = currentParagraphs.length - 1; i >= 0 && overlapCount < overlapTokens; i--) {
        overlapParas.unshift(currentParagraphs[i])
        overlapCount += estimateTokens(currentParagraphs[i])
      }
      currentParagraphs = overlapParas
      currentEstimate = overlapCount
    }
    
    currentParagraphs.push(para)
    currentEstimate += paraEstimate
  }
  
  if (currentParagraphs.length > 0) {
    chunks.push({
      text: currentParagraphs.join('\n\n'),
      index: chunks.length,
      tokenCount: currentEstimate
    })
  }
  
  // refine token counts with WASM tokenizer (batch call)
  if (chunks.length > 0) {
    try {
      const exactCounts = await countTokens(chunks.map(c => c.text))
      chunks.forEach((c, i) => { c.tokenCount = exactCounts[i] })
    } catch (err) {
      console.warn('[ChunkingService] WASM tokenization failed, using estimates:', err)
    }
  }
  
  return chunks
}

export async function chunkForEmbedding(text: string): Promise<ChunkResult[]> {
  return chunk(text, { targetTokens: 1500, overlapTokens: 200 })
}

// sync version for simple word count (keyword index still uses this)
export function countWords(text: string): number {
  return text.split(/\s+/).filter(w => w.length > 0).length
}

export const chunkingService = {
  chunk,
  chunkForEmbedding,
  countWords,
  countTokens
}
