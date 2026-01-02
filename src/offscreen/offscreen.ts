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
        // create session per chrome_ai.md docs - use expectedInputs/expectedOutputs format
        const session = await LanguageModel.create({
          initialPrompts: [{ role: 'system', content: payload.systemPrompt || 'You are a helpful assistant.' }],
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
  if (iframe.contentWindow) {
    const requestId = msg.data?.requestId
    if (requestId) {
      pendingResponses.set(requestId, sendResponse)
    }
    iframe.contentWindow.postMessage(msg.data, '*')
    return true
  } else {
    console.error('[Offscreen] Iframe not found')
    sendResponse({ error: 'Sandbox iframe not found' })
    return false
  }
})

// listen for sandbox responses
window.addEventListener('message', (event) => {
  if (event.source !== iframe.contentWindow) return
  
  const { requestId, ...data } = event.data
  if (requestId && pendingResponses.has(requestId)) {
    const resolve = pendingResponses.get(requestId)!
    pendingResponses.delete(requestId)
    resolve(data)
  }
})

console.log('[Offscreen] Ready')

