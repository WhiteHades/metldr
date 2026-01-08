import { extractText, getDocumentProxy, getMeta, extractLinks } from 'unpdf'
import { RecursiveSummaryStrategy } from './RecursiveSummaryStrategy'
import { pdfOutlineExtractor, type PdfTocEntry, type PdfMetadata } from './PdfOutlineExtractor'
import { ragService } from '../rag/RagService'

export interface PdfPageText {
  pageNum: number
  text: string
}

export interface PdfSummaryBullet {
  text: string
  pageRef?: { start: number; end: number }
}

export interface EnhancedPdfSummary {
  bullets: PdfSummaryBullet[]
  metadata: PdfMetadata
  toc: PdfTocEntry[]
  links: string[]
  timing: { total: number; model: string }
}

export interface StreamingPdfOptions {
  onPageText?: (pageNum: number, text: string) => void
  onChunkReady?: (text: string, chunkIndex: number) => void
}

export class PdfService {
  private strategy = new RecursiveSummaryStrategy()
  private readonly WORDS_PER_CHUNK = 350
  private readonly WORD_OVERLAP = 50

  // basic summarize (backwards compatible)
  async summarize(url: string): Promise<string> {
    try {
      console.log('[PdfService] Fetching PDF...')
      
      if (url.startsWith('file://')) {
        console.log('[PdfService] file:// URL detected, file picker required')
        throw new Error('LOCAL_PDF_NEEDS_PICKER')
      }
      
      const response = await fetch(url)
      const buffer = await response.arrayBuffer()
      const pdfBytes = new Uint8Array(buffer)
      
      const pdf = await getDocumentProxy(pdfBytes)
      console.log(`[PdfService] PDF loaded. Pages: ${pdf.numPages}`)
      
      const { text } = await extractText(pdf, { mergePages: true })
      console.log(`[PdfService] Text extracted. Length: ${text.length}`)
      
      const summary = await this.strategy.execute(text)
      
      // index summary for global search
      ragService.indexSummary(summary, {
        sourceId: url,
        sourceUrl: url,
        sourceType: 'pdf',
        title: url.split('/').pop() || 'PDF Document'
      }).catch(err => console.warn('[PdfService] Summary indexing failed:', err))
      
      return summary
    } catch (err) {
      console.error('[PdfService] Failed to summarize PDF:', err)
      throw err
    }
  }

  // enhanced summarize with metadata, TOC, and page-aware bullets
  async summarizeEnhanced(url: string): Promise<EnhancedPdfSummary> {
    const startTime = Date.now()
    
    try {
      console.log('[PdfService] Enhanced summarization...')
      
      if (url.startsWith('file://')) {
        throw new Error('LOCAL_PDF_NEEDS_PICKER')
      }
      
      const response = await fetch(url)
      const buffer = await response.arrayBuffer()
      const pdfBytes = new Uint8Array(buffer)
      
      // parallel extraction
      const [pdf, metadata, toc, highlightedText] = await Promise.all([
        getDocumentProxy(pdfBytes),
        pdfOutlineExtractor.extractMetadata(pdfBytes),
        pdfOutlineExtractor.extractOutline(pdfBytes),
        pdfOutlineExtractor.extractHighlightedText(pdfBytes)
      ])
      
      // extract text per page for page-aware summaries
      const { text: pageTexts, totalPages } = await extractText(pdf, { mergePages: false })
      
      // extract links
      const { links } = await extractLinks(pdf)
      
      console.log(`[PdfService] Enhanced extraction. Pages: ${totalPages}, TOC: ${toc.length}, Highlights: ${highlightedText.length}`)
      
      // if user highlighted text, prioritize it in summary
      let bullets: PdfSummaryBullet[]
      
      if (highlightedText.length > 0) {
        bullets = await this.summarizeWithHighlights(pageTexts, highlightedText)
      } else if (toc.length > 0) {
        bullets = await this.summarizeWithToc(pageTexts, toc)
      } else {
        bullets = await this.summarizeWithPageRefs(pageTexts)
      }
      
      return {
        bullets,
        metadata,
        toc,
        links,
        timing: { total: Date.now() - startTime, model: 'pdf-enhanced' }
      }
    } catch (err) {
      console.error('[PdfService] Enhanced summarization failed:', err)
      throw err
    }
  }

