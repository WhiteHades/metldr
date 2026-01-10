/// <reference types="chrome" />

// offscreen.ts - bridge between Service Worker and DOM contexts
// handles: 1) sandbox iframe relay, 2) chrome AI API calls

console.log('[Offscreen] Initializing bridge...')

const iframe = document.getElementById('sandbox-frame') as HTMLIFrameElement
const pendingResponses = new Map<string, (response: any) => void>()

// chrome AI handler - runs in offscreen context (has window context)
async function handleChromeAI(msg: any): Promise<any> {
  const { action, payload } = msg
  
  try {
    switch (action) {
      case 'summarize': {
        if (typeof Summarizer === 'undefined') {
          return { ok: false, error: 'Summarizer not available' }
        }
        const avail = await Summarizer.availability()
        if (avail === 'unavailable') {
          return { ok: false, error: 'Summarizer unavailable' }
        }
        // create summarizer per chrome_ai.md docs
        const summarizer = await Summarizer.create({
          type: payload.type || 'key-points',
          length: payload.length || 'medium',
          format: 'markdown',
          expectedInputLanguages: ['en', 'es', 'ja'],
          expectedContextLanguages: ['en', 'es', 'ja'],
          outputLanguage: 'en'
        })
        try {
          const summary = await summarizer.summarize(payload.content, { context: payload.context })
          return { ok: true, summary, provider: 'chrome-ai' }
        } finally {
          summarizer.destroy()
        }
      }
      
      case 'complete': {
        if (typeof LanguageModel === 'undefined') {
          return { ok: false, error: 'LanguageModel not available' }
        }
        const initialPrompts: Array<{ role: 'system' | 'user' | 'assistant'; content: string }> = [
          { role: 'system', content: payload.systemPrompt || 'You are a helpful assistant.' }
        ]
        
        if (payload.messages?.length) {
          initialPrompts.push(...payload.messages.slice(-6))
        }
        
        const session = await LanguageModel.create({
          initialPrompts,
          expectedInputs: [{ type: 'text', languages: ['en', 'es', 'ja'] }],
          expectedOutputs: [{ type: 'text', languages: ['en'] }]
        })
        try {
          const response = await session.prompt(payload.userPrompt)
          return { ok: true, content: response, provider: 'chrome-ai', model: 'gemini-nano' }
        } finally {
          session.destroy()
        }
      }
      
      case 'detectLanguage': {
        if (typeof LanguageDetector === 'undefined') {
          return { ok: false, error: 'LanguageDetector not available' }
        }
        const ldAvail = await LanguageDetector.availability()
        if (ldAvail === 'unavailable') {
          return { ok: false, error: 'LanguageDetector unavailable' }
        }
        
        const detector = await LanguageDetector.create()
        try {
          const results = await detector.detect(payload.text)
          if (!results?.length) {
            return { ok: false, error: 'no language detected' }
          }
          const top = results[0]
          return { 
            ok: true, 
            language: top.detectedLanguage, 
            confidence: top.confidence, 
            allResults: results, 
            provider: 'chrome-ai' 
          }
        } finally {
          detector.destroy()
        }
      }
      
      case 'capabilities': {
        const caps = {
          complete: typeof LanguageModel !== 'undefined',
          summarize: typeof Summarizer !== 'undefined',
          translate: typeof Translator !== 'undefined',
          detectLanguage: typeof LanguageDetector !== 'undefined',
          write: typeof Writer !== 'undefined',
          rewrite: typeof Rewriter !== 'undefined'
        }
        return { ok: true, capabilities: caps }
      }
      
      default:
        return { ok: false, error: `Unknown Chrome AI action: ${action}` }
    }
  } catch (err) {
    const errMsg = err instanceof DOMException ? `DOMException: ${err.name} - ${err.message}` : (err as Error).message
    console.error('[Offscreen] Chrome AI error:', errMsg)
    return { ok: false, error: errMsg }
  }
}

// main message listener
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.target !== 'offscreen') return false

  console.log('[Offscreen] Received:', msg.type)

  // handle ping
  if (msg.type === 'PING') {
    sendResponse({ status: 'pong' })
    return false
  }

  // handle chrome AI requests directly (they need window context)
  if (msg.type === 'CHROME_AI') {
    handleChromeAI(msg).then(sendResponse)
    return true
  }

  // forward to sandbox iframe for GPU/ML operations
  if (!iframe || !iframe.contentWindow) {
    console.error('[Offscreen] Sandbox iframe not ready')
    sendResponse({ ok: false, error: 'Sandbox iframe not ready' })
    return false
  }

  const requestId = msg.data?.requestId
  if (requestId) {
    // set timeout for sandbox response (3 minute timeout for model loading)
    const timeout = setTimeout(() => {
      if (pendingResponses.has(requestId)) {
        console.error('[Offscreen] Sandbox request timed out:', msg.type)
        pendingResponses.delete(requestId)
        sendResponse({ ok: false, error: `Sandbox timeout for ${msg.type}` })
      }
    }, 180000)
    
    pendingResponses.set(requestId, (response) => {
      clearTimeout(timeout)
      sendResponse(response)
    })
  }
  
  iframe.contentWindow.postMessage(msg.data, '*')
  return true
})

// listen for sandbox responses
window.addEventListener('message', (event) => {
  if (event.source !== iframe?.contentWindow) return
  
  const { requestId, ...data } = event.data
  
  // handle ready signal
  if (event.data?._gpuBridgeReady) {
    console.log('[Offscreen] GPU Bridge signaled ready')
    return
  }
  
  if (requestId && pendingResponses.has(requestId)) {
    const resolve = pendingResponses.get(requestId)!
    pendingResponses.delete(requestId)
    
    console.log('[Offscreen] Sandbox response for', requestId, '- ok:', data.ok, 'error:', data.error)
    
    // ensure error field exists if not ok
    if (!data.ok && !data.error) {
      console.warn('[Offscreen] Response has ok=false but no error message')
      data.error = 'Unknown sandbox error'
    }
    
    resolve(data)
  }
})

console.log('[Offscreen] Ready')

