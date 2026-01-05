import { ref, computed } from 'vue'
import { sendToBackground, withTiming } from './useMessaging'
import type { AppPageSummary, SummaryPromptData, AppSummaryResponse, ExtractedData } from '@/types'
import { logger } from '@/services/LoggerService'
import { cacheService } from '@/services/CacheService'
import { storageService } from '@/services/StorageService'
import { pdfService } from '@/services/pdf/PdfService'

const log = logger.createScoped('PageSummary')

declare const Summarizer: {
  availability: () => Promise<string>
  create: (opts: {
    type?: string
    length?: string
    format?: string
    expectedInputLanguages?: string[]
    expectedContextLanguages?: string[]
    outputLanguage?: string
  }) => Promise<{
    summarize: (content: string, opts?: { context?: string }) => Promise<string>
    destroy: () => void
  }>
} | undefined

const pageSummary = ref<AppPageSummary | null>(null)
const pageMetadata = ref<{ title: string; url: string } | null>(null)
const summaryLoading = ref<boolean>(false)
const summaryError = ref<string | null>(null)
const currentTabId = ref<number | null>(null)
const currentTabUrl = ref<string | null>(null)
const summaryPrompt = ref<SummaryPromptData | null>(null)
const summaryCollapsed = ref<boolean>(false)

const isEmailClient = computed(() => {
  if (!currentTabUrl.value) return false
  return currentTabUrl.value.includes('mail.google.com') || 
         currentTabUrl.value.includes('outlook.') || 
         currentTabUrl.value.includes('mail.yahoo.com')
})

