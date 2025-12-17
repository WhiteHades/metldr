<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick, computed, watch } from 'vue'
import { useThemeStore } from '@/stores/theme'
import { StorageManager, SUPPORTED_LANGUAGES } from '@/lib/StorageManager'
import { SummaryPrefs } from '@/lib/summaryPrefs'
import { formatTime, stripThinking } from '@/lib/textUtils'
import HistoryManager from '@/components/HistoryManager.vue'
import { Button, Toggle, ScrollArea, ScrollBar, Checkbox, Textarea, Input } from '@/components/ui'
import { marked } from 'marked'
import { 
  Sparkles, BarChart3, Settings, Loader2, ChevronDown, ChevronUp, Check, 
  Send, Trash2, X, RefreshCw, Database,
  Zap, Server, Circle, MessageCircle, FileText, AlertCircle
} from 'lucide-vue-next'
import type {
  AppPageSummary,
  AppChatMessage,
  SummaryPromptData,
  DropdownPos,
  DownloadProgressItem,
  OllamaHealthResponse,
  AppSummaryResponse,
  AppChatResponse
} from '@/types'

marked.setOptions({
  breaks: true,
  gfm: true
});

function renderMarkdown(text: string): string {
  if (!text) return ''
  const cleaned = stripThinking(text)
  return marked.parseInline(cleaned) as string
}

const themeStore = useThemeStore()

const activeTab = ref<string>('summary')
const ollamaStatus = ref<'checking' | 'ready' | 'not-found' | 'error'>('checking')
const availableModels = ref<string[]>([])
const selectedModel = ref<string>('')
const historyRef = ref<InstanceType<typeof HistoryManager> | null>(null)
const showModelDropdown = ref<boolean>(false)

const modelDropdownPos = ref<DropdownPos>({ top: 0, left: 0, width: 0 })
const chatContainer = ref<HTMLDivElement | null>(null)
const chatInputRef = ref<HTMLInputElement | null>(null)

// page summary state
const pageSummary = ref<AppPageSummary | null>(null)
const pageMetadata = ref<{ title: string; url: string } | null>(null)
const summaryLoading = ref<boolean>(false)
const summaryError = ref<string | null>(null)
const currentTabId = ref<number | null>(null)
const currentTabUrl = ref<string | null>(null)
const isEmailClient = computed(() => {
  if (!currentTabUrl.value) return false
  return currentTabUrl.value.includes('mail.google.com') || 
         currentTabUrl.value.includes('outlook.') || 
         currentTabUrl.value.includes('mail.yahoo.com')
})
const summaryCollapsed = ref<boolean>(false)
const summaryPrompt = ref<SummaryPromptData | null>(null)
const summaryMode = ref<'manual' | 'auto'>('manual')
const allowlistInput = ref<string>(SummaryPrefs.ALLOWLIST.join('\n'))
const denylistInput = ref<string>(SummaryPrefs.DENYLIST.join('\n'))
const minAutoWords = ref<number>(SummaryPrefs.DEFAULT_PREFS.minAutoWords)

// chat state
const chatMessages = ref<AppChatMessage[]>([])
const chatInput = ref<string>('')
const chatLoading = ref<boolean>(false)

const wordPopupEnabled = ref<boolean>(true)

const downloadedLanguages = ref<string[]>([])
const selectedLanguages = ref<string[]>(['en'])
const downloadProgress = ref<Record<string, DownloadProgressItem>>({})
const copiedSetup = ref<boolean>(false)

const isWindows = typeof navigator !== 'undefined' && navigator.platform?.toLowerCase().includes('win')
const setupCommands = isWindows
  ? `ollama serve`
  : `OLLAMA_ORIGINS="chrome-extension://*" ollama serve`

const storage = new StorageManager()

function getTabStorageKey(url: string | null): string | null {
  if (!url) return null
  try {
    const u = new URL(url)
    return `tab_session_${u.origin}${u.pathname}`
  } catch {
    return `tab_session_${url}`
  }
}

async function saveTabSession() {
  const key = getTabStorageKey(currentTabUrl.value);
  if (!key) return;
  
  try {
    const session = {
      chatMessages: chatMessages.value,
      pageSummary: pageSummary.value,
      summaryCollapsed: summaryCollapsed.value,
      timestamp: Date.now()
    };
    await chrome.storage.local.set({ [key]: session });
  } catch (err) {
    console.warn('metldr: failed to save tab session:', (err as Error).message)
  }
}

async function loadTabSession(url: string): Promise<boolean> {
  const key = getTabStorageKey(url)
  if (!key) return false
  
  try {
    const result = await chrome.storage.local.get([key])
    const session = result[key] as { timestamp: number; chatMessages: AppChatMessage[]; pageSummary: AppPageSummary | null; summaryCollapsed: boolean } | undefined
    
    if (session && (Date.now() - session.timestamp) < 86400000) {
      chatMessages.value = Array.isArray(session.chatMessages) ? session.chatMessages : []
      pageSummary.value = session.pageSummary || null
      summaryCollapsed.value = session.summaryCollapsed || false
      return true
    }
  } catch (err) {
    console.warn('metldr: failed to load tab session:', (err as Error).message)
  }
  return false
}

async function loadSummaryPrefs(): Promise<void> {
  try {
    const stored = await chrome.storage.local.get(['summaryPrefs'])
    const prefs = SummaryPrefs.buildPrefs(stored?.summaryPrefs || {})
    summaryMode.value = prefs.mode === 'auto' ? 'auto' : 'manual'
    allowlistInput.value = prefs.allowlist.join('\n')
    denylistInput.value = prefs.denylist.join('\n')
    minAutoWords.value = prefs.minAutoWords
  } catch (err) {
    console.warn('metldr: failed to load summary prefs:', (err as Error).message)
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
    await chrome.storage.local.set({ summaryPrefs: prefs })
  } catch (err) {
    console.warn('metldr: failed to save summary prefs:', (err as Error).message)
  }
}

async function sendToBackground(message: Record<string, unknown>, retries = 2): Promise<unknown> {
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const response = await chrome.runtime.sendMessage(message)
      return response
    } catch (error) {
      if ((error as Error).message?.includes('Receiving end does not exist') && attempt < retries) {
        await new Promise(resolve => setTimeout(resolve, 100 * (attempt + 1)))
        continue
      }
      throw error
    }
  }
}

