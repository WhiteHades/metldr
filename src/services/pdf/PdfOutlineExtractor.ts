import { getDocumentProxy } from 'unpdf'

export interface PdfTocEntry {
  title: string
  pageNumber: number
  level: number
  children: PdfTocEntry[]
}

export interface PdfMetadata {
  title?: string
  author?: string
  subject?: string
  keywords?: string
  creator?: string
  producer?: string
  creationDate?: Date
  modificationDate?: Date
  pageCount: number
}

export interface PdfAnnotation {
  type: string          // 'Highlight', 'Text', 'FreeText', etc.
  page: number
  content?: string      // comment text
  author?: string
  color?: number[]      // [r, g, b]
  rect?: number[]       // [x1, y1, x2, y2]
  quotedText?: string   // extracted highlighted text
}

export interface PdfStructureNode {
  role: string          // 'H1', 'P', 'Table', etc.
  level: number
  children: PdfStructureNode[]
}

export class PdfOutlineExtractor {
  // extract toc/outline
  async extractOutline(pdfBytes: Uint8Array): Promise<PdfTocEntry[]> {
    try {
      const pdf = await getDocumentProxy(pdfBytes)
      const outline = await pdf.getOutline()
      
      if (!outline || outline.length === 0) return []
      
      return await this.processOutlineItems(pdf, outline, 0)
    } catch (err) {
      console.error('[PdfOutlineExtractor] Failed to extract outline:', err)
      return []
    }
  }

  // extract user annotations (highlights, comments)
  async extractAnnotations(pdfBytes: Uint8Array): Promise<PdfAnnotation[]> {
    try {
      const pdf = await getDocumentProxy(pdfBytes)
      const annotations: PdfAnnotation[] = []
      
      for (let i = 1; i <= pdf.numPages; i++) {
        const page = await pdf.getPage(i)
        const pageAnnots = await page.getAnnotations({ intent: 'display' })
        
        for (const annot of pageAnnots) {
          // only markup annotations (highlights, underlines, comments)
          if (['Highlight', 'Underline', 'StrikeOut', 'Text', 'FreeText'].includes(annot.subtype)) {
            annotations.push({
              type: annot.subtype,
              page: i,
              content: annot.contents || undefined,
              author: annot.title || undefined,
              color: annot.color,
              rect: annot.rect
            })
          }
        }
      }
      
      return annotations
    } catch (err) {
      console.error('[PdfOutlineExtractor] Failed to extract annotations:', err)
      return []
    }
  }

  // extract highlighted text (for priority summarization)
  async extractHighlightedText(pdfBytes: Uint8Array): Promise<string[]> {
    try {
      const pdf = await getDocumentProxy(pdfBytes)
      const highlightedTexts: string[] = []
      
      for (let i = 1; i <= pdf.numPages; i++) {
        const page = await pdf.getPage(i)
        const [annotations, textContent] = await Promise.all([
          page.getAnnotations({ intent: 'display' }),
          page.getTextContent()
        ])
        
        const highlights = annotations.filter((a: any) => 
          a.subtype === 'Highlight' || a.subtype === 'Underline'
        )
        
        if (highlights.length === 0) continue
        
        const viewport = page.getViewport({ scale: 1.0 })
        
        for (const highlight of highlights) {
          const rect = highlight.rect
          
          // find text items within highlight rect
          const matchingText = textContent.items
            .filter((item: any) => {
              const tx = item.transform
              const x = tx[4]
              const y = viewport.height - tx[5]
              return x >= rect[0] && x <= rect[2] && y >= rect[1] && y <= rect[3]
            })
            .map((item: any) => item.str)
            .join(' ')
          
          if (matchingText.trim()) {
            highlightedTexts.push(matchingText.trim())
          }
        }
      }
      
      return highlightedTexts
    } catch (err) {
      console.error('[PdfOutlineExtractor] Failed to extract highlighted text:', err)
      return []
    }
  }

  // extract structure tree (tagged pdf semantics)
  async extractStructureTree(pdfBytes: Uint8Array): Promise<PdfStructureNode | null> {
    try {
      const pdf = await getDocumentProxy(pdfBytes)
      const page = await pdf.getPage(1)
      
      const structTree = await page.getStructTree()
      if (!structTree) return null
      
      return this.parseStructureNode(structTree, 0)
    } catch (err) {
      console.error('[PdfOutlineExtractor] Failed to extract structure tree:', err)
      return null
    }
  }

  // extract headings from structure tree
  async extractHeadings(pdfBytes: Uint8Array): Promise<Array<{ level: number; role: string }>> {
    const structure = await this.extractStructureTree(pdfBytes)
    if (!structure) return []
    
    const headings: Array<{ level: number; role: string }> = []
    
    const traverse = (node: PdfStructureNode) => {
      if (node.role.match(/^H[1-6]$/)) {
        headings.push({
          level: parseInt(node.role[1]),
          role: node.role
        })
      }
      node.children.forEach(traverse)
    }
    
    traverse(structure)
    return headings
  }

  private parseStructureNode(node: any, level: number): PdfStructureNode {
    return {
      role: node.role || 'Document',
      level,
      children: (node.children || [])
        .filter((child: any) => child.role)
        .map((child: any) => this.parseStructureNode(child, level + 1))
    }
  }

  // extract metadata
  async extractMetadata(pdfBytes: Uint8Array): Promise<PdfMetadata> {
    try {
      const { getMeta } = await import('unpdf')
      const pdf = await getDocumentProxy(pdfBytes)
      const { info } = await getMeta(pdf, { parseDates: true })
      
      return {
        title: info?.Title || undefined,
        author: info?.Author || undefined,
        subject: info?.Subject || undefined,
        keywords: info?.Keywords || undefined,
        creator: info?.Creator || undefined,
        producer: info?.Producer || undefined,
        creationDate: info?.CreationDate || undefined,
        modificationDate: info?.ModDate || undefined,
        pageCount: pdf.numPages
      }
    } catch (err) {
      console.error('[PdfOutlineExtractor] Failed to extract metadata:', err)
      return { pageCount: 0 }
    }
  }

  private async processOutlineItems(pdf: any, items: any[], level: number): Promise<PdfTocEntry[]> {
    const entries: PdfTocEntry[] = []
    
    for (const item of items) {
      const pageNumber = await this.resolvePageNumber(pdf, item.dest)
      entries.push({
        title: item.title || 'Untitled',
        pageNumber,
        level,
        children: item.items?.length > 0 
          ? await this.processOutlineItems(pdf, item.items, level + 1) 
          : []
      })
    }
    
    return entries
  }

  private async resolvePageNumber(pdf: any, dest: any): Promise<number> {
    if (!dest) return 1
    
    try {
      let destArray = dest
      if (typeof dest === 'string') destArray = await pdf.getDestination(dest)
      if (!Array.isArray(destArray) || destArray.length === 0) return 1
      
      const pageRef = destArray[0]
      const pageIndex = await pdf.getPageIndex(pageRef)
      return pageIndex + 1
    } catch {
      return 1
    }
  }
}

export const pdfOutlineExtractor = new PdfOutlineExtractor()
