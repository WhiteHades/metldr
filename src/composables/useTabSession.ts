import { ref, type Ref } from 'vue'
import type { AppPageSummary, AppChatMessage } from '@/types'
import { cacheService } from '@/services/CacheService'
import { logger } from '@/services/LoggerService'
import { concurrencyManager } from '@/services/ConcurrencyManager'

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
    // file:// URLs have origin='null' (literal string), handle specially
    if (u.protocol === 'file:') {
      return url.split('#')[0].split('?')[0] // strip hash and query, keep full path
    }
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
    summaryCollapsed: Ref<boolean>,
    setUrlMessages?: (url: string, messages: AppChatMessage[]) => void
  ): Promise<boolean> {
    const emailId = extractGmailThreadId(url)
    
    if (emailId) {
      const session = await cacheService.getEmailSession(emailId)
      if (session) {
        const msgs = Array.isArray(session.chatMessages) ? session.chatMessages : []
        chatMessages.value = msgs
        if (setUrlMessages) setUrlMessages(url, msgs)
        log.log('loaded gmail session', { emailId: emailId.slice(0, 20), msgs: msgs.length })
        return msgs.length > 0
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
      
      const msgs = Array.isArray(session.chatMessages) ? session.chatMessages : []
      chatMessages.value = msgs
      pageSummary.value = session.pageSummary || null
      summaryCollapsed.value = session.summaryCollapsed || false
      
      // sync with useChat's urlStates
      if (setUrlMessages) setUrlMessages(url, msgs)
      
      log.log('loaded tab session', { url: normalizedUrl.slice(0, 50), msgs: msgs.length })
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
    fetchSummary: (force: boolean, trigger: string) => Promise<void>,
    switchChatUrl?: (url: string) => void,
    setUrlMessages?: (url: string, messages: AppChatMessage[]) => void
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
        concurrencyManager.abortForUrl(previousUrl)
        
        const prevChatMessages = [...chatMessages.value]
        const prevPageSummary = pageSummary.value
        const prevSummaryCollapsed = summaryCollapsed.value
        const tempRef = { value: previousUrl } as Ref<string | null>
        const tempChat = { value: prevChatMessages } as Ref<AppChatMessage[]>
        const tempSummary = { value: prevPageSummary } as Ref<AppPageSummary | null>
        const tempCollapsed = { value: prevSummaryCollapsed } as Ref<boolean>
        saveTabSession(tempRef, tempChat, tempSummary, tempCollapsed)
      }
      
      // CRITICAL: switch chat state to new URL's isolated state
      if (switchChatUrl) {
        switchChatUrl(newUrl)
      }
      
      // only clear if switchChatUrl wasn't provided (legacy fallback)
      if (!switchChatUrl) {
        chatMessages.value = []
      }
      pageSummary.value = null
      summaryCollapsed.value = false
      
      previousUrl = newUrl
      
      if (switchChatUrl) {
        if (chatMessages.value.length === 0) {
          await loadTabSession(newUrl, chatMessages, pageSummary, summaryCollapsed, setUrlMessages)
          log.log('loaded chat from IDB (in-memory was empty)')
        } else {
          const dummyChatRef = ref<AppChatMessage[]>([])
          await loadTabSession(newUrl, dummyChatRef, pageSummary, summaryCollapsed, setUrlMessages)
        }
      } else {
        await loadTabSession(newUrl, chatMessages, pageSummary, summaryCollapsed, setUrlMessages)
      }
      
      // Always auto-fetch for PDFs, otherwise respect summaryMode
      const isPdf = newUrl.toLowerCase().endsWith('.pdf') || newUrl.includes('.pdf?')
      if ((summaryMode.value === 'auto' || isPdf) && !pageSummary.value) {
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
        
        if (switchChatUrl) {
          switchChatUrl(currentTabUrl.value)
        }
        
        const isPdf = currentTabUrl.value.toLowerCase().endsWith('.pdf') || currentTabUrl.value.includes('.pdf?')
        loadTabSession(currentTabUrl.value, chatMessages, pageSummary, summaryCollapsed, setUrlMessages).then(hasSession => {
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
