import type { PageInfo, ExtractedContent, PreSummarizeData, PreSummarizeCallback } from '../types'

export class PageMonitor {
  private pageType = 'unknown'
  private confidence = 0
  private dwellThreshold: number
  private dwellTimer = 0
  private dwellInterval: ReturnType<typeof setInterval> | null = null
  private urlObserver: ReturnType<typeof setInterval> | null = null
  private currentPageUrl: string
  private summarisationQueued = false
  private onPreSummarize: PreSummarizeCallback | null = null

  constructor(dwellThreshold = 15) {
    this.dwellThreshold = dwellThreshold
    this.currentPageUrl = window.location.href
  }

  setPreSummarizeCallback(callback: PreSummarizeCallback): void {
    this.onPreSummarize = callback
  }

  detectPageType(): PageInfo {
    const url = window.location.href
    const hostname = window.location.hostname
    const doc = document

    const domainChecks = [
      { pattern: /mail\.google\.com/, type: 'email', confidence: 1.0 },
      { pattern: /(docs?\.| documentation|developer\.|api\.|guide\.)/, type: 'documentation', confidence: 0.9 },
      { pattern: /(reddit\.com|news\.ycombinator\.com|stackoverflow\.com)/, type: 'forum', confidence: 0.95 },
      { pattern: /(arxiv\.org|\.edu|scholar\.google)/, type: 'academic', confidence: 0.9 },
      { pattern: /(medium\.com|substack\.com|dev\.to|hashnode)/, type: 'article', confidence: 0.85 },
      { pattern: /github\.com.*\/(readme|blob|tree)/, type: 'documentation', confidence: 0.8 },
      { pattern: /wikipedia\.org/, type: 'encyclopedia', confidence: 1.0 }
    ]

    for (const check of domainChecks) {
      if (check.pattern.test(hostname) || check.pattern.test(url)) {
        this.pageType = check.type
        this.confidence = check.confidence
        return { type: this.pageType, confidence: this.confidence, metadata: { url, hostname } }
      }
    }

    const h1 = doc.querySelector('h1')
    const article = doc.querySelector('article')
    const main = doc.querySelector('main')

    if (h1 || article || main) {
      this.pageType = 'article'
      this.confidence = 0.6
    } else {
      this.pageType = 'unknown'
      this.confidence = 0.2
    }

    return {
      type: this.pageType,
      confidence: this.confidence,
      metadata: { url, hostname }
    }
  }

  isReadable(): boolean {
    const main = document.querySelector('main') || 
                 document.querySelector('article') ||
                 document.querySelector('.post-content') ||
                 document.querySelector('[role="main"]')
    
    const minWords = 100
    const textContent = (main || document.body).textContent || ''
    const wordCount = textContent.split(/\s+/).length
    
    return wordCount >= minWords
  }

  extractContent(): ExtractedContent {
    const content: ExtractedContent = {
      content: '',
      sections: [],
      headers: []
    }

    const mainContent = document.querySelector('main') ||
                       document.querySelector('article') ||
                       document.querySelector('.post-content') ||
                       document.querySelector('[role="main"]') ||
                       document.body

    if (mainContent) {
      const text = mainContent.textContent?.trim() || ''
      content.content = text
    }

    const headers = document.querySelectorAll('h1, h2, h3')
    headers.forEach(h => {
      const headerText = h.textContent?.trim()
      if (headerText) content.headers.push(headerText)
    })

    const sections = document.querySelectorAll('section, article, .section, [role="region"]')
    sections.forEach(s => {
      const sectionText = s.textContent?.trim() || ''
      if (sectionText.length > 50) {
        content.sections.push(sectionText.substring(0, 500))
      }
    })

    console.log(`page content manager: extracted ${content.content.length} chars, ${content.headers.length} headers, ${content.sections.length} sections`)
    return content
  }

  startDwellMonitoring(): void {
    console.log('page content manager: starting dwell-time monitoring')

    this.urlObserver = setInterval(() => {
      if (window.location.href !== this.currentPageUrl) {
        console.log('page content manager: url changed, resetting dwell timer')
        this.dwellTimer = 0
        this.summarisationQueued = false
        this.currentPageUrl = window.location.href
      }
    }, 1000)

    this.dwellInterval = setInterval(() => {
      if (!document.hidden && document.hasFocus()) {
        this.dwellTimer++

        if (this.dwellTimer === this.dwellThreshold && !this.summarisationQueued) {
          console.log('page content manager: dwell threshold reached, queueing pre-summarization')
          this.queuePreSummarization()
        }
      }
    }, 1000)

    window.addEventListener('beforeunload', () => {
      this.dwellTimer = 0
      this.summarisationQueued = false
    })

    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        console.log('page content manager: tab hidden, pausing dwell timer')
      } else {
        console.log('page content manager: tab visible, resuming dwell timer')
      }
    })
  }

  async queuePreSummarization(): Promise<void> {
    this.summarisationQueued = true

    const pageInfo = this.detectPageType()
    console.log('page content manager: page type detected:', pageInfo.type, 'confidence:', pageInfo.confidence)

    if (!this.isReadable()) {
      console.log('page content manager: page not readable, skipping pre-summarization')
      return
    }

    const extracted = this.extractContent()

    if (!extracted.content || extracted.content.length < 200) {
      console.log('page content manager: insufficient content, skipping')
      return
    }

    console.log('page content manager: triggering pre-summarization callback')

    if (this.onPreSummarize) {
      this.onPreSummarize({
        priority: 'low',
        url: window.location.href,
        pageType: pageInfo.type,
        metadata: pageInfo.metadata,
        content: extracted.content.slice(0, 5000),
        sections: extracted.sections
      })
    }
  }

  stopDwellMonitoring(): void {
    if (this.dwellInterval) clearInterval(this.dwellInterval)
    if (this.urlObserver) clearInterval(this.urlObserver)
  }
}

export const pageMonitor = new PageMonitor()
