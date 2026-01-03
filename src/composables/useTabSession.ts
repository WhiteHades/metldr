import { type Ref } from 'vue'
import type { AppPageSummary, AppChatMessage } from '@/types'
import { cacheService } from '@/services/CacheService'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('TabSession')

function extractGmailThreadId(url: string | null): string | null {
  if (!url) return null
  try {
    const u = new URL(url)
    if (u.hostname === 'mail.google.com' && u.hash) {
      const threadMatch = u.hash.match(/#[^/]+\/(?:[^/]+\/)?([A-Za-z0-9_-]{16,})$/)
      if (threadMatch) return threadMatch[1]
    }
    return null
  } catch {
    return null
  }
}

function normalizeUrl(url: string | null): string | null {
  if (!url) return null
  try {
    const u = new URL(url)
    return `${u.origin}${u.pathname}`
  } catch {
    return url
  }
}

function isGmail(url: string | null): boolean {
  if (!url) return false
  try {
    return new URL(url).hostname === 'mail.google.com'
  } catch {
    return false
  }
}

export function useTabSession() {
  async function saveTabSession(
    currentTabUrl: Ref<string | null>,
    chatMessages: Ref<AppChatMessage[]>,
    pageSummary: Ref<AppPageSummary | null>,
    summaryCollapsed: Ref<boolean>
  ): Promise<void> {
    const url = currentTabUrl.value
    if (!url) return
    
    if (!pageSummary.value && chatMessages.value.length === 0) {
      log.log('skipping save - no data')
      return
    }
    
    const emailId = extractGmailThreadId(url)
    
    if (emailId) {
      await cacheService.setEmailChat(emailId, chatMessages.value)
      log.log('saved gmail chat', { emailId: emailId.slice(0, 20), msgs: chatMessages.value.length })
    } else {
      const normalizedUrl = normalizeUrl(url)
      if (!normalizedUrl) return
      await cacheService.setTabSession(normalizedUrl, chatMessages.value, pageSummary.value, summaryCollapsed.value)
      log.log('saved tab session', { url: normalizedUrl.slice(0, 50), msgs: chatMessages.value.length })
    }
  }

  async function loadTabSession(
    url: string,
    chatMessages: Ref<AppChatMessage[]>,
    pageSummary: Ref<AppPageSummary | null>,
    summaryCollapsed: Ref<boolean>
  ): Promise<boolean> {
    const emailId = extractGmailThreadId(url)
    
    if (emailId) {
      const session = await cacheService.getEmailSession(emailId)
      if (session) {
        chatMessages.value = Array.isArray(session.chatMessages) ? session.chatMessages : []
        log.log('loaded gmail session', { emailId: emailId.slice(0, 20), msgs: chatMessages.value.length })
        return chatMessages.value.length > 0
      }
      log.log('no gmail session for', emailId.slice(0, 20))
      return false
    }
    
    const normalizedUrl = normalizeUrl(url)
    if (!normalizedUrl) return false
    
    const session = await cacheService.getTabSession(normalizedUrl)
    if (session) {
      // Check if this is a stale PDF session with empty/broken content
      const isPdf = url.toLowerCase().endsWith('.pdf') || url.includes('.pdf?')
      if (isPdf && session.pageSummary?.bullets) {
        const looksEmpty = session.pageSummary.bullets.some((b: string) => 
          b.toLowerCase().includes('empty') || 
          b.toLowerCase().includes('no content') ||
          b.toLowerCase().includes('no main points')
        )
        if (looksEmpty) {
          log.log('stale PDF session detected, clearing for re-fetch')
          // Don't load the stale session - return false so auto-fetch triggers
          chatMessages.value = []
          pageSummary.value = null
          summaryCollapsed.value = false
          return false
        }
      }
      
      chatMessages.value = Array.isArray(session.chatMessages) ? session.chatMessages : []
      pageSummary.value = session.pageSummary || null
      summaryCollapsed.value = session.summaryCollapsed || false
      log.log('loaded tab session', { url: normalizedUrl.slice(0, 50), msgs: chatMessages.value.length })
      return true
    }
    
    log.log('no tab session for', normalizedUrl.slice(0, 50))
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
    
    function getCacheKey(url: string | null): string | null {
      if (!url) return null
      const emailId = extractGmailThreadId(url)
      return emailId || normalizeUrl(url)
    }
    
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
      
      const newKey = getCacheKey(newUrl)
      const prevKey = getCacheKey(previousUrl)
      
      if (prevKey === newKey) return
      
      log.log('tab changed', { from: prevKey?.slice(0, 30), to: newKey?.slice(0, 30) })
      
      if (previousUrl) {
        const prevChatMessages = [...chatMessages.value]
        const prevPageSummary = pageSummary.value
        const prevSummaryCollapsed = summaryCollapsed.value
        const tempRef = { value: previousUrl } as Ref<string | null>
        const tempChat = { value: prevChatMessages } as Ref<AppChatMessage[]>
        const tempSummary = { value: prevPageSummary } as Ref<AppPageSummary | null>
        const tempCollapsed = { value: prevSummaryCollapsed } as Ref<boolean>
        saveTabSession(tempRef, tempChat, tempSummary, tempCollapsed)
      }
      
      chatMessages.value = []
      pageSummary.value = null
      summaryCollapsed.value = false
      
      previousUrl = newUrl
      const hasSession = await loadTabSession(newUrl, chatMessages, pageSummary, summaryCollapsed)
      
      // Always auto-fetch for PDFs, otherwise respect summaryMode
      const isPdf = newUrl.toLowerCase().endsWith('.pdf') || newUrl.includes('.pdf?')
      if (!hasSession && (summaryMode.value === 'auto' || isPdf)) {
        fetchSummary(false, 'auto')
      }
    }
    
    if (urlPollInterval) clearInterval(urlPollInterval)
    urlPollInterval = setInterval(async () => {
      await refreshTabUrl()
      if (currentTabUrl.value && getCacheKey(currentTabUrl.value) !== getCacheKey(previousUrl)) {
        await handleTabChange(currentTabUrl.value)
      }
    }, 500)
    
    refreshTabUrl().then(() => {
      if (currentTabUrl.value) {
        previousUrl = currentTabUrl.value
        const isPdf = currentTabUrl.value.toLowerCase().endsWith('.pdf') || currentTabUrl.value.includes('.pdf?')
        loadTabSession(currentTabUrl.value, chatMessages, pageSummary, summaryCollapsed).then(hasSession => {
          // Always auto-fetch for PDFs, otherwise respect summaryMode
          if (!hasSession && aiReady.value && (summaryMode.value === 'auto' || isPdf)) {
            fetchSummary(false, 'auto')
          }
        })
      }
    })
    
    const onActivated = async () => {
      await refreshTabUrl()
      if (currentTabUrl.value && getCacheKey(currentTabUrl.value) !== getCacheKey(previousUrl)) {
        await handleTabChange(currentTabUrl.value)
      }
    }
    
    const onUpdated = async (tabId: number, changeInfo: { status?: string; url?: string }, tab: chrome.tabs.Tab) => {
      if (tabId !== currentTabId.value) return
      
      if (tab.url) {
        currentTabUrl.value = tab.url
      }
      
      if (changeInfo.status === 'complete' && tab.url) {
        if (getCacheKey(tab.url) !== getCacheKey(previousUrl)) {
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
    setupTabListener,
    extractGmailThreadId,
    isGmail
  }
}
