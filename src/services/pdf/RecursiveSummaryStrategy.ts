import { aiGateway } from '../ai/AIGateway'
import type { PdfChunk } from '../../types'

export class RecursiveSummaryStrategy {
  private readonly CHUNK_SIZE = 12000 // approx 3-4k tokens
  private readonly OVERLAP = 500

  async execute(fullText: string): Promise<string> {
    console.log(`[RecursiveSummary] Starting for text length: ${fullText.length}`)

    const chunks = this.chunkText(fullText)
    console.log(`[RecursiveSummary] Created ${chunks.length} chunks`)

    if (chunks.length === 1) {
      const res = await aiGateway.summarize({ content: chunks[0].text, type: 'key-points' })
      return res.summary || ''
    }

    const chunkSummaries: string[] = []
    const batchSize = 2
    
    for (let i = 0; i < chunks.length; i += batchSize) {
      const batch = chunks.slice(i, i + batchSize)
      const results = await Promise.all(
        batch.map(async (chunk, idx) => {
          try {
            const res = await aiGateway.summarize({ 
              content: `Context: Part ${i + idx + 1} of a larger document.\n\n${chunk.text}`, 
              type: 'key-points' 
            })
            return res.ok && res.summary ? res.summary : `[Failed to summarize chunk ${i + idx}]`
          } catch (e) {
            return `[Error summarizing chunk: ${(e as Error).message}]`
          }
        })
      )
      chunkSummaries.push(...results)
    }

    const combinedSummary = chunkSummaries.join('\n\n')
    console.log(`[RecursiveSummary] Reducing ${chunkSummaries.length} summaries...`)
    
    const finalRes = await aiGateway.summarize({ 
      content: `Here is a collection of summaries from sections of a document. Combine them into one coherent main summary:\n\n${combinedSummary}`, 
      type: 'key-points' // 'markdown' wasn't in allowed types? checking types again, type is key-points|tldr|teaser|headline.
    })
    
    return finalRes.summary || 'Failed to generate final summary.'
  }

  private chunkText(text: string): PdfChunk[] {
    const chunks: PdfChunk[] = []
    let start = 0
    
    while (start < text.length) {
      const end = Math.min(start + this.CHUNK_SIZE, text.length)
      
      let cleanEnd = end
      if (end < text.length) {
        const lastPeriod = text.lastIndexOf('.', end)
        const lastNewline = text.lastIndexOf('\n', end)
        cleanEnd = Math.max(lastPeriod, lastNewline)
        if (cleanEnd < start + this.CHUNK_SIZE / 2) {
          cleanEnd = end
        }
      }

      chunks.push({ 
        text: text.slice(start, cleanEnd), 
        pageStart: 0, 
        pageEnd: 0, 
        tokenCountEstimate: (cleanEnd - start) / 4 
      })
      
      start = cleanEnd - this.OVERLAP
      if (start < cleanEnd) start = cleanEnd
    }
    
    return chunks
  }
}