function updateDropdownPosition(buttonElement: Element | null, posRef: { value: DropdownPos }): void {
  if (!buttonElement) return
  const rect = buttonElement.getBoundingClientRect()
  posRef.value = {
    top: rect.bottom + 8,
    left: rect.left,
    width: rect.width,
  }
}

function toggleModelDropdown(): void {
  if (!showModelDropdown.value) {
    const btn = document.querySelector('.model-selector-btn')
    updateDropdownPosition(btn, modelDropdownPos)
  }
  showModelDropdown.value = !showModelDropdown.value
}

async function selectModel(model: string): Promise<void> {
  selectedModel.value = model
  showModelDropdown.value = false
  try {
    await chrome.storage.local.set({ selectedModel: model })
  } catch (error) {
    console.error('metldr: failed to save model selection:', error)
  }
}

async function checkOllama(showChecking = true): Promise<boolean> {
  const wasReady = ollamaStatus.value === 'ready'
  if (showChecking && !wasReady) {
    ollamaStatus.value = 'checking'
  }
  
  try {
    storage.initDictionary().catch((err: Error) => {
      console.warn('metldr: dict init failed:', err.message)
    })
    
    let response: OllamaHealthResponse | null = null
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        response = await sendToBackground({ type: 'CHECK_OLLAMA_HEALTH' }) as OllamaHealthResponse
        if (response && response.success !== undefined) break
        await new Promise(r => setTimeout(r, 150 * (attempt + 1)))
      } catch (err) {
        console.warn('metldr: ollama check attempt', attempt + 1, 'failed:', (err as Error).message)
        await new Promise(r => setTimeout(r, 150 * (attempt + 1)))
      }
    }
    
    if (!response || response.success === undefined) {
      console.warn('metldr: no valid response from background')
      if (!wasReady) {
        ollamaStatus.value = 'not-found'
      }
      return wasReady
    }
    
    const { connected, models } = response
    
    if (connected && models && models.length > 0) {
      ollamaStatus.value = 'ready'
      availableModels.value = models
      
      try {
        const result = await chrome.storage.local.get(['selectedModel'])
        const storedModel = result.selectedModel as string | undefined
        if (storedModel && models.includes(storedModel)) {
          selectedModel.value = storedModel
        } else if (!selectedModel.value || !models.includes(selectedModel.value)) {
          selectedModel.value = models[0]
        }
      } catch {
        if (!selectedModel.value) selectedModel.value = models[0]
      }
      
      if (!wasReady && summaryMode.value === 'auto') {
        await fetchCurrentPageSummary(false, 'auto')
      }
      return true
    }
    
    if (!wasReady) {
      ollamaStatus.value = 'not-found'
    }
    return wasReady
  } catch {
    console.error('metldr: ollama check failed')
    if (!wasReady) {
      ollamaStatus.value = 'not-found'
    }
    return wasReady
  }
}

async function fetchCurrentPageSummary(force = false, trigger = 'auto'): Promise<void> {
  try {
    summaryPrompt.value = null
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true })
    if (!tabs.length) {
      summaryError.value = 'no active tab'
      return
    }
    
    const tab = tabs[0]
    currentTabId.value = tab.id ?? null
    currentTabUrl.value = tab.url ?? null
    
    if (!tab.url || tab.url.startsWith('chrome://') || tab.url.startsWith('edge://') || tab.url.startsWith('about:')) {
      summaryError.value = 'system page'
      return
    }
    
    if (!force) {
      const hasSession = await loadTabSession(tab.url)
      if (hasSession && pageSummary.value) {
        pageMetadata.value = { title: pageSummary.value.title || 'untitled', url: tab.url }
        return
      }
    }
    
    if (force) {
      chatMessages.value = []
    }
    
    console.log('[App] fetchCurrentPageSummary:', { force, trigger, tabId: tab.id, tabUrl: tab.url })
    
    summaryLoading.value = true
    summaryError.value = null
    
    const response = await sendToBackground({
      type: 'EXTRACT_AND_SUMMARIZE',
      tabId: tab.id,
      force,
      trigger
    }) as AppSummaryResponse | null
    
    console.log('[App] EXTRACT_AND_SUMMARIZE response:', response)
    
    if (!response || !response.success) {
      if (response?.prompt) {
        summaryPrompt.value = {
          url: tab.url,
          reason: response.reason || 'needs approval'
        }
        summaryError.value = null
      } else if (response?.skip) {
        summaryError.value = response.reason || 'page skipped'
      } else {
        summaryError.value = response?.error || 'summarisation failed'
      }
      summaryLoading.value = false
      return
    }
    
    pageSummary.value = response.summary || null
    pageMetadata.value = { title: response.summary?.title || 'untitled', url: tab.url }
    summaryError.value = null
    
    await saveTabSession()
    
  } catch (error) {
    console.error('metldr: page summary failed:', error)
    summaryError.value = (error as Error).message || 'unknown error'
  } finally {
    summaryLoading.value = false
  }
}

function triggerManualSummary() {
  fetchCurrentPageSummary(true, 'manual');
}

function acceptSummaryPrompt() {
  if (!summaryPrompt.value) return;
  summaryPrompt.value = null;
  fetchCurrentPageSummary(true, 'manual');
}

function declineSummaryPrompt() {
  summaryError.value = 'summary dismissed';
  summaryPrompt.value = null;
}

function handleKeydown(e: KeyboardEvent): void {
  if (e.ctrlKey && e.shiftKey && e.key?.toLowerCase?.() === 'l') {
    e.preventDefault()
    triggerManualSummary()
  }
}

function switchTab(tab: string): void {
  if (activeTab.value === tab) return
  activeTab.value = tab
}

function onBeforeEnter(el: Element): void {
  const htmlEl = el as HTMLElement
  htmlEl.style.height = '0'
  htmlEl.style.opacity = '0'
}