  // summarize from ArrayBuffer (file picker/drag-drop)
  async summarizeFromArrayBuffer(data: ArrayBuffer, filename?: string, sourceUrl?: string): Promise<{ summary: string; fullText: string }> {
    try {
      console.log('[PdfService] Processing PDF from ArrayBuffer...')
      
      const pdfBytes = new Uint8Array(data)
      const pdf = await getDocumentProxy(pdfBytes)
      
      console.log(`[PdfService] PDF loaded. Pages: ${pdf.numPages}`)
      
      const { text } = await extractText(pdf, { mergePages: true })
      console.log(`[PdfService] Text extracted. Length: ${text.length}`)
      
      const summary = await this.strategy.execute(text)
      
      // index summary for global search
      const pdfUrl = sourceUrl || `file://${filename || 'document.pdf'}`
      ragService.indexSummary(summary, {
        sourceId: pdfUrl,
        sourceUrl: pdfUrl,
        sourceType: 'pdf',
        title: filename || 'PDF Document'
      }).catch(err => console.warn('[PdfService] Summary indexing failed:', err))
      
      return { summary, fullText: text }
    } catch (err) {
      console.error('[PdfService] Failed to summarize PDF from ArrayBuffer:', err)
      throw err
    }
  }

  // extract text from URL
  async extractFromUrl(url: string): Promise<string> {
    try {
      console.log('[PdfService] Extracting text from URL...')
      
      if (url.startsWith('file://')) {
        throw new Error('LOCAL_PDF_NEEDS_PICKER')
      }
      
      const response = await fetch(url)
      const buffer = await response.arrayBuffer()
      const pdfBytes = new Uint8Array(buffer)
      
      const pdf = await getDocumentProxy(pdfBytes)
      const { text } = await extractText(pdf, { mergePages: true })
      
      console.log(`[PdfService] Text extracted. Length: ${text.length}`)
      return text
    } catch (err) {
      console.error('[PdfService] Failed to extract PDF from URL:', err)
      throw err
    }
  }

  // extract text from ArrayBuffer
  async extractFromArrayBuffer(data: ArrayBuffer): Promise<string> {
    try {
      console.log('[PdfService] Extracting text from ArrayBuffer...')
      
      const pdfBytes = new Uint8Array(data)
      const pdf = await getDocumentProxy(pdfBytes)
      
      const { text } = await extractText(pdf, { mergePages: true })
      console.log(`[PdfService] Text extracted. Length: ${text.length}`)
      
      return text
    } catch (err) {
      console.error('[PdfService] Failed to extract PDF text:', err)
      throw err
    }
  }

  // extract per-page text
  async extractPages(pdfBytes: Uint8Array): Promise<PdfPageText[]> {
    const pdf = await getDocumentProxy(pdfBytes)
    const { text: pageTexts } = await extractText(pdf, { mergePages: false })
    
    return pageTexts.map((text, i) => ({
      pageNum: i + 1,
      text
    }))
  }

  // section-based summarization using toc
  private async summarizeWithToc(pageTexts: string[], toc: PdfTocEntry[]): Promise<PdfSummaryBullet[]> {
    const bullets: PdfSummaryBullet[] = []
    const flatToc = this.flattenToc(toc)
    
    for (let i = 0; i < flatToc.length; i++) {
      const entry = flatToc[i]
      const nextEntry = flatToc[i + 1]
      
      const startPage = entry.pageNumber
      const endPage = nextEntry ? nextEntry.pageNumber - 1 : pageTexts.length
      
      // extract section text
      const sectionText = pageTexts.slice(startPage - 1, endPage).join('\n')
      
      if (sectionText.trim().length > 100) {
        // summarize this section
        const sectionSummary = await this.strategy.execute(sectionText, { maxBullets: 2 })
        
        bullets.push({
          text: `**${entry.title}**: ${sectionSummary.replace(/^[-•*]\s*/gm, '').trim()}`,
          pageRef: { start: startPage, end: endPage }
        })
      }
    }
    
    return bullets.slice(0, 8) // limit to 8 bullets
  }

  private async summarizeWithHighlights(pageTexts: string[], highlights: string[]): Promise<PdfSummaryBullet[]> {
    const highlightSection = highlights.length > 0 
      ? `Key Highlighted Content:\n${highlights.join('\n')}\n\n`
      : ''
    
    const fullText = pageTexts.join('\n\n')
    const contextText = highlightSection + fullText
    
    const summary = await this.strategy.execute(contextText)
    
    const lines = summary.split('\n').filter(l => l.trim())
    const bullets: PdfSummaryBullet[] = []
    
    for (const line of lines) {
      const cleanLine = line.replace(/^[-•*]\s*/, '').trim()
      if (!cleanLine) continue
      
      const pageRef = this.findPageRef(cleanLine, pageTexts)
      
      bullets.push({
        text: cleanLine,
        pageRef
      })
    }
    
    return bullets.slice(0, 8)
  }