const isViewingEmailThread = computed(() => {
  if (!isEmailClient.value || !currentTabUrl.value) return false
  try {
    const hash = new URL(currentTabUrl.value).hash
    const hashMatch = hash.match(/#[^/]+\/(?:[^/]+\/)?([A-Za-z0-9_-]{16,})$/)
    return !!hashMatch
  } catch {
    return false
  }
})
// local PDF needing file picker (no fullContent yet)
const isLocalPdfPending = computed(() => {
  if (!currentTabUrl.value) return false
  const url = currentTabUrl.value
  const isPdf = url.toLowerCase().endsWith('.pdf') || url.includes('.pdf?')
  const isLocal = url.startsWith('file://')
  return isPdf && isLocal && !pageSummary.value?.fullContent
})

// get active AI provider and model from storage (same pattern as useSettings)
async function getActiveAIInfo(): Promise<{ provider: 'chrome-ai' | 'ollama', model: string }> {
  try {
    const provider = await storageService.get<'chrome-ai' | 'ollama'>('preferredProvider', 'chrome-ai')
    const selectedModel = await storageService.get<string>('selectedModel', 'llama3.2')
    const model = provider === 'chrome-ai' ? 'gemini-nano' : selectedModel
    return { provider, model }
  } catch {
    return { provider: 'chrome-ai', model: 'gemini-nano' }
  }
}

// system urls where AI features should be disabled
const isSystemUrl = computed(() => {
  const url = currentTabUrl.value?.toLowerCase() || ''
  return url.startsWith('chrome://') || 
         url.startsWith('chrome-extension://') ||
         url.startsWith('edge://') ||
         url.startsWith('about:') ||
         url.startsWith('devtools://') ||
         url === ''
})

const chatDisabled = computed(() => {
  if (isSystemUrl.value) return true
  if (isLocalPdfPending.value) return true
  return isEmailClient.value && !isViewingEmailThread.value
})

const chatDisabledReason = computed(() => {
  if (isSystemUrl.value) return 'system'
  if (isLocalPdfPending.value) return 'local-pdf'
  if (isEmailClient.value && !isViewingEmailThread.value) return 'email'
  return undefined
})

function parseBullets(text: string): string[] {
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

// index content via background -> offscreen for persistence (survives panel close)
async function indexToRag(text: string, metadata: { sourceId: string; sourceUrl: string; sourceType: 'article' | 'email' | 'pdf'; title?: string }) {
  try {
    await sendToBackground({
      type: 'RAG_INDEX_CHUNKS',
      text,
      metadata
    })
    log.log('RAG indexing complete', metadata.sourceId.slice(0, 50))
  } catch (err) {
    const msg = (err as Error).message
    if (msg !== 'Indexing aborted' && msg !== 'Operation cancelled') {
      log.warn('RAG indexing failed', msg)
    }
  }
}

async function trySidePanelSummarize(content: string, context: string): Promise<{ ok: boolean; summary?: string; timing?: number }> {
  try {
    if (typeof Summarizer === 'undefined') return { ok: false }
    
    const avail = await Summarizer.availability()
    if (avail !== 'available' && avail !== 'downloadable') return { ok: false }
    
    log.log('using Chrome AI Summarizer in side panel')
    
    const { result, timing } = await withTiming(async () => {
      const summarizer = await Summarizer.create({
        type: 'key-points',
        length: 'medium',
        format: 'markdown',
        expectedInputLanguages: ['en', 'es', 'ja'],
        expectedContextLanguages: ['en'],
        outputLanguage: 'en'
      })
      try {
        return await summarizer.summarize(content, { context })
      } finally {
        summarizer.destroy()
      }
    })
    
    return { ok: true, summary: result, timing }
  } catch (err) {
    log.log('side panel summarize failed', (err as Error).message)
    return { ok: false }
  }
}

export function usePageSummary() {
  async function fetchCurrentPageSummary(
    force = false, 
    trigger = 'auto',
    saveTabSession?: () => Promise<void>
  ): Promise<void> {
    try {
      summaryPrompt.value = null
      const tabs = await chrome.tabs.query({ active: true, currentWindow: true })
      if (!tabs.length) {
        summaryError.value = 'no active tab'
        return
      }
      
      const tab = tabs[0]
      currentTabId.value = tab.id ?? null
      currentTabUrl.value = tab.url ?? null
      
      if (!tab.url || tab.url.startsWith('chrome://') || tab.url.startsWith('edge://') || tab.url.startsWith('about:')) {
        summaryError.value = 'system page'
        return
      }
      
      if (pageSummary.value && !force) {
        log.log('already have summary, skipping fetch')
        return
      }
      
      // Early PDF stale cache invalidation - check before normal cache path
      const isPdfUrl = tab.url.toLowerCase().endsWith('.pdf') || tab.url.includes('.pdf?')
      if (isPdfUrl && !force) {
        const cached = await cacheService.getPageSummary(tab.url) as AppPageSummary | null
        if (cached) {
          const looksEmpty = cached.bullets?.some(b => 
            b.toLowerCase().includes('empty') || 
            b.toLowerCase().includes('no content') ||
            b.toLowerCase().includes('no main points')
          )
          if (looksEmpty) {
            log.log('PDF cache looks stale (early check), clearing')
            await cacheService.deletePageSummary(tab.url)
            // Also clear the session so it doesn't reload stale data
            pageSummary.value = null
          }
        }
      }
      
      if (!force) {
        const cached = await cacheService.getPageSummary(tab.url) as AppPageSummary | null
        if (cached) {
        log.log('loaded from cache', tab.url.slice(0, 50))
        pageSummary.value = { ...cached, timing: { ...cached.timing, total: cached.timing?.total || 0, cached: true } }
          pageMetadata.value = { title: cached.title || 'untitled', url: tab.url }
          if (saveTabSession) await saveTabSession()
          return
        }
      }
      
      log.log('fetchCurrentPageSummary', { force, trigger, tabId: tab.id, tabUrl: tab.url?.slice(0, 50) })
      
      summaryLoading.value = true
      summaryError.value = null
      
      
      // PDF Detection: If URL ends with .pdf, use PdfService directly
      const isPdf = tab.url.toLowerCase().endsWith('.pdf') || tab.url.includes('.pdf?')
      if (isPdf) {
        try {
          log.log('detected PDF, using PdfService directly')
          
          const startTime = performance.now()
          const pdfSummary = await pdfService.summarize(tab.url)
          const timing = Math.round(performance.now() - startTime)
          
          const urlParts = tab.url.split('/')
          const pdfFilename = decodeURIComponent(urlParts[urlParts.length - 1] || 'PDF Document')
          const bullets = parseBullets(pdfSummary)
          const aiInfo = await getActiveAIInfo()
          const pdfWordCount = pdfSummary.split(/\s+/).length
          const pdfReadTimeMin = Math.max(1, Math.round(pdfWordCount / 200))
          
          const summaryData: AppPageSummary = {
            title: pdfFilename.replace('.pdf', ''),
            author: undefined,
            publishDate: undefined,
            publication: undefined,
            bullets: bullets.length ? bullets : [pdfSummary.slice(0, 200) + '...'],
            readTime: `${pdfReadTimeMin} min`,
            fullContent: pdfSummary,
            wordCount: pdfWordCount,
            timestamp: Date.now(),
            timing: { llm: timing, total: timing, cached: false, model: aiInfo.model, provider: aiInfo.provider }
          }
          
          pageSummary.value = summaryData
          pageMetadata.value = { title: summaryData.title || 'PDF', url: tab.url }
          summaryError.value = null
          
          await cacheService.setPageSummary(tab.url, summaryData, 3600)
          log.log('PDF summary cached', { pages: 'unknown', timing })
          
          indexToRag(pdfSummary, { sourceId: tab.url, sourceUrl: tab.url, sourceType: 'pdf', title: summaryData.title })
          
          if (saveTabSession) await saveTabSession()
          summaryLoading.value = false
          return
        } catch (err) {
          const errMsg = (err as Error).message
          log.error('PdfService failed', errMsg)
          
          // handle local PDF - show file picker button
          if (errMsg === 'LOCAL_PDF_NEEDS_PICKER' || errMsg === 'LOCAL_PDF_NEEDS_PERMISSION' || errMsg === 'ENABLE_FILE_ACCESS') {
            const urlParts = tab.url.split('/')
            const filename = decodeURIComponent(urlParts[urlParts.length - 1] || 'PDF Document')
            pageSummary.value = null // clear stale summary so error state shows
            summaryError.value = 'LOCAL_PDF_CLICK_TO_OPEN'
            pageMetadata.value = { title: filename.replace('.pdf', ''), url: tab.url }
          } else {
            summaryError.value = `PDF processing failed: ${errMsg}`
          }
          summaryLoading.value = false
          return
        }
      }
      
      const extractResponse = await sendToBackground({
        type: 'EXTRACT_ONLY',
        tabId: tab.id
      }) as { success: boolean; data?: ExtractedData; error?: string; prompt?: boolean; skip?: boolean; reason?: string } | null
      
      if (!extractResponse?.success || !extractResponse.data) {
        if (extractResponse?.prompt) {
          summaryPrompt.value = { url: tab.url, reason: extractResponse.reason || 'needs approval' }
          summaryError.value = null
        } else if (extractResponse?.skip) {
          summaryError.value = extractResponse.reason || 'page skipped'
        } else {
          summaryError.value = extractResponse?.error || 'extraction failed'
        }
        summaryLoading.value = false
        return
      }
      
      const extracted = extractResponse.data
      let metadata = `ARTICLE METADATA:\n- Title: "${extracted.title}"\n`
      if (extracted.author) metadata += `- Author: ${extracted.author}\n`
      if (extracted.publication) metadata += `- Publication: ${extracted.publication}\n`
      metadata += '\n---\nARTICLE CONTENT:\n\n'
      const fullContent = metadata + extracted.content
      
      const sidePanelResult = await trySidePanelSummarize(fullContent, `Article: "${extracted.title}"`)
      
      if (sidePanelResult.ok && sidePanelResult.summary) {
        const bullets = parseBullets(sidePanelResult.summary)
        const summaryData: AppPageSummary = {
          title: extracted.title,
          author: extracted.author,
          publishDate: extracted.publishDate,
          publication: extracted.publication,
          bullets: bullets.length ? bullets : ['could not generate summary'],
          readTime: extracted.readTime,
          fullContent,
          wordCount: extracted.wordCount,
          timestamp: Date.now(),
          timing: { llm: sidePanelResult.timing || 0, total: sidePanelResult.timing || 0, cached: false, model: 'gemini-nano', provider: 'chrome-ai' }
        }
        pageSummary.value = summaryData
        pageMetadata.value = { title: extracted.title || 'untitled', url: tab.url }
        summaryError.value = null
        
        await cacheService.setPageSummary(tab.url, summaryData, 3600)
        log.log('cached summary for', tab.url.slice(0, 50))
        
        indexToRag(fullContent, { sourceId: tab.url, sourceUrl: tab.url, sourceType: 'article', title: extracted.title })
        
        if (saveTabSession) await saveTabSession()
        summaryLoading.value = false
        return
      }
      
      log.log('side panel Chrome AI failed, falling back to background')
      const response = await sendToBackground({
        type: 'EXTRACT_AND_SUMMARIZE',
        tabId: tab.id,
        force,
        trigger
      }) as AppSummaryResponse | null
      
      log.log('EXTRACT_AND_SUMMARIZE response', response)
      
      if (!response || !response.success) {
        if (response?.prompt) {
          summaryPrompt.value = { url: tab.url, reason: response.reason || 'needs approval' }
          summaryError.value = null
        } else if (response?.skip) {
          summaryError.value = response.reason || 'page skipped'
        } else {
          summaryError.value = response?.error || 'summarisation failed'
        }
        summaryLoading.value = false
        return
      }
      
      pageSummary.value = response.summary || null
      pageMetadata.value = { title: response.summary?.title || 'untitled', url: tab.url }
      summaryError.value = null
      
      if (response.summary) {
        await cacheService.setPageSummary(tab.url, response.summary, 3600)
        
        if (response.summary.fullContent) {
          indexToRag(response.summary.fullContent, { sourceId: tab.url, sourceUrl: tab.url, sourceType: 'article', title: response.summary.title })
        }
      }
      
      if (saveTabSession) {
        await saveTabSession()
      }
      
    } catch (error) {
      log.error('page summary failed', error)
      summaryError.value = (error as Error).message || 'unknown error'
    } finally {
      summaryLoading.value = false
    }
  }

  function triggerManualSummary(saveTabSession?: () => Promise<void>) {
    fetchCurrentPageSummary(true, 'manual', saveTabSession)
  }

  function acceptSummaryPrompt(saveTabSession?: () => Promise<void>) {
    if (!summaryPrompt.value) return
    summaryPrompt.value = null
    fetchCurrentPageSummary(true, 'manual', saveTabSession)
  }

  function declineSummaryPrompt() {
    summaryError.value = 'summary dismissed'
    summaryPrompt.value = null
  }

  function resetSummaryState() {
    pageSummary.value = null
    pageMetadata.value = null
    summaryCollapsed.value = false
    summaryError.value = null
    summaryPrompt.value = null
  }

  async function refreshCurrentTabUrl(): Promise<void> {
    try {
      const tabs = await chrome.tabs.query({ active: true, currentWindow: true })
      if (tabs.length && tabs[0].url) {
        currentTabUrl.value = tabs[0].url
        currentTabId.value = tabs[0].id ?? null
      }
    } catch {
      // ignore
    }
  }

  // open file picker for local PDFs (requires user gesture/button click)
  async function openLocalPdf(saveTabSession?: () => Promise<void>): Promise<void> {
    try {
      summaryLoading.value = true
      summaryError.value = null
      
      const startTime = performance.now()
      const result = await pdfService.openAndSummarize()
      const totalTime = Math.round(performance.now() - startTime)
      
      const bullets = parseBullets(result.summary)
      const aiInfo = await getActiveAIInfo()
      const wordCount = result.fullText.split(/\s+/).length
      const readTimeMin = Math.max(1, Math.round(wordCount / 200)) // ~200 wpm avg reading speed
      
      const summaryData: AppPageSummary = {
        title: result.filename.replace('.pdf', ''),
        author: undefined,
        publishDate: undefined,
        publication: undefined,
        bullets: bullets.length ? bullets : [result.summary.slice(0, 200) + '...'],
        readTime: `${readTimeMin} min`,
        fullContent: result.fullText,
        wordCount: wordCount,
        timestamp: Date.now(),
        timing: { llm: totalTime, total: totalTime, cached: false, model: aiInfo.model, provider: aiInfo.provider }
      }
      
      pageSummary.value = summaryData
      pageMetadata.value = { title: summaryData.title || 'PDF', url: currentTabUrl.value || '' }
      
      if (currentTabUrl.value) {
        await cacheService.setPageSummary(currentTabUrl.value, summaryData, 3600)
        // index full text to RAG for chat/search
        indexToRag(result.fullText, { sourceId: currentTabUrl.value, sourceUrl: currentTabUrl.value, sourceType: 'pdf', title: summaryData.title })
      }
      
      if (saveTabSession) await saveTabSession()
    } catch (err) {
      if ((err as Error).name === 'AbortError') {
        summaryError.value = 'File selection cancelled'
      } else {
        log.error('openLocalPdf failed', (err as Error).message)
        summaryError.value = `PDF processing failed: ${(err as Error).message}`
      }
    } finally {
      summaryLoading.value = false
    }
  }

  return {
    pageSummary,
    pageMetadata,
    summaryLoading,
    summaryError,
    currentTabId,
    currentTabUrl,
    summaryPrompt,
    summaryCollapsed,
    
    isEmailClient,
    isViewingEmailThread,
    chatDisabled,
    chatDisabledReason,
    
    fetchCurrentPageSummary,
    triggerManualSummary,
    acceptSummaryPrompt,
    declineSummaryPrompt,
    resetSummaryState,
    refreshCurrentTabUrl,
    openLocalPdf
  }
}