function onEnter(el: Element, done: () => void): void {
  const htmlEl = el as HTMLElement
  htmlEl.offsetHeight
  htmlEl.style.transition = 'height 250ms cubic-bezier(0.4, 0, 0.2, 1), opacity 200ms ease-out'
  htmlEl.style.height = htmlEl.scrollHeight + 'px'
  htmlEl.style.opacity = '1'
  
  htmlEl.addEventListener('transitionend', function handler(e: TransitionEvent) {
    if (e.propertyName === 'height') {
      htmlEl.removeEventListener('transitionend', handler)
      done()
    }
  })
}

function onAfterEnter(el: Element): void {
  const htmlEl = el as HTMLElement
  htmlEl.style.height = 'auto'
  htmlEl.style.transition = ''
}

function onBeforeLeave(el: Element): void {
  const htmlEl = el as HTMLElement
  htmlEl.style.height = htmlEl.scrollHeight + 'px'
  htmlEl.style.opacity = '1'
}

function onLeave(el: Element, done: () => void): void {
  const htmlEl = el as HTMLElement
  htmlEl.offsetHeight
  htmlEl.style.transition = 'height 200ms cubic-bezier(0.4, 0, 0.2, 1), opacity 150ms ease-in'
  htmlEl.style.height = '0'
  htmlEl.style.opacity = '0'
  
  htmlEl.addEventListener('transitionend', function handler(e: TransitionEvent) {
    if (e.propertyName === 'height') {
      htmlEl.removeEventListener('transitionend', handler)
      done()
    }
  })
}

function onAfterLeave(el: Element): void {
  const htmlEl = el as HTMLElement
  htmlEl.style.height = ''
  htmlEl.style.transition = ''
}

async function retryDetection(): Promise<void> {
  ollamaStatus.value = 'checking'
  await checkOllama(false)
}

function copySetupCommands() {
  navigator.clipboard.writeText(setupCommands).then(() => {
    copiedSetup.value = true;
    setTimeout(() => { copiedSetup.value = false; }, 2000);
  });
}

function openWelcomePage() {
  chrome.tabs.create({ url: chrome.runtime.getURL('welcome.html') });
}

async function sendChatMessage(): Promise<void> {
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
    chatInputRef.value.focus()
  }
  
  chatLoading.value = true
  
  try {
    const model = selectedModel.value || availableModels.value[0]
    
    const recentMessages = chatMessages.value.slice(-6).map(m => ({
      role: m.role,
      content: m.content
    }))
    
    const response = await sendToBackground({
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
    }) as AppChatResponse | null
    
    if (response?.ok && response.content) {
      chatMessages.value.push({ 
        role: 'assistant', 
        content: response.content,
        timing: response.timing
      })
    } else {
      throw new Error(response?.error || 'chat failed')
    }
  } catch (error) {
    console.warn('metldr: chat error:', (error as Error).message)
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
    
    await saveTabSession()
    
    if (chatInputRef.value) {
      chatInputRef.value.focus()
    }
  }
}

function clearChat() {
  chatMessages.value = [];
  saveTabSession();
  nextTick(() => {
    if (chatInputRef.value) {
      chatInputRef.value.focus();
    }
  });
}

async function toggleWordPopup(newValue?: boolean) {
  wordPopupEnabled.value = newValue ?? !wordPopupEnabled.value
  
  try {
    await chrome.storage.local.set({ wordPopupEnabled: wordPopupEnabled.value })
    
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
    console.error('metldr: failed to save word popup setting:', error)
  }
}

async function clearCache() {
  if (!confirm('clear all cached summaries?')) return;
  
  try {
    await storage.cacheClearAll();
    alert('cache cleared!');
    if (historyRef.value) {
      historyRef.value.refresh();
    }
  } catch (error) {
    console.error('failed to clear cache:', error);
    alert('failed to clear cache');
  }
}

async function loadDictionarySettings(): Promise<void> {
  try {
    console.log('[metldr] loading dictionary settings...')
    await storage.initDictionary()
    const persisted = await chrome.storage.local.get(['dictDownloadProgress', 'downloadingLanguages'])
    const persistedProgress = (persisted.dictDownloadProgress || {}) as Record<string, DownloadProgressItem>
    const persistedDownloading = Array.isArray(persisted.downloadingLanguages) ? persisted.downloadingLanguages as string[] : []
    downloadProgress.value = { ...persistedProgress }
    downloadedLanguages.value = await storage.dictGetDownloadedLanguages() as string[]
    console.log('[metldr] downloaded languages:', downloadedLanguages.value)
    
    const settings = await chrome.storage.local.get(['selectedLanguages'])
    if (settings.selectedLanguages && Array.isArray(settings.selectedLanguages) && settings.selectedLanguages.length > 0) {
      selectedLanguages.value = settings.selectedLanguages as string[]
    }
    console.log('[metldr] selected languages:', selectedLanguages.value)
    
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

    if (!downloadedLanguages.value.includes('en')) {
      console.log('[metldr] english not downloaded, starting download...')
      startDownload('en')
    } else {
      console.log('[metldr] english already downloaded')
    }
  } catch (error) {
    console.error('metldr: failed to load dictionary settings:', error)
  }
}

async function startDownload(langCode: string): Promise<void> {
  console.log('[metldr] starting download for:', langCode)
  downloadProgress.value = { ...downloadProgress.value, [langCode]: { progress: 0, letter: 'a', entries: 0 } }
  
  try {
    await storage.dictDownloadLanguage(langCode, (progressData: { progress: number; letter: string; entriesProcessed: number }) => {
      downloadProgress.value = { ...downloadProgress.value, [langCode]: {
        progress: progressData.progress,
        letter: progressData.letter,
        entries: progressData.entriesProcessed
      } }
    })
    
    console.log('[metldr] download completed for:', langCode)
    const { [langCode]: _, ...rest } = downloadProgress.value
    downloadProgress.value = rest
    downloadedLanguages.value = await storage.dictGetDownloadedLanguages() as string[]

    if (!selectedLanguages.value.includes(langCode)) {
      selectedLanguages.value.push(langCode)
      await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
    }
  } catch (error) {
    console.error('[metldr] download failed for', langCode, ':', error)
    const { [langCode]: _, ...rest } = downloadProgress.value
    downloadProgress.value = rest
  }
}

