import { ref } from 'vue'
import { StorageManager } from '@/utils/storage'
import type { DownloadProgressItem } from '@/types'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('Dictionary')

const downloadedLanguages = ref<string[]>([])
const selectedLanguages = ref<string[]>(['en'])
const downloadProgress = ref<Record<string, DownloadProgressItem>>({})

export function useDictionary() {
  const storage = new StorageManager()

  async function loadDictionarySettings(): Promise<void> {
    try {
      log.log('loading dictionary settings')
      await storage.initDictionary()
      const persisted = await chrome.storage.local.get(['dictDownloadProgress', 'downloadingLanguages'])
      const persistedProgress = (persisted.dictDownloadProgress || {}) as Record<string, DownloadProgressItem>
      const persistedDownloading = Array.isArray(persisted.downloadingLanguages) ? persisted.downloadingLanguages as string[] : []
      downloadProgress.value = { ...persistedProgress }
      downloadedLanguages.value = await storage.dictGetDownloadedLanguages() as string[]
      log.log('downloaded languages', downloadedLanguages.value)
      
      const settings = await chrome.storage.local.get(['selectedLanguages'])
      if (settings.selectedLanguages && Array.isArray(settings.selectedLanguages) && settings.selectedLanguages.length > 0) {
        selectedLanguages.value = settings.selectedLanguages as string[]
      }
      
      // sync: add all downloaded languages to selected (they should be usable)
      downloadedLanguages.value.forEach((lang: string) => {
        if (!selectedLanguages.value.includes(lang)) {
          selectedLanguages.value.push(lang)
        }
      })
      await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
      
      log.log('selected languages', selectedLanguages.value)
      
      persistedDownloading.forEach((lang: string) => {
        if (!downloadProgress.value[lang]) {
          downloadProgress.value = { ...downloadProgress.value, [lang]: { progress: 0, letter: 'a', entries: 0 } }
        }
        startDownload(lang)
      })

      selectedLanguages.value.forEach((lang: string) => {
        if (!downloadedLanguages.value.includes(lang) && !downloadProgress.value[lang]) {
          startDownload(lang)
        }
      })

      // auto-download english if not present
      if (!downloadedLanguages.value.includes('en') && !downloadProgress.value['en']) {
        log.log('english not downloaded, starting download')
        if (!selectedLanguages.value.includes('en')) {
          selectedLanguages.value.push('en')
          await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
        }
        startDownload('en')
      }
    } catch (error) {
      log.error('failed to load dictionary settings', error)
    }
  }

  async function startDownload(langCode: string): Promise<void> {
    log.log('starting download for', langCode)
    downloadProgress.value = { ...downloadProgress.value, [langCode]: { progress: 0, letter: 'a', entries: 0 } }
    
    try {
      await storage.dictDownloadLanguage(langCode, (progressData: { progress: number; letter: string; entriesProcessed: number }) => {
        downloadProgress.value = { ...downloadProgress.value, [langCode]: {
          progress: progressData.progress,
          letter: progressData.letter,
          entries: progressData.entriesProcessed
        } }
      })
      
      log.log('download completed for', langCode)
      const { [langCode]: _, ...rest } = downloadProgress.value
      downloadProgress.value = rest
      downloadedLanguages.value = await storage.dictGetDownloadedLanguages() as string[]

      if (!selectedLanguages.value.includes(langCode)) {
        selectedLanguages.value.push(langCode)
        await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
      }
    } catch (error) {
      log.error('download failed for ' + langCode, error)
      const { [langCode]: _, ...rest } = downloadProgress.value
      downloadProgress.value = rest
    }
  }

  async function toggleLanguage(langCode: string): Promise<void> {
    console.log('[Dictionary] toggleLanguage called:', langCode)
    console.log('[Dictionary] current selectedLanguages:', selectedLanguages.value)
    console.log('[Dictionary] current downloadedLanguages:', downloadedLanguages.value)
    
    const index = selectedLanguages.value.indexOf(langCode)
    
    if (index === -1) {
      // enabling language
      log.log('enabling language:', langCode)
      selectedLanguages.value = [...selectedLanguages.value, langCode]
      await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
      
      // trigger download if not already downloaded
      if (!downloadedLanguages.value.includes(langCode) && !downloadProgress.value[langCode]) {
        log.log('language not downloaded, starting download:', langCode)
        startDownload(langCode)
      } else {
        log.log('language already downloaded or downloading:', langCode)
      }
    } else {
      // disabling language
      log.log('disabling language:', langCode)
      selectedLanguages.value = selectedLanguages.value.filter(l => l !== langCode)
      await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
    }
    
    log.log('updated selectedLanguages:', selectedLanguages.value)
  }

  async function deleteLanguageData(langCode: string): Promise<void> {
    if (!confirm(`delete ${langCode} dictionary data?`)) return
    
    try {
      downloadedLanguages.value = downloadedLanguages.value.filter(l => l !== langCode)
      const { [langCode]: _, ...rest } = downloadProgress.value
      downloadProgress.value = rest
      await chrome.storage.local.set({
        downloadingLanguages: Object.keys(rest),
        dictDownloading: false
      })

      await storage.dictDeleteLanguage(langCode)
      downloadedLanguages.value = await storage.dictGetDownloadedLanguages() as string[]
      const index = selectedLanguages.value.indexOf(langCode)
      if (index !== -1) {
        selectedLanguages.value.splice(index, 1)
        await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
      }
    } catch (error) {
      log.error('failed to delete language', error)
      alert('failed to delete dictionary')
    }
  }

  async function initSelectedLanguages(): Promise<void> {
    try {
      await chrome.storage.local.set({ 
        selectedLanguages: selectedLanguages.value?.length > 0 ? selectedLanguages.value : ['en']
      })
    } catch (error) {
      log.error('failed to save selectedLanguages', error)
    }
  }

  function updateSelectedLanguages(langs: string[]): void {
    selectedLanguages.value = langs
  }

  return {
    // State
    downloadedLanguages,
    selectedLanguages,
    downloadProgress,
    storage,
    
    // Methods
    loadDictionarySettings,
    startDownload,
    toggleLanguage,
    deleteLanguageData,
    initSelectedLanguages,
    updateSelectedLanguages
  }
}
