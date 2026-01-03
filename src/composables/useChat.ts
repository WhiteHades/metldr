import { ref, nextTick, type Ref } from 'vue'
import type { AppPageSummary, AppChatMessage, AppChatResponse } from '@/types'
import type { AIProviderPreference } from './useSettings'
import { sendToBackground, withTiming } from './useMessaging'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('Chat')

const chatMessages = ref<AppChatMessage[]>([])
const chatInput = ref<string>('')
const chatLoading = ref<boolean>(false)
const chatIndexing = ref<boolean>(false)
const emailContext = ref<{ summary?: string; subject?: string; sender?: string } | null>(null)

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
    
    const userMessage = chatInput.value.trim()
    chatInput.value = ''
    
    if (!Array.isArray(chatMessages.value)) {
      chatMessages.value = []
    }
    
    chatMessages.value.push({ role: 'user', content: userMessage })
    
    await nextTick()
    if (chatContainer.value) {
      chatContainer.value.scrollTop = chatContainer.value.scrollHeight
    }
    
    if (chatInputRef.value) {
      const el = chatInputRef.value as unknown as { focus?: () => void }
      el.focus?.()
    }
    
    chatLoading.value = true
    
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
              let emailContext = ''
              if (response.metadata?.subject) {
                emailContext += `Subject: ${response.metadata.subject}\n`
              }
              if (response.metadata?.from) {
                emailContext += `From: ${response.metadata.from}\n`
              }
              emailContext += `\nEMAIL CONTENT:\n${response.content}`
              contextText = emailContext
              log.log('fetched email content', contextText.length)
            }
          } else {
            log.log('no fullContent, extracting article from page')
            try {
              const extractResponse = await chrome.runtime.sendMessage({
                type: 'EXTRACT_ONLY',
                tabId: tab.id
              }) as { success: boolean; data?: { title?: string; author?: string; content?: string } } | undefined
              
              log.log('EXTRACT_ONLY response:', { success: extractResponse?.success, hasContent: !!extractResponse?.data?.content })
              
              if (extractResponse?.success && extractResponse.data?.content) {
                const d = extractResponse.data
                let articleContext = 'ARTICLE METADATA:\n'
                if (d.title) articleContext += `- Title: "${d.title}"\n`
                if (d.author) articleContext += `- Author: ${d.author}\n`
                articleContext += '\n---\nARTICLE CONTENT:\n\n' + d.content
                contextText = articleContext
                log.log('fetched article content', contextText.length)
              } else {
                // fallback: try to get innerText directly from content script
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
      
      // get current page URL for scoped search
      let currentUrl = ''
      try {
        const tabs = await chrome.tabs.query({ active: true, currentWindow: true })
        currentUrl = tabs[0]?.url || ''
      } catch {}
      
      // smart context: use full content if small, RAG search if large
      const CONTEXT_THRESHOLD = 8000
      let relevantContext = ''
      
      // always index for cross-content search (fire-and-forget for small docs)
      if (contextText && currentUrl) {
        const { ragService } = await import('@/services/rag/RagService')
        const hasIndex = await ragService.hasIndexedContent(currentUrl)
        
        if (!hasIndex) {
          if (contextText.length > CONTEXT_THRESHOLD) {
            // large doc: must wait for indexing before search
            chatIndexing.value = true
            log.log(`indexing large doc for chat... ${contextText.length} chars`)
            try {
              await ragService.indexChunks(contextText, {
                sourceId: currentUrl,
                sourceUrl: currentUrl,
                sourceType: 'article',
                title: ''
              })
            } catch (indexErr) {
              log.warn('indexing failed', (indexErr as Error).message)
            } finally {
              chatIndexing.value = false
            }
          } else {
            // small doc: index async for cross-content search, don't block chat
            ragService.indexChunks(contextText, {
              sourceId: currentUrl,
              sourceUrl: currentUrl,
              sourceType: 'article',
              title: ''
            }).catch(err => log.warn('async indexing failed', (err as Error).message))
          }
        }
        
        // use RAG search for large docs, full content for small
        if (contextText.length > CONTEXT_THRESHOLD) {
          relevantContext = await ragService.searchWithContext(userMessage, 5, currentUrl)
          log.log(`found context from RAG: ${relevantContext.length} chars`)
          if (!relevantContext) {
            relevantContext = contextText.slice(0, CONTEXT_THRESHOLD)
            log.log('RAG empty, using truncated content')
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
      
      // build system prompt with context
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
      
      // try chrome ai directly in side panel (side panel has window = page context)
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
              // create session with NO options (simplest form - per Chrome AI docs)
              const session = await LanguageModel.create()
              log.log('session created')
                
              const start = performance.now()
              // combine system prompt + user message
              const fullPrompt = `${systemPrompt}\n\nUser question: ${userMessage}`
              
              let sessionSuccess = false
              let msgIndex = -1
              
              try {
                // try streaming first for real-time response
                const stream = session.promptStreaming(fullPrompt)
                let fullResponse = ''
                  
                for await (const chunk of stream) {
                  fullResponse += chunk
                  // add message on first chunk (no placeholder)
                  if (msgIndex === -1) {
                    msgIndex = chatMessages.value.length
                    chatMessages.value.push({ role: 'assistant', content: fullResponse })
                  } else {
                    chatMessages.value[msgIndex].content = fullResponse
                  }
                  await nextTick()
                  if (chatContainer.value) {
                    chatContainer.value.scrollTop = chatContainer.value.scrollHeight
                  }
                }
                
                const timing = Math.round(performance.now() - start)
                log.log('streaming complete', { len: fullResponse.length, timing })
                if (msgIndex >= 0) {
                  chatMessages.value[msgIndex].timing = { total: timing, model: 'gemini-nano' }
                }
                chromeResult = { ok: true, content: fullResponse, model: 'gemini-nano', timing }
                sessionSuccess = true
              } catch (streamErr) {
                log.log('streaming failed, trying non-streaming...', (streamErr as Error).message)
                
                // fallback to non-streaming prompt (session still alive)
                try {
                  const result = await session.prompt(fullPrompt)
                  const timing = Math.round(performance.now() - start)
                  // add message (streaming may not have added one)
                  if (msgIndex === -1) {
                    chatMessages.value.push({ 
                      role: 'assistant', 
                      content: result,
                      timing: { total: timing, model: 'gemini-nano' }
                    })
                  } else {
                    chatMessages.value[msgIndex].content = result
                    chatMessages.value[msgIndex].timing = { total: timing, model: 'gemini-nano' }
                  }
                  chromeResult = { ok: true, content: result, model: 'gemini-nano', timing }
                  log.log('non-streaming complete', { len: result.length, timing })
                  sessionSuccess = true
                } catch (promptErr) {
                  // both failed - remove message if one was added
                  if (msgIndex >= 0) {
                    chatMessages.value.splice(msgIndex, 1)
                  }
                  throw promptErr
                }
              }
              
              // destroy session only after all attempts complete
              session.destroy()
              
              if (!sessionSuccess) {
                throw new Error('Chrome AI session failed')
              }
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

      // if streaming already added the message, skip duplicate add
      if (chromeResult?.ok && chromeResult.content) {
        // message already added during streaming, just save session
      } else {
        // fallback to ollama with streaming
        const model = selectedModel.value || availableModels.value[0]
        if (!model) {
          throw new Error('no AI available - Chrome AI is off and Ollama is not running')
        }
        
        const recentMessages = chatMessages.value.slice(-6).map(m => ({
          role: m.role as 'user' | 'assistant' | 'system',
          content: m.content
        }))
        
        // add system prompt as first message
        const messagesWithSystem = [
          { role: 'system' as const, content: systemPrompt },
          ...recentMessages
        ]
        
        log.log('streaming with ollama...', model)
        const start = performance.now()
        
        const { OllamaService } = await import('@/services/OllamaService')
        let fullResponse = ''
        let msgIndex = -1
        
        try {
          for await (const chunk of OllamaService.completeStream(model, messagesWithSystem)) {
            fullResponse += chunk
            // add message on first chunk (no placeholder)
            if (msgIndex === -1) {
              msgIndex = chatMessages.value.length
              chatMessages.value.push({ role: 'assistant', content: fullResponse })
            } else {
              chatMessages.value[msgIndex].content = fullResponse
            }
            await nextTick()
            if (chatContainer.value) {
              chatContainer.value.scrollTop = chatContainer.value.scrollHeight
            }
          }
          
          const timing = Math.round(performance.now() - start)
          log.log('ollama streaming complete', { len: fullResponse.length, timing })
          if (msgIndex >= 0) {
            chatMessages.value[msgIndex].timing = { total: timing, model }
          }
        } catch (err) {
          // remove message if one was added
          if (msgIndex >= 0) {
            chatMessages.value.splice(msgIndex, 1)
          }
          throw err
        }
      }
    } catch (error) {
      log.warn('chat error', (error as Error).message)
      chatMessages.value.push({
        role: 'assistant',
        content: 'sorry, something went wrong. try again.'
      })
    } finally {
      chatIndexing.value = false
      await nextTick()
      if (chatContainer.value) {
        chatContainer.value.scrollTop = chatContainer.value.scrollHeight
      }
      chatLoading.value = false
      
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
    chatMessages.value = []
    chatInput.value = ''
    chatLoading.value = false
    emailContext.value = null
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
    resetChatState
  }
}
