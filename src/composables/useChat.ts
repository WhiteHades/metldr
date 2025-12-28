import { ref, nextTick, type Ref } from 'vue'
import type { AppPageSummary, AppChatMessage, AppChatResponse } from '@/types'
import type { AIProviderPreference } from './useSettings'
import { sendToBackground, withTiming } from './useMessaging'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('Chat')

const chatMessages = ref<AppChatMessage[]>([])
const chatInput = ref<string>('')
const chatLoading = ref<boolean>(false)
const emailContext = ref<{ summary?: string; subject?: string; sender?: string } | null>(null)

declare const LanguageModel: {
  availability: (opts?: { languages?: string[] }) => Promise<string | { available: string }>
  create: (options: { 
    initialPrompts?: { role: string; content: string }[]
    temperature?: number
    topK?: number
    expectedInputs?: { type: string; languages: string[] }[]
    expectedOutputs?: { type: string; languages: string[] }[]
  }) => Promise<{
    prompt: (text: string) => Promise<string>
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
            }
          }
        } catch (err) {
          log.log('failed to fetch page content', (err as Error).message)
        }
      }
      
      const MAX_CONTEXT = 50000
      if (contextText.length > MAX_CONTEXT) {
        const headLen = Math.floor(MAX_CONTEXT * 0.6)
        const tailLen = MAX_CONTEXT - headLen
        contextText = contextText.slice(0, headLen) + '\n\n[...truncated...]\n\n' + contextText.slice(-tailLen)
      }

      const isEmail = contextText.includes('EMAIL CONTENT:')
      const systemPrompt = contextText 
        ? isEmail 
          ? `you are an assistant helping the user understand an email thread.\n\n${contextText}\n\nRULES:\n1. answer based ONLY on the email content\n2. if info isn't in the email, say so\n3. be concise`
          : `you are an assistant helping the user understand an article.\n\nARTICLE:\n${contextText}\n\nRULES:\n1. answer based ONLY on the article\n2. if info isn't in the article, say so\n3. be concise`
        : 'you are a helpful assistant. be concise.'

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
                const session = await LanguageModel.create({
                  initialPrompts: [{ role: 'system', content: systemPrompt }],
                  temperature: 0.7,
                  topK: 3,
                  expectedInputs: [{ type: 'text', languages: ['en', 'es', 'ja'] }],
                  expectedOutputs: [{ type: 'text', languages: ['en'] }]
                })
                log.log('session created, prompting...')
                
                const start = performance.now()
                try {
                  const result = await session.prompt(userMessage)
                  const timing = Math.round(performance.now() - start)
                  log.log('prompt succeeded', { len: result?.length, timing })
                  chromeResult = { ok: true, content: result, model: 'gemini-nano', timing }
                } finally {
                  session.destroy()
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

      if (chromeResult?.ok && chromeResult.content) {
        chatMessages.value.push({ 
          role: 'assistant', 
          content: chromeResult.content,
          timing: { total: chromeResult.timing || 0, model: 'gemini-nano' }
        })
      } else {
        // fallback to background/ollama
        const model = selectedModel.value || availableModels.value[0]
        const recentMessages = chatMessages.value.slice(-6).map(m => ({
          role: m.role,
          content: m.content
        }))
        
        const response = await sendToBackground<AppChatResponse | null>({
          type: 'CHAT_MESSAGE',
          model,
          messages: recentMessages,
          pageContext: pageSummary.value ? {
            title: pageSummary.value.title,
            author: pageSummary.value.author,
            publication: pageSummary.value.publication,
            content: pageSummary.value.content,
            fullContent: pageSummary.value.fullContent
          } : null
        })
        
        if (response?.ok && response.content) {
          chatMessages.value.push({ 
            role: 'assistant', 
            content: response.content,
            timing: response.timing
          })
        } else {
          throw new Error(response?.error || 'chat failed')
        }
      }
    } catch (error) {
      log.warn('chat error', (error as Error).message)
      chatMessages.value.push({
        role: 'assistant',
        content: 'sorry, something went wrong. try again.'
      })
    } finally {
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
    emailContext,
    
    // Methods
    sendChatMessage,
    clearChat,
    resetChatState
  }
}
