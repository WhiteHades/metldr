import { ref, watch } from 'vue'
import { SummaryPrefs } from '@/utils/summaryPrefs'
import { storageService } from '@/services/StorageService'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('useSettings')

export type AIProviderPreference = 'chrome-ai' | 'ollama'

const summaryMode = ref<'manual' | 'auto'>('manual')
const allowlistInput = ref<string>(SummaryPrefs.ALLOWLIST.join('\n'))
const denylistInput = ref<string>(SummaryPrefs.DENYLIST.join('\n'))
const minAutoWords = ref<number>(SummaryPrefs.DEFAULT_PREFS.minAutoWords)
const wordPopupEnabled = ref<boolean>(true)
const preferredProvider = ref<AIProviderPreference>('chrome-ai')

export function useSettings() {
  async function loadSummaryPrefs(): Promise<void> {
    try {
      const stored = await storageService.get('summaryPrefs', {})
      const prefs = SummaryPrefs.buildPrefs(stored || {})
      summaryMode.value = prefs.mode === 'auto' ? 'auto' : 'manual'
      allowlistInput.value = prefs.allowlist.join('\n')
      denylistInput.value = prefs.denylist.join('\n')
      minAutoWords.value = prefs.minAutoWords
    } catch (err) {
      log.warn('failed to load summary prefs', (err as Error).message)
      summaryMode.value = 'manual'
      allowlistInput.value = SummaryPrefs.ALLOWLIST.join('\n')
      denylistInput.value = SummaryPrefs.DENYLIST.join('\n')
      minAutoWords.value = SummaryPrefs.DEFAULT_PREFS.minAutoWords
    }
  }

  async function saveSummaryPrefs(): Promise<void> {
    try {
      const prefs = {
        mode: summaryMode.value,
        allowlist: SummaryPrefs.parseListInput(allowlistInput.value, SummaryPrefs.ALLOWLIST),
        denylist: SummaryPrefs.parseListInput(denylistInput.value, SummaryPrefs.DENYLIST),
        minAutoWords: Number.isFinite(Number(minAutoWords.value)) ? Number(minAutoWords.value) : SummaryPrefs.DEFAULT_PREFS.minAutoWords,
        minPromptWords: SummaryPrefs.DEFAULT_PREFS.minPromptWords
      }
      await storageService.set('summaryPrefs', prefs)
    } catch (err) {
      log.warn('failed to save summary prefs', (err as Error).message)
    }
  }

  async function loadWordPopupSetting(): Promise<void> {
    try {
      const enabled = await storageService.get('wordPopupEnabled', true)
      wordPopupEnabled.value = enabled
    } catch (error) {
      log.error('failed to load word popup settings', error)
    }
  }

  async function loadProviderPreference(): Promise<void> {
    try {
      const provider = await storageService.get<AIProviderPreference>('preferredProvider', 'chrome-ai')
      if (provider === 'chrome-ai' || provider === 'ollama') {
        preferredProvider.value = provider
      }
    } catch (error) {
      log.error('failed to load provider preference', error)
    }
  }

  async function setProviderPreference(provider: AIProviderPreference): Promise<void> {
    preferredProvider.value = provider
    try {
      await storageService.set('preferredProvider', provider)
    } catch (error) {
      log.error('failed to save provider preference', error)
    }
  }

  async function toggleWordPopup(newValue?: boolean): Promise<void> {
    wordPopupEnabled.value = newValue ?? !wordPopupEnabled.value
    
    try {
      await storageService.set('wordPopupEnabled', wordPopupEnabled.value)
      
      const tabs = await chrome.tabs.query({})
      tabs.forEach(tab => {
        if (tab.id !== undefined) {
          chrome.tabs.sendMessage(tab.id, {
            type: 'WORD_POPUP_TOGGLED',
            enabled: wordPopupEnabled.value
          }).catch(() => {})
        }
      })
    } catch (error) {
      log.error('failed to save word popup setting', error)
    }
  }

  function setupSettingsWatcher(): void {
    watch([summaryMode, allowlistInput, denylistInput, minAutoWords], () => {
      saveSummaryPrefs()
    })
  }

  function setupStorageListener(
    onSelectedModelChange?: (model: string) => void,
    onSelectedLanguagesChange?: (langs: string[]) => void
  ): void {
    storageService.onChange((changes) => {
      if (changes.summaryPrefs?.newValue) {
        const prefs = changes.summaryPrefs.newValue as { mode?: string; allowlist?: string[]; denylist?: string[]; minAutoWords?: number }
        summaryMode.value = prefs.mode === 'auto' ? 'auto' : 'manual'
        if (prefs.allowlist) allowlistInput.value = prefs.allowlist.join('\n')
        if (prefs.denylist) denylistInput.value = prefs.denylist.join('\n')
        if (prefs.minAutoWords) minAutoWords.value = prefs.minAutoWords
      }
      
      if (changes.wordPopupEnabled !== undefined) {
        wordPopupEnabled.value = (changes.wordPopupEnabled.newValue as boolean) ?? true
      }
      
      if (changes.selectedModel?.newValue && onSelectedModelChange) {
        onSelectedModelChange(changes.selectedModel.newValue as string)
      }
      
      if (changes.selectedLanguages?.newValue && onSelectedLanguagesChange) {
        onSelectedLanguagesChange(changes.selectedLanguages.newValue as string[])
      }

      if (changes.preferredProvider?.newValue) {
        const pref = changes.preferredProvider.newValue as string
        if (pref === 'chrome-ai' || pref === 'ollama') {
          preferredProvider.value = pref
        }
      }
    })
  }

  return {
    summaryMode,
    allowlistInput,
    denylistInput,
    minAutoWords,
    wordPopupEnabled,
    preferredProvider,
    
    loadSummaryPrefs,
    saveSummaryPrefs,
    loadWordPopupSetting,
    toggleWordPopup,
    loadProviderPreference,
    setProviderPreference,
    setupSettingsWatcher,
    setupStorageListener
  }
}