async function toggleLanguage(langCode: string): Promise<void> {
  const index = selectedLanguages.value.indexOf(langCode)
  
  if (index === -1) {
    selectedLanguages.value.push(langCode)
    await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
    
    if (!downloadedLanguages.value.includes(langCode) && !downloadProgress.value[langCode]) {
      startDownload(langCode)
    }
  } else {
    selectedLanguages.value.splice(index, 1)
    await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] })
  }
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
    const index = selectedLanguages.value.indexOf(langCode);
    if (index !== -1) {
      selectedLanguages.value.splice(index, 1);
      await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] });
    }
  } catch (error) {
    console.error('metldr: failed to delete language:', error);
    alert('failed to delete dictionary');
  }
}

watch([summaryMode, allowlistInput, denylistInput, minAutoWords], () => {
  saveSummaryPrefs();
});

function setupTabListener() {
  chrome.tabs.onActivated.addListener(async (activeInfo) => {
    if (ollamaStatus.value === 'ready') {
      await saveTabSession();
      
      pageSummary.value = null;
      pageMetadata.value = null;
      chatMessages.value = [];
      summaryCollapsed.value = false;
      summaryError.value = null;
      summaryPrompt.value = null;
      
      if (summaryMode.value === 'auto') {
        await fetchCurrentPageSummary(false, 'auto');
      }
    }
  });
  
  chrome.tabs.onUpdated.addListener(async (tabId, changeInfo, tab) => {
    if (changeInfo.status === 'complete' && tabId === currentTabId.value && ollamaStatus.value === 'ready') {
      const urlChanged = tab.url !== currentTabUrl.value;
      
      if (urlChanged) {
        await saveTabSession();
        pageSummary.value = null;
        pageMetadata.value = null;
        chatMessages.value = [];
        summaryPrompt.value = null;
        summaryError.value = null;
      }
      
      if (summaryMode.value === 'auto') {
        await fetchCurrentPageSummary(urlChanged, 'auto');
      }
    }
  });
}

function setupStorageListener(): void {
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== 'local') return
    
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
    
    if (changes.selectedModel?.newValue) {
      selectedModel.value = changes.selectedModel.newValue as string
    }
    
    if (changes.selectedLanguages?.newValue) {
      selectedLanguages.value = changes.selectedLanguages.newValue as string[]
    }
  })
}

onMounted(async () => {
  await themeStore.loadSavedTheme();
  
  try {
    const settings = await chrome.storage.local.get(['wordPopupEnabled'])
    if (typeof settings.wordPopupEnabled === 'boolean') {
      wordPopupEnabled.value = settings.wordPopupEnabled
    }
  } catch (error) {
    console.error('metldr: failed to load word popup settings:', error)
  }
  
  await loadSummaryPrefs();
  await loadDictionarySettings();
  
  try {
    await chrome.storage.local.set({ 
      selectedLanguages: selectedLanguages.value?.length > 0 ? selectedLanguages.value : ['en']
    });
  } catch (error) {
    console.error('metldr: failed to save selectedLanguages:', error);
  }
  
  await checkOllama();
  setupTabListener();
  setupStorageListener();

  window.addEventListener('keydown', handleKeydown);

  const dropdownClickHandler = (e: MouseEvent): void => {
    const target = e.target as HTMLElement
    if (showModelDropdown.value && !target.closest('.model-selector-btn') && !target.closest('.model-dropdown')) {
      showModelDropdown.value = false
    }
  }

  document.addEventListener('click', dropdownClickHandler);

  const statusCheckInterval = setInterval(async () => {
    if (ollamaStatus.value !== 'ready') {
      await checkOllama(false); 
    }
  }, 5000);

  onUnmounted(() => {
    clearInterval(statusCheckInterval);
    document.removeEventListener('click', dropdownClickHandler);
    window.removeEventListener('keydown', handleKeydown);
  });
});
</script>

