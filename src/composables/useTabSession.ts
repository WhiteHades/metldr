import { type Ref } from 'vue'
import type { AppPageSummary, AppChatMessage } from '@/types'
import { cacheService } from '@/services/CacheService'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('TabSession')

function normalizeUrl(url: string | null): string | null {
  if (!url) return null
  try {
    const u = new URL(url)
    return `${u.origin}${u.pathname}`
  } catch {
    return url
  }
}

export function useTabSession() {
  async function saveTabSession(
    currentTabUrl: Ref<string | null>,
    chatMessages: Ref<AppChatMessage[]>,
    pageSummary: Ref<AppPageSummary | null>,
    summaryCollapsed: Ref<boolean>
  ): Promise<void> {
    const url = normalizeUrl(currentTabUrl.value)
    if (!url) return
    
    if (!pageSummary.value && chatMessages.value.length === 0) {
      log.log('skipping save - no data', url.slice(0, 50))
      return
    }
    
    await cacheService.setTabSession(
      url,
      chatMessages.value,
      pageSummary.value,
      summaryCollapsed.value
    )
    log.log('saved', { url: url.slice(0, 50), msgs: chatMessages.value.length, hasSummary: !!pageSummary.value })
  }

  async function loadTabSession(
    url: string,
    chatMessages: Ref<AppChatMessage[]>,
    pageSummary: Ref<AppPageSummary | null>,
    summaryCollapsed: Ref<boolean>
  ): Promise<boolean> {
    const normalizedUrl = normalizeUrl(url)
    if (!normalizedUrl) return false
    
    const session = await cacheService.getTabSession(normalizedUrl)
    
    if (session) {
      chatMessages.value = Array.isArray(session.chatMessages) ? session.chatMessages : []
      pageSummary.value = session.pageSummary || null
      summaryCollapsed.value = session.summaryCollapsed || false
      log.log('loaded', { url: normalizedUrl.slice(0, 50), msgs: chatMessages.value.length, hasSummary: !!session.pageSummary })
      return true
    }
    
    log.log('no session for', normalizedUrl.slice(0, 50))
    return false
  }

  function setupTabListener(
    currentTabId: Ref<number | null>,
    currentTabUrl: Ref<string | null>,
    chatMessages: Ref<AppChatMessage[]>,
    pageSummary: Ref<AppPageSummary | null>,
    summaryCollapsed: Ref<boolean>,
    aiReady: Ref<boolean>,
    summaryMode: Ref<string>,
    fetchSummary: (force: boolean, trigger: string) => Promise<void>
  ): () => void {
    let urlPollInterval: ReturnType<typeof setInterval> | null = null
    let previousUrl: string | null = null
    
    async function refreshTabUrl(): Promise<void> {
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

    async function handleTabChange(newUrl: string): Promise<void> {
      if (!aiReady.value) return
      
      const normalizedNew = normalizeUrl(newUrl)
      const normalizedPrev = normalizeUrl(previousUrl)
      
      if (normalizedPrev === normalizedNew) return
      
      log.log('tab changed', { from: normalizedPrev?.slice(0, 40), to: normalizedNew?.slice(0, 40) })
      
      if (normalizedPrev) {
        const tempRef = { value: previousUrl } as Ref<string | null>
        await saveTabSession(tempRef, chatMessages, pageSummary, summaryCollapsed)
      }
      
      chatMessages.value = []
      pageSummary.value = null
      summaryCollapsed.value = false
      
      const hasSession = await loadTabSession(newUrl, chatMessages, pageSummary, summaryCollapsed)
      previousUrl = newUrl
      
      if (!hasSession && summaryMode.value === 'auto') {
        await fetchSummary(false, 'auto')
      }
    }
    
    if (urlPollInterval) clearInterval(urlPollInterval)
    urlPollInterval = setInterval(async () => {
      await refreshTabUrl()
      if (currentTabUrl.value && normalizeUrl(currentTabUrl.value) !== normalizeUrl(previousUrl)) {
        await handleTabChange(currentTabUrl.value)
      }
    }, 1500)
    
    refreshTabUrl().then(() => {
      if (currentTabUrl.value) {
        previousUrl = currentTabUrl.value
        loadTabSession(currentTabUrl.value, chatMessages, pageSummary, summaryCollapsed).then(hasSession => {
          if (!hasSession && aiReady.value && summaryMode.value === 'auto') {
            fetchSummary(false, 'auto')
          }
        })
      }
    })
    
    const onActivated = async () => {
      await refreshTabUrl()
      if (currentTabUrl.value && normalizeUrl(currentTabUrl.value) !== normalizeUrl(previousUrl)) {
        await handleTabChange(currentTabUrl.value)
      }
    }
    
    const onUpdated = async (tabId: number, changeInfo: { status?: string; url?: string }, tab: chrome.tabs.Tab) => {
      if (tabId !== currentTabId.value) return
      
      if (tab.url) {
        currentTabUrl.value = tab.url
      }
      
      if (changeInfo.status === 'complete' && tab.url) {
        if (normalizeUrl(tab.url) !== normalizeUrl(previousUrl)) {
          await handleTabChange(tab.url)
        }
      }
    }
    
    chrome.tabs.onActivated.addListener(onActivated)
    chrome.tabs.onUpdated.addListener(onUpdated)
    
    return () => {
      if (urlPollInterval) clearInterval(urlPollInterval)
      chrome.tabs.onActivated.removeListener(onActivated)
      chrome.tabs.onUpdated.removeListener(onUpdated)
    }
  }

  return {
    saveTabSession,
    loadTabSession,
    setupTabListener
  }
}
