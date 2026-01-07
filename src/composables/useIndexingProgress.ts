import { ref, computed } from 'vue'

// per-url indexing state tracking
const indexingByUrl = ref<Map<string, { percent: number; message: string }>>(new Map())

// simple progress messages
const PROGRESS_MESSAGES = [
  'crunching vectors...',
  'embedding chunks...',
  'building index...',
  'processing content...',
  'working on it...',
  'almost there...'
]

let messageIndex = 0
let listenerRegistered = false

function getRandomMessage(): string {
  return PROGRESS_MESSAGES[messageIndex++ % PROGRESS_MESSAGES.length]
}

function updateProgress(sourceId: string, percent: number) {
  if (percent >= 100) {
    // completed - remove after brief delay
    setTimeout(() => {
      indexingByUrl.value.delete(sourceId)
    }, 800)
    indexingByUrl.value.set(sourceId, { percent: 100, message: 'done!' })
  } else {
    indexingByUrl.value.set(sourceId, { percent, message: getRandomMessage() })
  }
}

function handleMessage(msg: any) {
  if (msg.type === 'INDEXING_PROGRESS' && msg.sourceId) {
    updateProgress(msg.sourceId, msg.percent)
  }
  return false
}

export function useIndexingProgress(currentUrl?: () => string) {
  // check if current url is being indexed
  const isIndexing = computed(() => {
    if (!currentUrl) return indexingByUrl.value.size > 0
    const url = currentUrl()
    if (!url) return false
    // check if any key starts with this url (partial match for url normalization)
    for (const key of indexingByUrl.value.keys()) {
      if (key.startsWith(url.slice(0, 50)) || url.startsWith(key.slice(0, 50))) {
        return true
      }
    }
    return false
  })

  const indexingProgress = computed(() => {
    if (!currentUrl) return 0
    const url = currentUrl()
    if (!url) return 0
    for (const [key, val] of indexingByUrl.value.entries()) {
      if (key.startsWith(url.slice(0, 50)) || url.startsWith(key.slice(0, 50))) {
        return val.percent
      }
    }
    return 0
  })

  const indexingMessage = computed(() => {
    if (!currentUrl) return ''
    const url = currentUrl()
    if (!url) return ''
    for (const [key, val] of indexingByUrl.value.entries()) {
      if (key.startsWith(url.slice(0, 50)) || url.startsWith(key.slice(0, 50))) {
        return val.message
      }
    }
    return ''
  })

  const progressText = computed(() => {
    if (!isIndexing.value) return ''
    return `${indexingMessage.value} ${indexingProgress.value}%`
  })

  function setupListener() {
    if (listenerRegistered) return
    chrome.runtime.onMessage.addListener(handleMessage)
    listenerRegistered = true
  }

  function cleanupListener() {
    if (!listenerRegistered) return
    chrome.runtime.onMessage.removeListener(handleMessage)
    listenerRegistered = false
  }

  return {
    isIndexing,
    indexingProgress,
    indexingMessage,
    progressText,
    setupListener,
    cleanupListener
  }
}