<template>
  <div class="h-screen flex flex-col bg-background text-foreground overflow-hidden">
    <!-- teleport target for dropdowns -->
    <div id="dropdown-portal" class="fixed inset-0 pointer-events-none z-50"></div>

    <!-- header with tabs -->
    <header v-if="ollamaStatus === 'ready'" class="shrink-0 sticky top-0 z-10 border-b border-border">
      <div class="flex gap-1.5 p-2 bg-background/95 backdrop-blur-lg">
        <button 
          v-for="(tab, idx) in [{key: 'summary', label: 'Summary', icon: FileText, color: 'primary'}, {key: 'stats', label: 'Stats', icon: BarChart3, color: 'secondary'}, {key: 'settings', label: 'Settings', icon: Settings, color: 'accent'}]"
          :key="tab.key"
          @click="switchTab(tab.key)"
          class="flex-1 flex items-center justify-center gap-1.5 py-2 px-3 text-[11px] font-medium rounded-lg transition-all duration-200"
          :class="[
            activeTab === tab.key 
              ? `bg-${tab.color}/25 text-${tab.color} border border-${tab.color}/40` 
              : 'text-foreground/70 hover:text-foreground hover:bg-muted border border-transparent'
          ]"
        >
          <component 
            :is="tab.icon" 
            :size="13" 
            :stroke-width="2" 
            :class="activeTab === tab.key ? `text-${tab.color}` : ''"
          />
          {{ tab.label }}
        </button>
      </div>
    </header>

    <!-- main content -->
    <main class="flex-1 overflow-hidden">
      <Transition name="fade" mode="out-in">
        <!-- checking state -->
        <div v-if="ollamaStatus === 'checking'" class="flex flex-col items-center justify-center h-full p-6">
          <Loader2 class="w-8 h-8 mb-3 animate-spin text-primary" :stroke-width="2" />
          <p class="text-[12px] text-foreground/70">connecting to ollama...</p>
        </div>

        <!-- not found state - assume user has ollama, show how to start -->
        <div v-else-if="ollamaStatus === 'not-found'" class="p-4 h-full overflow-y-auto">
          <div class="rounded-xl bg-linear-to-br from-primary/10 via-card to-secondary/5 p-5 border border-primary/20 shadow-lg">
            <!-- header -->
            <div class="flex items-center gap-3 mb-5">
              <div class="w-10 h-10 rounded-xl bg-primary/20 flex items-center justify-center shrink-0 ring-2 ring-primary/10">
                <Zap :size="20" class="text-primary" />
              </div>
              <div>
                <h2 class="text-[15px] font-semibold text-foreground">start ollama</h2>
                <p class="text-[11px] text-foreground/60">run this command to connect</p>
              </div>
            </div>
            
            <!-- main command -->
            <div class="bg-background/80 backdrop-blur-sm rounded-lg p-4 border border-border mb-4">
              <div class="flex items-center justify-between mb-2">
                <span class="text-[10px] font-medium text-foreground/50 uppercase tracking-wide">
                  {{ isWindows ? 'powershell' : 'terminal' }}
                </span>
                <button 
                  @click="copySetupCommands" 
                  class="text-[10px] text-primary hover:text-primary/80 transition-colors flex items-center gap-1"
                >
                  <Check v-if="copiedSetup" :size="10" />
                  <span>{{ copiedSetup ? 'copied!' : 'copy' }}</span>
                </button>
              </div>
              <pre class="font-mono text-[12px] text-foreground whitespace-pre-wrap leading-relaxed">{{ setupCommands }}</pre>
            </div>
            
            <!-- auto-retry indicator -->
            <div class="flex items-center gap-2 mb-4 px-1">
              <div class="relative flex items-center justify-center">
                <Circle :size="8" class="text-primary animate-pulse" fill="currentColor" />
                <div class="absolute inset-0 rounded-full bg-primary/30 animate-ping"></div>
              </div>
              <span class="text-[10px] text-foreground/60">auto-detecting ollama...</span>
            </div>
            
            <!-- retry button -->
            <Button 
              @click="retryDetection" 
              variant="outline"
              class="w-full mb-4 h-9 text-[12px] border-primary/30 hover:bg-primary/10 hover:border-primary/50"
            >
              <RefreshCw :size="13" class="mr-2" />
              check connection now
            </Button>
            
            <!-- fallback section -->
            <div class="pt-4 border-t border-border/50">
              <p class="text-[11px] text-foreground/50 text-center">
                don't have ollama installed?
                <button 
                  @click="openWelcomePage" 
                  class="text-primary hover:text-primary/80 hover:underline transition-colors font-medium ml-1"
                >
                  view setup guide →
                </button>
              </p>
            </div>
          </div>
        </div>

        <!-- summary tab -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'summary'" :class="['flex flex-col h-full relative', summaryPrompt ? 'pb-16' : '']">
          
          <div v-if="summaryPrompt" class="absolute right-2 bottom-2 max-w-xs rounded-lg border border-warning/50 bg-warning/10 px-3 py-2 text-[10px] shadow-lg z-10">
            <div class="flex items-start gap-2">
              <div class="flex-1 min-w-0">
                <div class="font-medium text-foreground truncate">summarise this page?</div>
                <div class="text-foreground/70 truncate">{{ summaryPrompt.reason }}</div>
              </div>
              <div class="flex gap-1 shrink-0">
                <button class="px-2 py-1 rounded text-[10px] text-foreground/70 hover:bg-muted transition-colors" @click="declineSummaryPrompt">no</button>
                <button class="px-2 py-1 rounded text-[10px] bg-warning/30 border border-warning/50 text-warning-foreground hover:bg-warning/40 transition-colors" @click="acceptSummaryPrompt">yes</button>
              </div>
            </div>
          </div>
          
          <!-- collapsible summary area -->
          <div class="shrink-0 p-2 pb-0">
            <!-- summary card with collapse toggle -->
            <div v-if="pageSummary" class="rounded-lg bg-card border border-border">
              <!-- header (always visible) -->
              <div 
                class="flex items-center gap-2 p-2.5 cursor-pointer hover:bg-primary/5 rounded-t-lg transition-colors"
                @click="summaryCollapsed = !summaryCollapsed"
              >
                <component 
                  :is="summaryCollapsed ? ChevronDown : ChevronUp" 
                  :size="11" 
                  class="text-primary/60 shrink-0 transition-transform"
                />
                <div class="flex-1 min-w-0">
                  <h3 class="text-[12px] font-medium text-foreground truncate leading-tight">
                    {{ pageSummary.title || 'untitled' }}
                  </h3>
                  <p v-if="summaryCollapsed" class="text-[10px] text-foreground/60 truncate">
                    {{ pageSummary.bullets.length }} key points · {{ pageSummary.readTime || 'n/a' }} read
                  </p>
                </div>
                <button 
                  @click.stop="pageSummary ? fetchCurrentPageSummary(true, 'manual') : triggerManualSummary()" 
                  class="flex items-center justify-center w-7 h-7 rounded-md hover:bg-primary/10 shrink-0 transition-colors"
                  :disabled="summaryLoading"
                  :title="pageSummary ? 'regenerate summary' : 'summarise now (Ctrl+Shift+L)'"
                >
                  <component 
                    :is="pageSummary ? RefreshCw : Zap" 
                    :size="12" 
                    :class="[pageSummary && summaryLoading ? 'animate-spin' : '', 'text-primary']" 
                  />
                </button>
              </div>
              
              <Transition
                @before-enter="onBeforeEnter"
                @enter="onEnter"
                @after-enter="onAfterEnter"
                @before-leave="onBeforeLeave"
                @leave="onLeave"
                @after-leave="onAfterLeave"
              >
                <div v-if="!summaryCollapsed" class="summary-content overflow-hidden">
                  <div class="px-3 pb-3 pt-1 border-t border-border">
                    <!-- metadata -->
                    <p v-if="pageSummary.publication || pageSummary.author" class="text-[10px] text-foreground/60 mb-2">
                      {{ pageSummary.publication }}{{ pageSummary.author ? ` · ${pageSummary.author}` : '' }}
                    </p>
                    
                    <!-- bullets -->
                    <ul class="space-y-1.5">
                      <li v-for="(bullet, i) in pageSummary.bullets" :key="i" class="flex gap-2 text-[11px] leading-relaxed">
                        <span class="text-primary shrink-0 mt-0.5">•</span>
                        <span class="text-foreground markdown-content" v-html="renderMarkdown(bullet)"></span>
                      </li>
                    </ul>
                    
                    <div class="flex items-center justify-between pt-2 mt-2 border-t border-border">
                      <span class="text-[10px] text-foreground/60">{{ pageSummary.readTime || 'n/a' }} read</span>
                      <div 
                        v-if="pageSummary.timing"
                        class="timing-badge group relative cursor-help"
                      >
                        <div class="flex items-center gap-1 text-[10px] text-foreground/60 hover:text-foreground transition-colors">
                          <span>{{ formatTime(pageSummary.timing.total) }}</span>
                          <span v-if="pageSummary.timing.cached" class="text-primary">· cached</span>
                        </div>
                        <!-- hover breakdown tooltip -->
                        <div class="timing-tooltip absolute bottom-full right-0 mb-2 px-2.5 py-2 rounded-lg bg-popover border border-border shadow-lg opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-150 z-50 min-w-[140px]">
                          <div class="text-[10px] font-medium text-foreground mb-1.5 uppercase tracking-wide">time breakdown</div>
                          <div class="space-y-1 text-[10px]">
                            <div class="flex justify-between gap-3">
                              <span class="text-foreground/70">extraction</span>
                              <span class="text-foreground font-mono">{{ formatTime(pageSummary.timing.extraction || 0) }}</span>
                            </div>
                            <div class="flex justify-between gap-3">
                              <span class="text-foreground/70">llm</span>
                              <span class="text-foreground font-mono">{{ formatTime(pageSummary.timing.llm || 0) }}</span>
                            </div>
                            <div class="flex justify-between gap-3 pt-1 border-t border-border">
                              <span class="text-foreground">total</span>
                              <span class="text-primary font-mono font-medium">{{ formatTime(pageSummary.timing.total) }}</span>
                            </div>
                            <div v-if="pageSummary.timing.model" class="pt-1 text-foreground/60 truncate">
                              {{ pageSummary.timing.model }}
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              </Transition>
            </div>

            <!-- loading state -->
            <div v-else-if="summaryLoading" class="flex items-center gap-2.5 p-3 rounded-lg bg-muted border border-border">
              <Loader2 class="w-4 h-4 animate-spin text-primary" />
              <span class="text-[11px] text-foreground">analysing...</span>
            </div>

            <!-- error state with retry button -->
            <div v-else-if="summaryError" class="space-y-2">
              <div class="flex items-center gap-2.5 p-3 rounded-lg bg-destructive/15 border border-destructive/30">
                <AlertCircle :size="13" class="text-destructive shrink-0" />
                <div class="flex-1 min-w-0">
                  <p class="text-[11px] text-foreground">{{ summaryError }}</p>
                </div>
              </div>
              <!-- show retry button on nonemail pages -->
              <Button 
                v-if="!isEmailClient"
                @click="triggerManualSummary"
                :disabled="summaryLoading"
                class="w-full text-[11px] text-muted-foreground hover:text-foreground hover:bg-muted/60"
                variant="ghost"
              >
                <Zap :size="14" class="mr-2" />
                try summarising anyway
              </Button>
            </div>

            <!-- no page state with summarise button -->
            <div v-else class="space-y-2">
              <div v-if="isEmailClient" class="flex items-center gap-2.5 p-3 rounded-lg bg-muted border border-border">
                <FileText :size="13" class="text-foreground/50 shrink-0" />
                <p class="text-[11px] text-foreground">browse a page to get a summary</p>
              </div>
              <Button 
                v-else
                @click="triggerManualSummary"
                :disabled="summaryLoading"
                class="w-full"
                variant="outline"
              >
                <Zap :size="14" class="mr-2" />
                summarise this page
              </Button>
            </div>
          </div>

          <!-- chat ui -->
          <div class="flex-1 flex flex-col min-h-0 p-2">
            <div class="flex-1 flex flex-col rounded-lg bg-card border border-border p-3 min-h-0">
              <div class="flex items-center justify-between mb-2 shrink-0">
                <div class="flex items-center gap-1.5">
                  <MessageCircle :size="11" class="text-secondary" />
                  <span class="text-[10px] font-medium text-secondary uppercase tracking-wide">chat</span>
                </div>
                <button 
                  v-if="chatMessages.length > 0"
                  @click="clearChat" 
                  class="text-[10px] text-foreground/60 hover:text-destructive transition-colors"
                >
                  clear
                </button>
              </div>
              
              <!-- messages container -->
              <ScrollArea 
                ref="chatContainer"
                class="flex-1 min-h-0"
              >
                <div class="space-y-2 pr-3">
                <!-- empty state -->
                <div v-if="chatMessages.length === 0 && !chatLoading" class="flex items-center justify-center h-full">
                  <p class="text-[10px] text-foreground/50">ask about the article</p>
                </div>
                
                <!-- messages -->
                <div v-for="(msg, i) in chatMessages" :key="i" class="message-wrapper">
                  <div v-if="msg.role === 'user'" class="flex justify-end">
                    <div class="max-w-[85%] bg-primary/25 border border-primary/35 rounded-xl rounded-br-sm px-2.5 py-1.5 text-[11px] text-foreground">
                      {{ msg.content }}
                    </div>
                  </div>
                  <div v-else class="flex flex-col items-start max-w-[85%]">
                    <div class="bg-muted border border-border rounded-xl rounded-bl-sm px-2.5 py-1.5 text-[11px] text-foreground markdown-content" v-html="renderMarkdown(msg.content)">
                    </div>
                    <div 
                      v-if="msg.timing" 
                      class="flex items-center gap-1 mt-0.5 ml-1 text-[9px] text-foreground/50 hover:text-foreground/70 transition-colors cursor-default group relative"
                    >
                      <span>{{ formatTime(msg.timing.total) }}</span>
                      <div class="chat-timing-tooltip absolute left-full top-1/2 -translate-y-1/2 ml-2 px-2 py-1.5 rounded bg-popover border border-border shadow-md opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-150 z-50 whitespace-nowrap">
                        <div class="text-[9px] text-foreground/70">
                          <span class="text-foreground">{{ formatTime(msg.timing.total) }}</span>
                          <span v-if="msg.timing.model" class="text-foreground/60"> · {{ msg.timing.model }}</span>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
                
                <!-- loading -->
                <div v-if="chatLoading" class="flex justify-start">
                  <div class="bg-muted border border-border rounded-xl rounded-bl-sm px-3 py-2">
                    <div class="flex gap-1">
                      <span class="w-1.5 h-1.5 bg-secondary rounded-full animate-bounce" style="animation-delay: 0ms"></span>
                      <span class="w-1.5 h-1.5 bg-secondary rounded-full animate-bounce" style="animation-delay: 150ms"></span>
                      <span class="w-1.5 h-1.5 bg-secondary rounded-full animate-bounce" style="animation-delay: 300ms"></span>
                    </div>
                  </div>
                </div>
                </div>
              </ScrollArea>

              <!-- input -->
              <div class="flex items-center gap-2 mt-2 shrink-0">
                <Input 
                  ref="chatInputRef"
                  v-model="chatInput"
                  @keydown.enter.prevent="sendChatMessage"
                  type="text"
                  placeholder="ask something..."
                  class="flex-1 h-9 text-[11px]"
                />
                <Button 
                  @click="sendChatMessage"
                  :disabled="!chatInput.trim() || chatLoading"
                  size="icon"
                  class="h-9 w-9 bg-primary/15 hover:bg-primary/25 border border-primary/30 text-primary"
                  variant="ghost"
                >
                  <Send :size="14" />
                </Button>
              </div>
            </div>
          </div>
        </div>

        <!-- stats tab -->
        <ScrollArea v-else-if="ollamaStatus === 'ready' && activeTab === 'stats'" class="h-full">
          <div class="p-3">
            <HistoryManager ref="historyRef" :limit="10" />
          </div>
        </ScrollArea>

        <!-- settings tab -->
        <ScrollArea v-else-if="ollamaStatus === 'ready' && activeTab === 'settings'" class="h-full">
          <div class="p-3 space-y-3">
          <!-- model selection -->
          <div class="rounded-xl bg-card p-4 border border-border">
            <div class="flex items-center gap-2.5 mb-3">
              <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/25">
                <Server :size="12" class="text-primary" />
              </div>
              <span class="text-[12px] font-medium text-foreground tracking-wide">ai model</span>
            </div>
            
            <div class="relative">
              <button 
                @click="toggleModelDropdown"
                class="model-selector-btn w-full flex items-center justify-between px-3 py-2.5 bg-muted hover:bg-muted/80 rounded-lg border border-input transition-colors"
              >
                <span class="font-mono text-[12px] text-foreground">{{ selectedModel }}</span>
                <ChevronDown 
                  :size="14" 
                  class="text-foreground/60 transition-transform"
                  :class="{ 'rotate-180': showModelDropdown }"
                />
              </button>

              <Teleport to="#dropdown-portal">
                <Transition
                  enter-active-class="transition-all duration-150"
                  enter-from-class="opacity-0 scale-95"
                  enter-to-class="opacity-100 scale-100"
                  leave-active-class="transition-all duration-100"
                  leave-from-class="opacity-100 scale-100"
                  leave-to-class="opacity-0 scale-95"
                >
                  <ScrollArea 
                    v-if="showModelDropdown" 
                    class="model-dropdown fixed rounded-lg bg-background border border-border shadow-xl max-h-48 pointer-events-auto"
                    :style="{ 
                      top: modelDropdownPos.top + 'px', 
                      left: modelDropdownPos.left + 'px', 
                      width: modelDropdownPos.width + 'px'
                    }"
                  >
                    <div class="py-1">
                    <button 
                      v-for="model in availableModels" 
                      :key="model"
                      @click="selectModel(model)"
                      class="w-full flex items-center justify-between px-3 py-2 text-[12px] font-mono hover:bg-muted transition-colors"
                      :class="{ 'bg-muted': selectedModel === model }"
                    >
                      <span class="text-foreground">{{ model }}</span>
                      <Check v-if="selectedModel === model" :size="14" class="text-primary" />
                    </button>
                    </div>
                  </ScrollArea>
                </Transition>
              </Teleport>
            </div>
          </div>

          <!-- summary preferences -->
          <div class="rounded-xl bg-card p-4 border border-border space-y-3">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2.5">
                <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/25">
                  <FileText :size="12" class="text-primary" />
                </div>
                <span class="text-[12px] font-medium text-foreground tracking-wide">auto-summarise</span>
              </div>
              <Toggle 
                :model-value="summaryMode === 'auto'"
                @update:model-value="(v: boolean) => summaryMode = v ? 'auto' : 'manual'"
              />
            </div>
            <p class="text-[10px] text-foreground/60">when enabled, pages matching the allowlist will be summarised automatically.</p>

            <div v-if="summaryMode === 'auto'" class="space-y-3 pt-2 border-t border-border">
              <label class="text-[11px] text-foreground/80 flex items-center gap-2">
                <span class="shrink-0 text-[10px] w-20">min words</span>
                <Input type="number" min="0" class="w-full h-7 text-[11px]" v-model.number="minAutoWords" />
              </label>

              <div class="grid grid-cols-2 gap-3">
                <div>
                  <div class="flex items-center justify-between mb-1">
                    <span class="text-[11px] text-foreground/80">allowlist</span>
                    <span class="text-[10px] text-foreground/50">one per line</span>
                  </div>
                  <Textarea 
                    class="w-full h-28 text-[11px] resize-none"
                    v-model="allowlistInput"
                    placeholder="example.com"
                  />
                </div>
                <div>
                  <div class="flex items-center justify-between mb-1">
                    <span class="text-[11px] text-foreground/80">denylist</span>
                    <span class="text-[10px] text-foreground/50">one per line</span>
                  </div>
                  <Textarea 
                    class="w-full h-28 text-[11px] resize-none"
                    v-model="denylistInput"
                    placeholder="dashboard"
                  />
                </div>
              </div>
            </div>
          </div>

          <!-- word lookup toggle -->
          <div class="rounded-xl bg-card p-4 border border-border">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2.5 group relative cursor-help" title="show definitions on text selection">
                <div class="flex items-center justify-center w-6 h-6 rounded-md bg-secondary/25">
                  <Sparkles :size="12" class="text-secondary" />
                </div>
                <span class="text-[12px] font-medium text-foreground tracking-wide">word lookup</span>
              </div>
              <Toggle 
                :model-value="wordPopupEnabled"
                @update:model-value="toggleWordPopup"
              />
            </div>
          </div>

          <!-- dictionaries -->
          <div class="rounded-xl bg-card p-4 border border-border">
            <div class="flex items-center gap-2.5 mb-3">
              <div class="flex items-center justify-center w-6 h-6 rounded-md bg-accent/25">
                <Database :size="12" class="text-accent" />
              </div>
              <span class="text-[12px] font-medium text-foreground tracking-wide">dictionaries</span>
            </div>
            
            <ScrollArea class="h-48 w-full">
              <div class="space-y-1 pr-4">
                <label 
                  v-for="lang in SUPPORTED_LANGUAGES" 
                  :key="lang.code" 
                  class="flex items-center gap-2.5 py-1.5 cursor-pointer hover:bg-muted/50 rounded-lg px-2 -mx-2 transition-colors"
                >
                  <Checkbox
                    :checked="selectedLanguages.includes(lang.code)"
                    @update:checked="toggleLanguage(lang.code)"
                    class="h-4 w-4"
                  />
                  <span class="text-[12px] text-foreground/80 flex-1">{{ lang.name }}</span>
                  
                  <span 
                    v-if="downloadProgress[lang.code]"
                    class="text-[10px] text-foreground/60"
                  >
                    {{ Number(downloadProgress[lang.code].progress || 0).toFixed(0) }}%
                  </span>
                  
                  <button
                    v-if="downloadedLanguages.includes(lang.code)"
                    @click.stop="deleteLanguageData(lang.code)"
                    class="p-1 rounded hover:bg-muted text-foreground/40 hover:text-destructive transition-colors"
                  >
                    <X :size="12" />
                  </button>
                </label>
              </div>
            </ScrollArea>
          </div>

          <!-- theme -->
          <div class="rounded-xl bg-card p-4 border border-border">
            <div class="flex items-center gap-2.5 mb-3">
              <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/20">
                <Sparkles :size="12" class="text-primary" />
              </div>
              <span class="text-[12px] font-medium text-foreground tracking-wide">theme</span>
            </div>
            <div class="grid grid-cols-3 gap-2">
              <button
                v-for="(themeData, key) in themeStore.themes"
                :key="key"
                @click="themeStore.setTheme(key)"
                class="flex flex-col items-center gap-1.5 p-2 rounded-lg transition-all"
                :class="[
                  themeStore.currentTheme === key 
                    ? 'bg-muted ring-1 ring-border' 
                    : 'hover:bg-muted/50'
                ]"
              >
                <div class="flex gap-0.5">
                  <div class="w-3 h-3 rounded-full" :style="{ background: themeData.primary }"></div>
                  <div class="w-3 h-3 rounded-full" :style="{ background: themeData.secondary }"></div>
                  <div class="w-3 h-3 rounded-full" :style="{ background: themeData.accent }"></div>
                </div>
                <span class="text-[10px] text-foreground/70">{{ themeData.name }}</span>
              </button>
            </div>
          </div>

          <div 
            class="rounded-xl bg-card p-4 border border-border group cursor-help"
            title="clear cached summaries"
          >
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2.5">
                <div class="flex items-center justify-center w-6 h-6 rounded-md bg-destructive/20">
                  <Trash2 :size="12" class="text-destructive" />
                </div>
                <span class="text-[12px] font-medium text-foreground tracking-wide">cache</span>
              </div>
              <button @click="clearCache" class="px-2 py-1 rounded text-[11px] text-destructive/70 hover:text-destructive hover:bg-destructive/10 transition-colors">
                clear
              </button>
            </div>
          </div>
          </div>
        </ScrollArea>

        <!-- error state -->
        <div v-else-if="ollamaStatus === 'error'" class="flex flex-col items-center justify-center h-full p-6">
          <X :size="32" class="mb-3 text-destructive/50" />
          <p class="text-[12px] text-foreground/70 mb-3">connection error</p>
          <button @click="retryDetection" class="px-3 py-1.5 rounded-md text-[11px] bg-primary text-primary-foreground hover:bg-primary/90 transition-colors flex items-center gap-1.5">
            <RefreshCw :size="12" />
            retry
          </button>
        </div>
      </Transition>
    </main>
  </div>
