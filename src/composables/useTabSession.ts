import { ref, type Ref } from 'vue'
import type { AppPageSummary, AppChatMessage } from '@/types'
import { cacheService } from '@/services/CacheService'
import { logger } from '@/services/LoggerService'
import { concurrencyManager } from '@/services/ConcurrencyManager'

const log = logger.createScoped('TabSession')
const gmailThreadPattern = /#[^/]+\/(?:[^/]+\/)?([A-Za-z0-9_-]{16,})$/

type SessionKey = { kind: 'email' | 'page'; value: string }

function sessionKey(url: string): SessionKey {
  try {
    const parsed = new URL(url)
    if (parsed.hostname === 'mail.google.com' && parsed.hash) {
      const threadMatch = parsed.hash.match(gmailThreadPattern)
      if (threadMatch) return { kind: 'email', value: threadMatch[1] }
    }
    // file:// URLs have origin='null' (literal string), handle specially
    if (parsed.protocol === 'file:') {
      return { kind: 'page', value: url.split('#')[0].split('?')[0] }
    }
    return { kind: 'page', value: `${parsed.origin}${parsed.pathname}` }
  } catch {
    return { kind: 'page', value: url }
  }
}

function extractGmailThreadId(url: string | null): string | null {
  if (!url) return null
  const key = sessionKey(url)
  return key.kind === 'email' ? key.value : null
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
    
    const key = sessionKey(url)
    
    if (key.kind === 'email') {
      const emailId = key.value
      await cacheService.setEmailChat(emailId, chatMessages.value)
      log.log('saved gmail chat', { emailId: emailId.slice(0, 20), msgs: chatMessages.value.length })
    } else {
      const normalizedUrl = key.value
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
    const key = sessionKey(url)
    
    if (key.kind === 'email') {
      const emailId = key.value
      const messages = await cacheService.getEmailChat(emailId)
      const msgs = Array.isArray(messages) ? messages : []
      chatMessages.value = msgs
      if (setUrlMessages) setUrlMessages(url, msgs)
      if (msgs.length > 0) {
        log.log('loaded gmail session', { emailId: emailId.slice(0, 20), msgs: msgs.length })
        return true
      }
      log.log('no gmail session for', emailId.slice(0, 20))
      return false
    }
    
    const normalizedUrl = key.value
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
    setUrlMessages?: (url: string, messages: AppChatMessage[]) => void,
    resetSummaryState?: () => void
  ): () => void {
    let urlPollInterval: ReturnType<typeof setInterval> | null = null
    let previousUrl: string | null = null
    const cachedKeys: Array<{ url: string; key: string }> = []
    
    function getCacheKey(url: string | null): string | null {
      if (!url) return null
      const cached = cachedKeys.find(entry => entry.url === url)
      if (cached) return cached.key
      const key = sessionKey(url).value
      cachedKeys.push({ url, key })
      if (cachedKeys.length > 2) cachedKeys.shift()
      return key
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
      if (!aiReady.value || newUrl === previousUrl) return
      
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
      
      // reset ALL summary state (including summaryLoading) for clean slate
      if (resetSummaryState) {
        resetSummaryState()
      } else {
        // legacy fallback if resetSummaryState not provided
        pageSummary.value = null
        summaryCollapsed.value = false
      }
      
      // only clear chat if switchChatUrl wasn't provided (legacy fallback)
      if (!switchChatUrl) {
        chatMessages.value = []
      }
      
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
      if (currentTabUrl.value && currentTabUrl.value !== previousUrl && getCacheKey(currentTabUrl.value) !== getCacheKey(previousUrl)) {
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
