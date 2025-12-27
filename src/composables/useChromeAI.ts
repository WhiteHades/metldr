import { ref } from 'vue'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('ChromeAI')

type ChromeAIStatus = 'available' | 'downloadable' | 'downloading' | 'unavailable' | 'checking'

const chromeAIStatus = ref<ChromeAIStatus>('checking')

declare const LanguageModel: {
  availability: (opts?: { languages?: string[] }) => Promise<string | { available: string }>
} | undefined

declare const Summarizer: {
  availability: () => Promise<string>
} | undefined

export function useChromeAI() {
  async function checkChromeAI(): Promise<void> {
    try {
      log.log('checking Chrome AI availability')
      
      let hasLanguageModel = false
      let hasSummarizer = false
      let lmStatus: string | null = null
      let sumStatus: string | null = null

      if (typeof LanguageModel !== 'undefined') {
        try {
          const avail = await LanguageModel.availability({ languages: ['en', 'es', 'ja'] })
          lmStatus = typeof avail === 'string' ? avail : avail?.available
          hasLanguageModel = lmStatus === 'available' || lmStatus === 'downloadable' || lmStatus === 'downloading' || lmStatus === 'readily'
          log.log('LanguageModel.availability', lmStatus)
        } catch (e) {
          log.log('LanguageModel.availability error', (e as Error).message)
        }
      } else {
        log.log('LanguageModel is undefined')
      }

      if (typeof Summarizer !== 'undefined') {
        try {
          sumStatus = await Summarizer.availability()
          hasSummarizer = sumStatus === 'available' || sumStatus === 'downloadable' || sumStatus === 'downloading'
          log.log('Summarizer.availability', sumStatus)
        } catch (e) {
          log.log('Summarizer.availability error', (e as Error).message)
        }
      } else {
        log.log('Summarizer is undefined')
      }

      const bestStatus = lmStatus || sumStatus
      
      if (hasLanguageModel || hasSummarizer) {
        if (bestStatus === 'available') {
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
      
      log.log('final status', chromeAIStatus.value)
    } catch (e) {
      log.error('check failed', (e as Error).message)
      chromeAIStatus.value = 'unavailable'
    }
  }

  return {
    chromeAIStatus,
    checkChromeAI
  }
}