</template>

<style scoped>
.model-dropdown {
  z-index: 9999 !important;
}

.markdown-content :deep(strong),
.markdown-content :deep(b) {
  font-weight: 600;
  color: oklch(from var(--bc) calc(l + 0.1) c h);
}

.markdown-content :deep(em),
.markdown-content :deep(i) {
  font-style: italic;
}

.markdown-content :deep(code) {
  background: oklch(from var(--bc) l c h / 0.1);
  padding: 0.1em 0.3em;
  border-radius: 3px;
  font-family: ui-monospace, monospace;
  font-size: 0.9em;
}

.markdown-content :deep(a) {
  color: oklch(from var(--p) l c h);
  text-decoration: underline;
  text-underline-offset: 2px;
}

.markdown-content :deep(a:hover) {
  opacity: 0.8;
}

.fade-enter-active,
.fade-leave-active {
  transition: all 150ms ease;
}

.fade-enter-from {
  opacity: 0;
  transform: translateY(4px);
}

.fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.btn {
  transition: all 100ms ease;
}

.timing-tooltip::after {
  content: '';
  position: absolute;
  top: 100%;
  right: 12px;
  border: 5px solid transparent;
  border-top-color: oklch(from var(--b2) l c h);
}

.chat-timing-tooltip::before {
  content: '';
  position: absolute;
  right: 100%;
  top: 50%;
  transform: translateY(-50%);
  border: 4px solid transparent;
  border-right-color: oklch(from var(--b2) l c h);
}

.summary-content {
  will-change: height, opacity;
}

.message-wrapper {
  animation: messageSlideIn 200ms ease-out;
}

@keyframes messageSlideIn {
  from {
    opacity: 0;
    transform: translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
