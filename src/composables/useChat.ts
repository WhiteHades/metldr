import { ref, nextTick, type Ref, computed } from 'vue'
import type { AppPageSummary, AppChatMessage, AppChatResponse } from '@/types'
import type { AIProviderPreference } from './useSettings'
import { sendToBackground, withTiming } from './useMessaging'
import { logger } from '@/services/LoggerService'
import { analyticsService } from '@/services/AnalyticsService'
import { cacheService } from '@/services/CacheService'

const log = logger.createScoped('Chat')

let saveTimeout: ReturnType<typeof setTimeout> | null = null
const SAVE_DEBOUNCE_MS = 2000

function debouncedSaveChat(url: string, messages: AppChatMessage[]): void {
  if (saveTimeout) clearTimeout(saveTimeout)
  saveTimeout = setTimeout(() => {
    const normalizedUrl = url.split('?')[0].replace(/\/+$/, '')
    cacheService.setTabSession(normalizedUrl, messages, null, false)
      .catch(err => log.warn('auto-save failed', err.message))
  }, SAVE_DEBOUNCE_MS)
}

function flushSaveChat(): void {
  if (saveTimeout) {
    clearTimeout(saveTimeout)
    saveTimeout = null
  }
}

// URL-scoped state - each URL has its own isolated state
interface ChatState {
  messages: AppChatMessage[]
  loading: boolean
  indexing: boolean
  input: string
}

// Map of URL -> ChatState, this is the source of truth
const urlStates = new Map<string, ChatState>()

// current active URL - determines which state the shared refs point to  
let activeUrl: string | null = null

// shared refs that point to current URL's state (for Vue reactivity)
const chatMessages = ref<AppChatMessage[]>([])
const chatInput = ref<string>('')
const chatLoading = ref<boolean>(false)
const chatIndexing = ref<boolean>(false)
const emailContext = ref<{ summary?: string; subject?: string; sender?: string } | null>(null)

// get or create state for a URL
function getUrlState(url: string): ChatState {
  if (!urlStates.has(url)) {
    urlStates.set(url, {
      messages: [],
      loading: false,
      indexing: false,
      input: ''
    })
  }
  return urlStates.get(url)!
}

// sync shared refs TO the URL state (save before switch)
function saveCurrentToUrlState(): void {
  if (activeUrl) {
    const state = getUrlState(activeUrl)
    // only overwrite messages if chatMessages has more (urlStates may have been updated directly during streaming)
    if (chatMessages.value.length >= state.messages.length) {
      state.messages = [...chatMessages.value]
    }
    state.loading = chatLoading.value
    state.indexing = chatIndexing.value
    state.input = chatInput.value
    log.log(`[State] saved to ${activeUrl.slice(0, 40)}: ${state.messages.length} msgs`)
  }
}

// sync shared refs FROM the URL state (load after switch)
function loadFromUrlState(url: string): void {
  const state = getUrlState(url)
  chatMessages.value = [...state.messages]
  chatLoading.value = state.loading
  chatIndexing.value = state.indexing
  chatInput.value = state.input
  activeUrl = url
  log.log(`[State] loaded from ${url.slice(0, 40)}: ${state.messages.length} msgs`)
}

// switch state to a new URL (called on tab change)
function switchToUrl(newUrl: string): void {
  if (newUrl === activeUrl) return
  saveCurrentToUrlState()
  loadFromUrlState(newUrl)
}

// update state for a specific URL (may be different from current)
function updateUrlState(targetUrl: string, updater: (state: ChatState) => void): void {
  const state = getUrlState(targetUrl)
  updater(state)
  
  // if this is the active URL, also update shared refs
  if (targetUrl === activeUrl) {
    chatMessages.value = [...state.messages]
    chatLoading.value = state.loading
    chatIndexing.value = state.indexing
  }
}

