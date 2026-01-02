import * as pdfjsLib from 'pdfjs-dist'
import { RecursiveSummaryStrategy } from './RecursiveSummaryStrategy'
import { ragService } from '../rag/RagService'

pdfjsLib.GlobalWorkerOptions.workerSrc = chrome.runtime.getURL('assets/pdf.worker.min.js')

export interface StreamingPdfOptions {
  onPageText?: (pageNum: number, text: string) => void
  onChunkReady?: (text: string, chunkIndex: number) => void
}

export class PdfService {
  private strategy = new RecursiveSummaryStrategy()
  private readonly WORDS_PER_CHUNK = 350
  private readonly WORD_OVERLAP = 50

  async summarize(url: string): Promise<string> {
    try {
      console.log('[PdfService] Fetching PDF...')
      
      // file:// URLs cannot be fetched due to CSP - must use file picker
      // (isAllowedFileSchemeAccess only controls content script injection, not fetch)
      if (url.startsWith('file://')) {
        console.log('[PdfService] file:// URL detected, file picker required')
        throw new Error('LOCAL_PDF_NEEDS_PICKER')
      }
      
      const loadingTask = pdfjsLib.getDocument(url)
      const pdf = await loadingTask.promise
      
      console.log(`[PdfService] PDF loaded. Pages: ${pdf.numPages}`)
      const fullText = await this.extractText(pdf)
      
      console.log(`[PdfService] Text extracted. Length: ${fullText.length}`)
      const summary = await this.strategy.execute(fullText)
      
      return summary
    } catch (err) {
      console.error('[PdfService] Failed to summarize PDF:', err)
      throw err
    }
  }

  // summarize from ArrayBuffer (used with file picker or drag-drop)
  // returns both summary and full extracted text (for RAG/chat)
  async summarizeFromArrayBuffer(data: ArrayBuffer, filename?: string): Promise<{ summary: string; fullText: string }> {
    try {
      console.log('[PdfService] Processing PDF from ArrayBuffer...')
      
      const loadingTask = pdfjsLib.getDocument({ data })
      const pdf = await loadingTask.promise
      
      console.log(`[PdfService] PDF loaded. Pages: ${pdf.numPages}`)
      const fullText = await this.extractText(pdf)
      
      console.log(`[PdfService] Text extracted. Length: ${fullText.length}`)
      const summary = await this.strategy.execute(fullText)
      
      return { summary, fullText }
    } catch (err) {
      console.error('[PdfService] Failed to summarize PDF from ArrayBuffer:', err)
      throw err
    }
  }

  // extract text only (for RAG indexing before/without summarizing)
  async extractFromArrayBuffer(data: ArrayBuffer): Promise<string> {
    try {
      console.log('[PdfService] Extracting text from PDF...')
      
      const loadingTask = pdfjsLib.getDocument({ data })
      const pdf = await loadingTask.promise
      
      console.log(`[PdfService] PDF loaded. Pages: ${pdf.numPages}`)
      const fullText = await this.extractText(pdf)
      
      console.log(`[PdfService] Text extracted. Length: ${fullText.length}`)
      return fullText
    } catch (err) {
      console.error('[PdfService] Failed to extract PDF text:', err)
      throw err
    }
  }

  // open file picker and summarize selected PDF (using hidden input for better compatibility)
  openAndSummarize(): Promise<{ summary: string; fullText: string; filename: string }> {
    return new Promise((resolve, reject) => {
      console.log('[PdfService] Opening file input...')
      
      // create hidden file input
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
      
      // trigger file picker
      input.click()
    })
  }

  async *streamPages(url: string): AsyncGenerator<{ pageNum: number; text: string }> {
    const loadingTask = pdfjsLib.getDocument(url)
    const pdf = await loadingTask.promise
    
    for (let i = 1; i <= pdf.numPages; i++) {
      const page = await pdf.getPage(i)
      const content = await page.getTextContent()
      const text = content.items.map((item: any) => item.str).join(' ')
      yield { pageNum: i, text }
    }
  }

  async *streamChunks(url: string): AsyncGenerator<{ text: string; chunkIndex: number; pageStart: number }> {
    let buffer = ''
    let chunkIndex = 0
    let currentPageStart = 1

    for await (const { pageNum, text } of this.streamPages(url)) {
      buffer += text + ' '
      
      while (this.countWords(buffer) >= this.WORDS_PER_CHUNK) {
        const { chunk, remainder } = this.extractChunk(buffer, this.WORDS_PER_CHUNK)
        yield { text: chunk, chunkIndex, pageStart: currentPageStart }
        chunkIndex++
        buffer = this.getOverlap(chunk, this.WORD_OVERLAP) + remainder
        currentPageStart = pageNum
      }
    }

    if (buffer.trim()) {
      yield { text: buffer.trim(), chunkIndex, pageStart: currentPageStart }
    }
  }

  async extractAndIndex(url: string, title?: string): Promise<{ fullText: string; chunksIndexed: number }> {
    let fullText = ''
    let chunksIndexed = 0

    console.log('[PdfService] Streaming PDF chunks...')
    
    for await (const { text, chunkIndex } of this.streamChunks(url)) {
      fullText += text + '\n\n'
      
      // index chunk immediately (don't wait for full doc)
      ragService.indexChunks(text, {
        sourceId: url,
        sourceUrl: url,
        sourceType: 'pdf',
        title: title || 'PDF Document'
      }).catch(err => console.warn('[PdfService] Chunk indexing failed:', err))
      
      chunksIndexed = chunkIndex + 1
      console.log(`[PdfService] Streamed chunk ${chunkIndex + 1}`)
    }

    console.log(`[PdfService] Streaming complete. ${chunksIndexed} chunks indexed.`)
    return { fullText, chunksIndexed }
  }

  private async extractText(pdf: pdfjsLib.PDFDocumentProxy): Promise<string> {
    let fullText = ''
    for (let i = 1; i <= pdf.numPages; i++) {
      const page = await pdf.getPage(i)
      const content = await page.getTextContent()
      const strings = content.items.map((item: any) => item.str)
      fullText += strings.join(' ') + '\n\n'
    }
    return fullText
  }

  private countWords(text: string): number {
    return text.trim().split(/\s+/).filter(w => w.length > 0).length
  }

  private extractChunk(text: string, targetWords: number): { chunk: string; remainder: string } {
    const words = text.split(/\s+/)
    const chunkWords = words.slice(0, targetWords)
    const remainderWords = words.slice(targetWords)
    return {
      chunk: chunkWords.join(' '),
      remainder: remainderWords.join(' ')
    }
  }

  private getOverlap(text: string, overlapWords: number): string {
    const words = text.split(/\s+/)
    return words.slice(-overlapWords).join(' ') + ' '
  }
}

export const pdfService = new PdfService()