  // page-aware summarization without toc
  private async summarizeWithPageRefs(pageTexts: string[]): Promise<PdfSummaryBullet[]> {
    const fullText = pageTexts.join('\n\n')
    const summary = await this.strategy.execute(fullText)
    
    // parse bullets from summary
    const lines = summary.split('\n').filter(l => l.trim())
    const bullets: PdfSummaryBullet[] = []
    
    for (const line of lines) {
      const cleanLine = line.replace(/^[-•*]\s*/, '').trim()
      if (!cleanLine) continue
      
      // try to find which pages contain this content
      const pageRef = this.findPageRef(cleanLine, pageTexts)
      
      bullets.push({
        text: cleanLine,
        pageRef
      })
    }
    
    return bullets.slice(0, 8)
  }

  // find page range where content appears
  private findPageRef(content: string, pageTexts: string[]): { start: number; end: number } | undefined {
    const keywords = content.toLowerCase().split(/\s+/).filter(w => w.length > 4).slice(0, 5)
    
    if (keywords.length === 0) return undefined
    
    let bestMatch = { start: 1, end: 1, score: 0 }
    
    for (let i = 0; i < pageTexts.length; i++) {
      const pageText = pageTexts[i].toLowerCase()
      const matchCount = keywords.filter(kw => pageText.includes(kw)).length
      
      if (matchCount > bestMatch.score) {
        bestMatch = { start: i + 1, end: i + 1, score: matchCount }
      }
    }
    
    return bestMatch.score >= 2 ? { start: bestMatch.start, end: bestMatch.end } : undefined
  }

  // flatten hierarchical toc
  private flattenToc(toc: PdfTocEntry[]): PdfTocEntry[] {
    const flat: PdfTocEntry[] = []
    
    for (const entry of toc) {
      flat.push(entry)
      if (entry.children.length > 0) {
        flat.push(...this.flattenToc(entry.children))
      }
    }
    
    return flat.sort((a, b) => a.pageNumber - b.pageNumber)
  }

  // file picker summarize
  openAndSummarize(): Promise<{ summary: string; fullText: string; filename: string }> {
    return new Promise((resolve, reject) => {
      console.log('[PdfService] Opening file input...')
      
      const input = document.createElement('input')
      input.type = 'file'
      input.accept = '.pdf,application/pdf'
      input.style.display = 'none'
      document.body.appendChild(input)
      
      input.onchange = async () => {
        const file = input.files?.[0]
        document.body.removeChild(input)
        
        if (!file) {
          reject(new Error('No file selected'))
          return
        }
        
        console.log('[PdfService] Selected file:', file.name)
        
        try {
          const arrayBuffer = await file.arrayBuffer()
          const result = await this.summarizeFromArrayBuffer(arrayBuffer, file.name)
          resolve({ summary: result.summary, fullText: result.fullText, filename: file.name })
        } catch (err) {
          reject(err)
        }
      }
      
      input.oncancel = () => {
        document.body.removeChild(input)
        const err = new Error('File selection cancelled')
        err.name = 'AbortError'
        reject(err)
      }
      
      input.click()
    })
  }

  // rag indexing
  async extractAndIndex(url: string, title?: string): Promise<{ fullText: string; chunksIndexed: number }> {
    let fullText = ''
    let chunksIndexed = 0

    console.log('[PdfService] Extracting and indexing PDF...')
    
    const text = await this.extractFromUrl(url)
    const words = text.split(/\s+/)
    
    // chunk the text
    for (let i = 0; i < words.length; i += this.WORDS_PER_CHUNK - this.WORD_OVERLAP) {
      const chunkWords = words.slice(i, i + this.WORDS_PER_CHUNK)
      const chunkText = chunkWords.join(' ')
      
      fullText += chunkText + '\n\n'
      
      // broadcast progress to side panel
      const broadcastProgress = (percent: number) => {
        chrome.runtime.sendMessage({
          type: 'INDEXING_PROGRESS',
          sourceId: url,
          percent
        }).catch(() => {})
      }
      
      ragService.indexChunks(chunkText, {
        sourceId: url,
        sourceUrl: url,
        sourceType: 'pdf',
        title: title || 'PDF Document'
      }, broadcastProgress).catch(err => console.warn('[PdfService] Chunk indexing failed:', err))
      
      chunksIndexed++
    }

    console.log(`[PdfService] Indexing complete. ${chunksIndexed} chunks indexed.`)
    return { fullText, chunksIndexed }
  }

  private countWords(text: string): number {
    return text.trim().split(/\s+/).filter(w => w.length > 0).length
  }
}

export const pdfService = new PdfService()
