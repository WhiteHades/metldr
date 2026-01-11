import { ref, onUnmounted } from 'vue'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('ChromeAI')

type ChromeAIStatus = 'available' | 'downloadable' | 'downloading' | 'unavailable' | 'checking'

const chromeAIStatus = ref<ChromeAIStatus>('checking')
let pollingInterval: ReturnType<typeof setInterval> | null = null
let storageListener: ((changes: { [key: string]: chrome.storage.StorageChange }, area: string) => void) | null = null
let isPolling = false

declare const LanguageModel: {
  availability: (opts?: { languages?: string[] }) => Promise<string | { available: string }>
} | undefined

declare const Summarizer: {
  availability: () => Promise<string>
} | undefined

async function doCheck(silent = false): Promise<void> {
  try {
    if (!silent) log.log('checking Chrome AI availability')
    
    let hasLanguageModel = false
    let hasSummarizer = false
    let lmStatus: string | null = null
    let sumStatus: string | null = null

    if (typeof LanguageModel !== 'undefined') {
      try {
        const avail = await LanguageModel.availability({ languages: ['en', 'es', 'ja'] })
        lmStatus = typeof avail === 'string' ? avail : avail?.available
        hasLanguageModel = lmStatus === 'available' || lmStatus === 'downloadable' || lmStatus === 'downloading' || lmStatus === 'readily'
        if (!silent) log.log('LanguageModel.availability', lmStatus)
      } catch (e) {
        if (!silent) log.log('LanguageModel.availability error', (e as Error).message)
      }
    }

    if (typeof Summarizer !== 'undefined') {
      try {
        sumStatus = await Summarizer.availability()
        hasSummarizer = sumStatus === 'available' || sumStatus === 'downloadable' || sumStatus === 'downloading'
        if (!silent) log.log('Summarizer.availability', sumStatus)
      } catch (e) {
        if (!silent) log.log('Summarizer.availability error', (e as Error).message)
      }
    }

    const bestStatus = lmStatus || sumStatus
    
    if (hasLanguageModel || hasSummarizer) {
      if (bestStatus === 'available' || bestStatus === 'readily') {
        chromeAIStatus.value = 'available'
      } else if (bestStatus === 'downloadable') {
        chromeAIStatus.value = 'downloadable'
      } else if (bestStatus === 'downloading') {
        chromeAIStatus.value = 'downloading'
      } else {
        chromeAIStatus.value = 'unavailable'
      }
    } else {
      chromeAIStatus.value = 'unavailable'
    }
    
    if (!silent) log.log('final status', chromeAIStatus.value)
  } catch (e) {
    log.error('check failed', (e as Error).message)
    chromeAIStatus.value = 'unavailable'
  }
}

// start polling when status is downloading or downloadable
function startPolling(): void {
  if (isPolling) return
  isPolling = true
  
  pollingInterval = setInterval(async () => {
    const prevStatus = chromeAIStatus.value
    await doCheck(true)
    
    // stop polling once available
    if (chromeAIStatus.value === 'available') {
      log.log('chrome ai now available, stopping poll')
      stopPolling()
    }
    
    // also stop if becomes unavailable (download cancelled)
    if (prevStatus === 'downloading' && chromeAIStatus.value === 'unavailable') {
      log.log('download cancelled/failed, stopping poll')
      stopPolling()
    }
  }, 2000) // check every 2s
}

function stopPolling(): void {
  if (pollingInterval) {
    clearInterval(pollingInterval)
    pollingInterval = null
  }
  isPolling = false
}

// listen for storage changes from welcome page download completion
function setupStorageListener(): void {
  if (storageListener) return
  
  storageListener = (changes, area) => {
    if (area === 'local' && changes.chromeAIDownloadComplete) {
      log.log('received download complete signal from welcome page')
      // immediately recheck status
      doCheck(true)
    }
    if (area === 'local' && changes.chromeAIStatus?.newValue === 'available') {
      log.log('received status update from storage')
      chromeAIStatus.value = 'available'
    }
  }
  
  chrome.storage.onChanged.addListener(storageListener)
}

function cleanupStorageListener(): void {
  if (storageListener) {
    chrome.storage.onChanged.removeListener(storageListener)
    storageListener = null
  }
}

export function useChromeAI() {
  async function checkChromeAI(): Promise<void> {
    await doCheck()
    
    // setup listeners for cross-context communication
    setupStorageListener()
    
    // start polling if not yet available
    if (chromeAIStatus.value === 'downloading' || chromeAIStatus.value === 'downloadable') {
      startPolling()
    }
  }
  
  // force refresh - useful for manual refresh buttons
  async function refreshChromeAI(): Promise<void> {
    chromeAIStatus.value = 'checking'
    await doCheck()
  }
  
  // cleanup function for component unmount
  function cleanup(): void {
    stopPolling()
    cleanupStorageListener()
  }

  return {
    chromeAIStatus,
    checkChromeAI,
    refreshChromeAI,
    cleanup
  }
}