// set messages for a URL from external source (IDB load) - syncs with urlStates
function setUrlMessages(url: string, messages: AppChatMessage[]): void {
  const state = getUrlState(url)
  state.messages = [...messages]
  
  // if this is the active URL, also update shared refs
  if (url === activeUrl) {
    chatMessages.value = [...messages]
  }
  log.log(`[State] set ${url.slice(0, 40)}: ${messages.length} msgs from IDB`)
}

// get messages for a URL from in-memory state
function getUrlMessages(url: string): AppChatMessage[] {
  const state = urlStates.get(url)
  return state ? [...state.messages] : []
}

declare const LanguageModel: {
  availability: (opts?: { languages?: string[] }) => Promise<string | { available: string }>
  create: (options?: { 
    initialPrompts?: { role: string; content: string }[]
    temperature?: number
    topK?: number
    expectedInputs?: { type: string; languages: string[] }[]
    expectedOutputs?: { type: string; languages: string[] }[]
  }) => Promise<{
    prompt: (text: string) => Promise<string>
    promptStreaming: (text: string) => AsyncIterable<string>
    destroy: () => void
  }>
} | undefined

export function useChat() {
  // get current active URL
  async function getActiveUrl(): Promise<string> {
    try {
      const tabs = await chrome.tabs.query({ active: true, currentWindow: true })
      return tabs[0]?.url || ''
    } catch {
      return ''
    }
  }

  async function sendChatMessage(
    pageSummary: Ref<AppPageSummary | null>,
    selectedModel: Ref<string>,
    availableModels: Ref<string[]>,
    chatContainer: Ref<HTMLElement | null>,
    chatInputRef: Ref<HTMLElement | null>,
    saveTabSession?: () => Promise<void>,
    preferredProvider?: Ref<AIProviderPreference>
  ): Promise<void> {
    if (!chatInput.value.trim() || chatLoading.value) return
    
    // CRITICAL: capture target URL at message start
    const targetUrl = await getActiveUrl()
    if (!targetUrl) {
      log.warn('no active URL, cannot start chat')
      return
    }
    
    log.log(`[Chat] starting for ${targetUrl.slice(0, 50)}`)
    
    const userMessage = chatInput.value.trim()
    const messageStartTime = performance.now()  // track total time from message send
    chatInput.value = ''
    
    // update target URL's state directly
    updateUrlState(targetUrl, state => {
      state.messages.push({ role: 'user', content: userMessage })
      state.loading = true
      state.input = ''
    })
    
    analyticsService.trackChat('user', userMessage).catch(() => {})
    
    await nextTick()
    if (chatContainer.value) {
      chatContainer.value.scrollTop = chatContainer.value.scrollHeight
    }
    
    if (chatInputRef.value) {
      const el = chatInputRef.value as unknown as { focus?: () => void }
      el.focus?.()
    }
    
    try {
      let contextText = pageSummary.value?.fullContent || ''
      
      if (!contextText) {
        try {
          const tabs = await chrome.tabs.query({ active: true, currentWindow: true })
          const tab = tabs[0]
          if (!tab?.id || !tab.url) throw new Error('no active tab')
          
          if (tab.url.includes('mail.google.com')) {
            log.log('no fullContent, fetching email from content script')
            
            let response: { success: boolean; content?: string; metadata?: { subject?: string; from?: string } } | undefined
            
            try {
              response = await chrome.tabs.sendMessage(tab.id, { type: 'GET_EMAIL_CONTENT' })
            } catch {
              log.log('content script not ready, injecting and retrying')
              await chrome.scripting.executeScript({
                target: { tabId: tab.id },
                files: ['content.js']
              })
              await new Promise(r => setTimeout(r, 500))
              response = await chrome.tabs.sendMessage(tab.id, { type: 'GET_EMAIL_CONTENT' })
            }
            
            if (response?.success && response.content) {
              let emailCtx = ''
              if (response.metadata?.subject) {
                emailCtx += `Subject: ${response.metadata.subject}\n`
              }
              if (response.metadata?.from) {
                emailCtx += `From: ${response.metadata.from}\n`
              }
              emailCtx += `\nEMAIL CONTENT:\n${response.content}`
              contextText = emailCtx
              log.log('fetched email content', contextText.length)
            }
          } else {
            log.log('no fullContent, extracting article from page')
            try {
              const extractResponse = await chrome.runtime.sendMessage({
                type: 'EXTRACT_ONLY',
                tabId: tab.id
              }) as { success: boolean; data?: { title?: string; author?: string; content?: string } } | undefined
              
              if (extractResponse?.success && extractResponse.data?.content) {
                const d = extractResponse.data
                let articleContext = 'ARTICLE METADATA:\n'
                if (d.title) articleContext += `- Title: "${d.title}"\n`
                if (d.author) articleContext += `- Author: ${d.author}\n`
                articleContext += '\n---\nARTICLE CONTENT:\n\n' + d.content
                contextText = articleContext
                log.log('fetched article content', contextText.length)
              } else {
                log.log('EXTRACT_ONLY failed, trying direct extraction')
                const [result] = await chrome.scripting.executeScript({
                  target: { tabId: tab.id! },
                  func: () => document.body.innerText.slice(0, 15000)
                })
                if (result?.result) {
                  contextText = `PAGE CONTENT:\n\n${result.result}`
                  log.log('direct extraction got', `${contextText.length} chars`)
                }
              }
            } catch (extractErr) {
              log.log('extraction error:', (extractErr as Error).message)
            }
          }
        } catch (err) {
          log.log('failed to fetch page content', (err as Error).message)
        }
      }
      
      const currentUrl = targetUrl // use captured URL, not current
      
      const CONTEXT_THRESHOLD = 8000
      let relevantContext = ''
      
      if (contextText && currentUrl) {
        // route through background -> offscreen for persistence (survives panel close)
        const hasIndexRes = await sendToBackground({ type: 'RAG_HAS_INDEXED_CONTENT', sourceUrl: currentUrl }) as { success: boolean; hasContent?: boolean }
        const hasIndex = hasIndexRes?.success && hasIndexRes.hasContent
        
        if (!hasIndex) {
          if (contextText.length > CONTEXT_THRESHOLD) {
            updateUrlState(targetUrl, s => { s.indexing = true })
            log.log(`indexing large doc for chat... ${contextText.length} chars`)
            try {
              await sendToBackground({
                type: 'RAG_INDEX_CHUNKS',
                text: contextText,
                metadata: { sourceId: currentUrl, sourceUrl: currentUrl, sourceType: 'article', title: '' }
              })
            } catch (indexErr) {
              if ((indexErr as Error).message !== 'Indexing aborted') {
                log.warn('indexing failed', (indexErr as Error).message)
              }
            } finally {
              updateUrlState(targetUrl, s => { s.indexing = false })
            }
          } else {
            // fire and forget for small docs - background handles concurrency
            sendToBackground({
              type: 'RAG_INDEX_CHUNKS',
              text: contextText,
              metadata: { sourceId: currentUrl, sourceUrl: currentUrl, sourceType: 'article', title: '' }
            }).catch(err => log.warn('async indexing failed', (err as Error).message))
          }
        }
        
        if (contextText.length > CONTEXT_THRESHOLD) {
          const ragStart = performance.now()
          const searchRes = await sendToBackground({ type: 'RAG_SEARCH_WITH_CONTEXT', query: userMessage, limit: 5, sourceUrl: currentUrl }) as { success: boolean; context?: string; timing?: { embed?: number; search?: number } }
          const ragTime = Math.round(performance.now() - ragStart)
          relevantContext = searchRes?.success ? (searchRes.context || '') : ''
          log.log(`found context from RAG: ${relevantContext.length} chars (${ragTime}ms)`)
          
          if (!relevantContext) {
            log.log('RAG empty for large doc, triggering indexing...')
            updateUrlState(targetUrl, s => { s.indexing = true })
            try {
              await sendToBackground({
                type: 'RAG_INDEX_CHUNKS',
                text: contextText,
                metadata: { sourceId: currentUrl, sourceUrl: currentUrl, sourceType: 'article', title: '' }
              })
              const retryRes = await sendToBackground({ type: 'RAG_SEARCH_WITH_CONTEXT', query: userMessage, limit: 5, sourceUrl: currentUrl }) as { success: boolean; context?: string }
              relevantContext = retryRes?.success ? (retryRes.context || '') : ''
              log.log(`RAG retry after indexing: ${relevantContext.length} chars`)
            } catch (indexErr) {
              log.warn('indexing failed, using truncated content', (indexErr as Error).message)
            } finally {
              updateUrlState(targetUrl, s => { s.indexing = false })
            }
            
            if (!relevantContext) {
              relevantContext = contextText.slice(0, CONTEXT_THRESHOLD)
              log.log('RAG still empty after indexing, using truncated content')
            }
          }
        } else {
          relevantContext = contextText
          log.log(`using full context (${contextText.length} chars < ${CONTEXT_THRESHOLD} threshold)`)
        }
      } else if (contextText) {
        relevantContext = contextText
        log.log(`no URL, using full context (${contextText.length} chars)`)
      }

      const isEmail = contextText.includes('EMAIL CONTENT:')
      const docType = isEmail ? 'email thread' : 'article'
      
      const systemPrompt = relevantContext  
        ? `You are an assistant helping the user understand a ${docType}.

RELEVANT SECTIONS FROM THE DOCUMENT:
${relevantContext}

RULES:
1. Answer based ONLY on the sections above
2. If the info isn't in these sections, say "I don't see that in the document"
3. Be concise and helpful`
        : contextText
          ? `You are an assistant helping the user understand a ${docType}.

DOCUMENT:
${contextText.slice(0, 8000)}

RULES:
1. Answer based on the document
2. Be concise`
          : 'You are a helpful assistant. Be concise.'


      const useOllama = preferredProvider?.value === 'ollama'
      
      let chromeResult: { ok: boolean; content?: string; model?: string; timing?: number } | null = null
      if (!useOllama) {
        try {
          if (typeof LanguageModel !== 'undefined') {
            const langOpts = { languages: ['en', 'es', 'ja'] }
            const avail = await LanguageModel.availability(langOpts)
            const status = typeof avail === 'string' ? avail : avail?.available
            log.log('LanguageModel availability:', { status, raw: avail })
            if (status === 'available' || status === 'downloadable' || status === 'readily') {
              log.log('creating Chrome AI session...')
              
              try {
                const state = getUrlState(targetUrl)
                const recentMessages = state.messages.slice(-6).map(m => ({
                  role: m.role as 'user' | 'assistant',
                  content: m.content
                }))
                
                const initialPrompts: Array<{ role: 'system' | 'user' | 'assistant'; content: string }> = [
                  { role: 'system', content: systemPrompt }
                ]
                
                // add chat history (exclude the current message we just added)
                for (const msg of recentMessages) {
                  if (msg.content !== userMessage) {
                    initialPrompts.push(msg)
                  }
                }
                
                log.log('Chrome AI session with initial prompts:', initialPrompts.length)
                const session = await LanguageModel.create({ initialPrompts })
                log.log('session created')
                  
                const start = performance.now()
                
                let fullResponse = ''
                
                try {
                  const stream = session.promptStreaming(userMessage)
                  
                  for await (const chunk of stream) {
                    fullResponse += chunk
                    
                    updateUrlState(targetUrl, state => {
                      const lastMsg = state.messages[state.messages.length - 1]
                      if (lastMsg?.role === 'assistant') {
                        lastMsg.content = fullResponse
                      } else {
                        state.messages.push({ role: 'assistant', content: fullResponse })
                      }
                    })
                    
                    // debounced auto save to IDB during streaming
                    debouncedSaveChat(targetUrl, getUrlState(targetUrl).messages)
                    
                    await nextTick()
                    if (activeUrl === targetUrl && chatContainer.value) {
                      chatContainer.value.scrollTop = chatContainer.value.scrollHeight
                    }
                  }
                  
                  flushSaveChat()
                  
                  const llmTime = Math.round(performance.now() - start)
                  const totalTime = Math.round(performance.now() - messageStartTime)
                  const ragTime = totalTime - llmTime
                  log.log('streaming complete', { len: fullResponse.length, total: totalTime, llm: llmTime, rag: ragTime })
                  
                  updateUrlState(targetUrl, state => {
                    const lastMsg = state.messages[state.messages.length - 1]
                    if (lastMsg?.role === 'assistant') {
                      lastMsg.timing = { 
                        total: totalTime, 
                        llm: llmTime,
                        rag: ragTime > 100 ? ragTime : undefined,  // only show if significant
                        model: 'gemini-nano' 
                      }
                    }
                  })
                  
                  // save to IDB immediately - prevents lost response if user switched tabs during streaming
                  const completedState = getUrlState(targetUrl)
                  const normalizedUrl = targetUrl.split('?')[0].replace(/\/+$/, '')
                  cacheService.setTabSession(normalizedUrl, completedState.messages, null, false)
                    .catch(err => log.warn('post-stream save failed', err.message))
                  
                  chromeResult = { ok: true, content: fullResponse, model: 'gemini-nano', timing: totalTime }
                  analyticsService.trackChat('assistant', fullResponse, totalTime).catch(() => {})
                } catch (streamErr) {
                  log.log('streaming failed, trying non-streaming...', (streamErr as Error).message)
                  
                  try {
                    const result = await session.prompt(userMessage)
                    const timing = Math.round(performance.now() - start)
                    
                    updateUrlState(targetUrl, state => {
                      state.messages.push({ 
                        role: 'assistant', 
                        content: result,
                        timing: { total: timing, model: 'gemini-nano' }
                      })
                    })
                    
                    chromeResult = { ok: true, content: result, model: 'gemini-nano', timing }
                    log.log('non-streaming complete', { len: result.length, timing })
                  } catch (promptErr) {
                    throw promptErr
                  }
                }
                
                session.destroy()
              } catch (sessionErr) {
                log.log('session/prompt error:', (sessionErr as Error).message)
                throw sessionErr
              }
            } else {
              log.log('LanguageModel not available, status:', status)
            }
          } else {
            log.log('LanguageModel is undefined')
          }
        } catch (err) {
          log.log('Chrome AI failed, falling back', (err as Error).message)
        }
      }

      if (chromeResult?.ok && chromeResult.content) {
        // already handled above
      } else {
        const model = selectedModel.value || availableModels.value[0]
        if (!model) {
          throw new Error('no AI available - Chrome AI is off and Ollama is not running')
        }
        
        const state = getUrlState(targetUrl)
        const recentMessages = state.messages.slice(-6).map(m => ({
          role: m.role as 'user' | 'assistant' | 'system',
          content: m.content
        }))
        
        const messagesWithSystem = [
          { role: 'system' as const, content: systemPrompt },
          ...recentMessages
        ]
        
        log.log('streaming with ollama...', model)
        const start = performance.now()
        
        const { OllamaService } = await import('@/services/OllamaService')
        let fullResponse = ''
        
        try {
          for await (const chunk of OllamaService.completeStream(model, messagesWithSystem)) {
            fullResponse += chunk
            
            updateUrlState(targetUrl, state => {
              const lastMsg = state.messages[state.messages.length - 1]
              if (lastMsg?.role === 'assistant') {
                lastMsg.content = fullResponse
              } else {
                state.messages.push({ role: 'assistant', content: fullResponse })
              }
            })
            
            // debounced auto-save to IDB during streaming
            debouncedSaveChat(targetUrl, getUrlState(targetUrl).messages)
            
            await nextTick()
            if (activeUrl === targetUrl && chatContainer.value) {
              chatContainer.value.scrollTop = chatContainer.value.scrollHeight
            }
          }
          
          flushSaveChat() // cancel pending debounce since we'll do full save at end
          
          const timing = Math.round(performance.now() - start)
          log.log('ollama streaming complete', { len: fullResponse.length, timing })
          
          const llmTime = timing
          const totalTime = Math.round(performance.now() - messageStartTime)
          const ragTime = totalTime - llmTime
          
          updateUrlState(targetUrl, state => {
            const lastMsg = state.messages[state.messages.length - 1]
            if (lastMsg?.role === 'assistant') {
              lastMsg.timing = { 
                total: totalTime, 
                llm: llmTime,
                rag: ragTime > 100 ? ragTime : undefined,
                model 
              }
            }
          })
          
          // save to IDB immediately - prevents lost response if user switched tabs
          const ollCompletedState = getUrlState(targetUrl)
          const ollNormalizedUrl = targetUrl.split('?')[0].replace(/\/+$/, '')
          cacheService.setTabSession(ollNormalizedUrl, ollCompletedState.messages, null, false)
            .catch(err => log.warn('post-stream save failed', err.message))
          
          analyticsService.trackChat('assistant', fullResponse, totalTime).catch(() => {})
        } catch (err) {
          throw err
        }
      }
    } catch (error) {
      log.warn('chat error', (error as Error).message)
      updateUrlState(targetUrl, state => {
        state.messages.push({
          role: 'assistant',
          content: 'sorry, something went wrong. try again.'
        })
      })
    } finally {
      updateUrlState(targetUrl, state => {
        state.loading = false
        state.indexing = false
      })
      
      await nextTick()
      if (activeUrl === targetUrl && chatContainer.value) {
        chatContainer.value.scrollTop = chatContainer.value.scrollHeight
      }
      
      if (saveTabSession) {
        await saveTabSession()
      }
      
      if (chatInputRef.value) {
        const el = chatInputRef.value as unknown as { focus?: () => void }
        el.focus?.()
      }
    }
  }

  function clearChat(chatInputRef?: Ref<HTMLElement | null>, saveTabSession?: () => Promise<void>) {
    if (activeUrl) {
      updateUrlState(activeUrl, state => {
        state.messages = []
      })
    }
    chatMessages.value = []
    if (saveTabSession) saveTabSession()
    nextTick(() => {
      if (chatInputRef?.value) {
        const el = chatInputRef.value as unknown as { focus?: () => void }
        el.focus?.()
      }
    })
  }

  function resetChatState() {
    if (activeUrl) {
      updateUrlState(activeUrl, state => {
        state.messages = []
        state.loading = false
        state.indexing = false
        state.input = ''
      })
    }
    chatMessages.value = []
    chatInput.value = ''
    chatLoading.value = false
    chatIndexing.value = false
    emailContext.value = null
  }

  // sync indexing status from background (call on sidepanel open/reopen)
  async function syncIndexingStatus(): Promise<void> {
    const url = await getActiveUrl()
    if (!url) return
    
    try {
      const res = await sendToBackground({ type: 'RAG_IS_INDEXING', sourceId: url }) as { success: boolean; isIndexing?: boolean }
      if (res?.success && res.isIndexing) {
        log.log(`[Sync] background is indexing ${url.slice(0, 40)}`)
        updateUrlState(url, s => { s.indexing = true })
      }
    } catch (err) {
      log.warn('[Sync] failed to query indexing status', (err as Error).message)
    }
  }

  return {
    // State
    chatMessages,
    chatInput,
    chatLoading,
    chatIndexing,
    emailContext,
    
    // Methods
    sendChatMessage,
    clearChat,
    resetChatState,
    syncIndexingStatus,
    
    // URL-scoped state management
    switchToUrl,
    getActiveUrl,
    saveCurrentToUrlState,
    loadFromUrlState,
    setUrlMessages,
    getUrlMessages
  }
}
