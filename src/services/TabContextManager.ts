import { ref, type Ref } from 'vue'
import type { AppPageSummary, AppChatMessage } from '@/types'

interface TabState {
  tabId: number
  url: string
  chatMessages: AppChatMessage[]
  pageSummary: AppPageSummary | null
  summaryCollapsed: boolean
  isIndexing: boolean
  isChatting: boolean
  isSummarizing: boolean
  fullContent: string
  abortController: AbortController | null
  timestamp: number
}

class TabContextManager {
  private contexts = new Map<number, TabState>()
  private currentTabId: number | null = null

  getOrCreate(tabId: number, url: string): TabState {
    let ctx = this.contexts.get(tabId)
    
    if (ctx && ctx.url !== url) {
      this.cleanup(tabId)
      ctx = undefined
    }

    if (!ctx) {
      ctx = {
        tabId,
        url,
        chatMessages: [],
        pageSummary: null,
        summaryCollapsed: false,
        isIndexing: false,
        isChatting: false,
        isSummarizing: false,
        fullContent: '',
        abortController: null,
        timestamp: Date.now()
      }
      this.contexts.set(tabId, ctx)
      console.log(`[TabContext] Created context for tab ${tabId}: ${url.slice(0, 50)}`)
    }

    return ctx
  }

  get(tabId: number): TabState | undefined {
    return this.contexts.get(tabId)
  }

  setCurrent(tabId: number): void {
    this.currentTabId = tabId
  }

  getCurrent(): TabState | undefined {
    if (this.currentTabId === null) return undefined
    return this.contexts.get(this.currentTabId)
  }

  update(tabId: number, updates: Partial<TabState>): void {
    const ctx = this.contexts.get(tabId)
    if (ctx) {
      Object.assign(ctx, updates, { timestamp: Date.now() })
    }
  }

  abort(tabId: number): void {
    const ctx = this.contexts.get(tabId)
    if (ctx?.abortController) {
      ctx.abortController.abort()
      ctx.abortController = null
    }
  }

  cleanup(tabId: number): void {
    const ctx = this.contexts.get(tabId)
    if (ctx) {
      this.abort(tabId)
      this.contexts.delete(tabId)
      console.log(`[TabContext] Cleaned up context for tab ${tabId}`)
    }
  }

  cleanupStale(maxAgeMs = 30 * 60 * 1000): void {
    const now = Date.now()
    for (const [tabId, ctx] of this.contexts) {
      if (now - ctx.timestamp > maxAgeMs && !ctx.isIndexing && !ctx.isChatting && !ctx.isSummarizing) {
        this.cleanup(tabId)
      }
    }
  }

  getActiveOperations(): { indexing: number; chatting: number; summarizing: number } {
    let indexing = 0, chatting = 0, summarizing = 0
    for (const ctx of this.contexts.values()) {
      if (ctx.isIndexing) indexing++
      if (ctx.isChatting) chatting++
      if (ctx.isSummarizing) summarizing++
    }
    return { indexing, chatting, summarizing }
  }

  getAllContexts(): TabState[] {
    return Array.from(this.contexts.values())
  }
}

export const tabContextManager = new TabContextManager()
