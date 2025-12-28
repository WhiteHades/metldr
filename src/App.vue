<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useThemeStore } from '@/stores/theme'
import { chromeAILogger } from '@/utils/chromeAILogger'

// Composables
import { 
  useOllama, 
  useChromeAI, 
  usePageSummary, 
  useChat, 
  useSettings, 
  useDictionary, 
  useTabSession,
  useDropdown
} from '@/composables'

// Components
import HistoryManager from '@/components/HistoryManager.vue'
import SummaryCard from '@/components/SummaryCard.vue'
import ChatPanel from '@/components/ChatPanel.vue'
import OllamaSetup from '@/components/OllamaSetup.vue'
import SettingsPanel from '@/components/SettingsPanel.vue'
import HoverRevealNav from '@/components/HoverRevealNav.vue'

import { ScrollArea } from '@/components/ui'
import { Loader2, RefreshCw, X, FileText, BarChart3, Settings } from 'lucide-vue-next'

// Theme store
const themeStore = useThemeStore()

// Debug logger
chromeAILogger.info('Side panel loaded - run chromeAILogger.runDiagnostic() for Chrome AI debug')

// ===== COMPOSABLES =====

// Ollama
const { 
  ollamaStatus, 
  availableModels, 
  selectedModel, 
  checkOllama, 
  selectModel 
} = useOllama()

// Chrome AI
const { chromeAIStatus, checkChromeAI } = useChromeAI()

// Page Summary
const {
  pageSummary,
  pageMetadata,
  summaryLoading,
  summaryError,
  currentTabId,
  currentTabUrl,
  summaryPrompt,
  summaryCollapsed,
  isEmailClient,
  isViewingEmailThread,
  chatDisabled,
  fetchCurrentPageSummary,
  resetSummaryState,
  refreshCurrentTabUrl
} = usePageSummary()

// Chat
const {
  chatMessages,
  chatInput,
  chatLoading,
  sendChatMessage: baseSendChatMessage,
  clearChat: baseClearChat,
  resetChatState
} = useChat()

// Settings
const {
  summaryMode,
  allowlistInput,
  denylistInput,
  minAutoWords,
  wordPopupEnabled,
  preferredProvider,
  loadSummaryPrefs,
  loadWordPopupSetting,
  toggleWordPopup,
  loadProviderPreference,
  setProviderPreference,
  loadFontSize,
  setFontSize,
  fontSize,
  setupSettingsWatcher,
  setupStorageListener
} = useSettings()

// Dictionary
const {
  downloadedLanguages,
  selectedLanguages,
  downloadProgress,
  storage,
  loadDictionarySettings,
  toggleLanguage,
  deleteLanguageData,
  initSelectedLanguages,
  updateSelectedLanguages
} = useDictionary()

// Tab Session
const { saveTabSession, setupTabListener } = useTabSession()

// Dropdown
const {
  showModelDropdown,
  modelDropdownPos,
  toggleModelDropdown,
  closeDropdown,
  setupDropdownClickHandler
} = useDropdown()

// ===== LOCAL STATE =====

const activeTab = ref<string>('summary')
const historyRef = ref<InstanceType<typeof HistoryManager> | null>(null)
const chatPanelRef = ref<InstanceType<typeof ChatPanel> | null>(null)
const navOpen = ref(false)

const aiReady = computed(() => {
  if (preferredProvider.value === 'chrome-ai') {
    return chromeAIStatus.value === 'available' || chromeAIStatus.value === 'downloadable'
  }
  return ollamaStatus.value === 'ready'
})

const aiChecking = computed(() => {
  if (preferredProvider.value === 'chrome-ai') {
    return chromeAIStatus.value === 'checking'
  }
  return ollamaStatus.value === 'checking'
})

// ===== HELPER FUNCTIONS =====

// Wrapper for saveTabSession
async function doSaveTabSession(): Promise<void> {
  await saveTabSession(currentTabUrl, chatMessages, pageSummary, summaryCollapsed)
}

// Wrapper for fetchCurrentPageSummary
async function doFetchSummary(force = false, trigger = 'auto'): Promise<void> {
  await fetchCurrentPageSummary(force, trigger, doSaveTabSession)
}

// Trigger manual summary
function triggerManualSummary(): void {
  doFetchSummary(true, 'manual')
}

// Accept summary prompt
function acceptSummaryPrompt(): void {
  if (!summaryPrompt.value) return
  summaryPrompt.value = null
  doFetchSummary(true, 'manual')
}

// Decline summary prompt
function declineSummaryPrompt(): void {
  summaryError.value = 'summary dismissed'
  summaryPrompt.value = null
}

async function sendChatMessage(): Promise<void> {
  await baseSendChatMessage(
    pageSummary,
    selectedModel,
    availableModels,
    ref(null),
    ref(null),
    doSaveTabSession,
    preferredProvider
  )
  chatPanelRef.value?.focusInput()
}

function clearChat(): void {
  baseClearChat(undefined, doSaveTabSession)
  chatPanelRef.value?.focusInput()
}

// Select model and close dropdown
async function handleSelectModel(model: string): Promise<void> {
  await selectModel(model)
  closeDropdown()
}

// Retry Ollama detection
async function retryDetection(): Promise<void> {
  ollamaStatus.value = 'checking'
  await checkOllama(false)
}

// Open welcome page
function openWelcomePage(): void {
  chrome.tabs.create({ url: chrome.runtime.getURL('welcome.html') })
}

// Clear cache
async function clearCache(): Promise<void> {
  if (!confirm('clear all cached summaries?')) return
  
  try {
    await storage.cacheClearAll()
    alert('cache cleared!')
    if (historyRef.value) {
      historyRef.value.refresh()
    }
  } catch (error) {
    console.error('failed to clear cache:', error)
    alert('failed to clear cache')
  }
}

// Switch tab
function switchTab(tab: string): void {
  if (activeTab.value === tab) return
  activeTab.value = tab
}

// Keyboard shortcuts
function handleKeydown(e: KeyboardEvent): void {
  if (e.ctrlKey && e.shiftKey && e.key?.toLowerCase?.() === 'l') {
    e.preventDefault()
    triggerManualSummary()
  }
}

// ===== LIFECYCLE =====

let cleanupTabListener: (() => void) | null = null
let cleanupDropdownHandler: (() => void) | null = null
let statusCheckInterval: ReturnType<typeof setInterval> | null = null

onMounted(async () => {
  await themeStore.loadSavedTheme()
  
  // Load settings
  await loadWordPopupSetting()
  await loadSummaryPrefs()
  await loadProviderPreference()
  await loadFontSize()
  await loadDictionarySettings()
  await initSelectedLanguages()
  
  // Check AI providers
  await checkOllama()
  await checkChromeAI()
  
  // Setup watchers and listeners
  setupSettingsWatcher()
  setupStorageListener(
    (model) => { selectedModel.value = model },
    (langs) => { updateSelectedLanguages(langs) }
  )
  
  // Setup tab listener - handles session save/load and tab changes automatically
  cleanupTabListener = setupTabListener(
    currentTabId,
    currentTabUrl,
    chatMessages,
    pageSummary,
    summaryCollapsed,
    aiReady,
    summaryMode,
    doFetchSummary
  )
  
  // Keyboard listener
  window.addEventListener('keydown', handleKeydown)
  
  // Dropdown click handler
  cleanupDropdownHandler = setupDropdownClickHandler()
  
  // Status check interval - always check so we detect disconnections
  statusCheckInterval = setInterval(async () => {
    const wasOllamaReady = ollamaStatus.value === 'ready'
    await checkOllama(false)
    
    // auto-fallback to Chrome AI if Ollama disconnected while selected
    if (wasOllamaReady && ollamaStatus.value !== 'ready' && preferredProvider.value === 'ollama') {
      console.log('[App] Ollama disconnected, falling back to Chrome AI')
      setProviderPreference('chrome-ai')
    }
  }, 5000)
})

onUnmounted(() => {
  if (statusCheckInterval) clearInterval(statusCheckInterval)
  if (cleanupTabListener) cleanupTabListener()
  if (cleanupDropdownHandler) cleanupDropdownHandler()
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div class="h-screen flex flex-col bg-background text-foreground overflow-hidden">
    <!-- teleport target for dropdowns -->
    <div id="dropdown-portal" class="fixed inset-0 pointer-events-none z-50"></div>

    <!-- hover-reveal navigation -->
    <HoverRevealNav 
      v-if="aiReady"
      :active-tab="activeTab"
      :tabs="[
        { key: 'summary', icon: FileText, label: 'Summary' },
        { key: 'stats', icon: BarChart3, label: 'Stats' },
        { key: 'settings', icon: Settings, label: 'Settings' }
      ]"
      @switch="switchTab"
      @open="navOpen = true"
      @close="navOpen = false"
    />

    <!-- main content -->
    <main 
      class="flex-1 overflow-hidden"
      :class="navOpen ? 'content-down' : 'content-up'"
    >
      <Transition name="fade" mode="out-in">
        <!-- checking state -->
        <div v-if="aiChecking" class="flex flex-col items-center justify-center h-full p-6">
          <Loader2 class="w-8 h-8 mb-3 animate-spin text-primary" :stroke-width="2" />
          <p class="text-[12px] text-foreground/70">connecting to ai...</p>
        </div>

        <!-- not found state (only if ollama preferred and not found) -->
        <OllamaSetup 
          v-else-if="preferredProvider === 'ollama' && ollamaStatus === 'not-found'"
          @retry="retryDetection"
          @open-welcome="openWelcomePage"
        />

        <!-- summary tab -->
        <div 
          v-else-if="aiReady && activeTab === 'summary'" 
          :class="['flex flex-col h-full min-h-0 relative', summaryPrompt ? 'pb-16' : '']"
        >
          <!-- Summary card (hidden on gmail threads) -->
          <SummaryCard 
            v-if="!isViewingEmailThread"
            :page-summary="pageSummary"
            :summary-loading="summaryLoading"
            :summary-error="summaryError"
            :summary-prompt="summaryPrompt"
            :summary-collapsed="summaryCollapsed"
            :is-email-client="isEmailClient"
            :is-viewing-email-thread="isViewingEmailThread"
            @update:collapsed="(v) => summaryCollapsed = v"
            @refresh="() => doFetchSummary(true, 'manual')"
            @manual-summary="triggerManualSummary"
            @accept-prompt="acceptSummaryPrompt"
            @decline-prompt="declineSummaryPrompt"
          />

          <!-- Chat panel -->
          <ChatPanel 
            ref="chatPanelRef"
            v-model:chat-input="chatInput"
            :chat-messages="chatMessages"
            :chat-loading="chatLoading"
            :chat-disabled="chatDisabled"
            :is-viewing-email-thread="isViewingEmailThread"
            @send="sendChatMessage"
            @clear="clearChat"
          />
        </div>

        <!-- stats tab -->
        <ScrollArea v-else-if="aiReady && activeTab === 'stats'" class="h-full">
          <div class="p-3">
            <HistoryManager ref="historyRef" :limit="10" />
          </div>
        </ScrollArea>

        <!-- settings tab -->
        <SettingsPanel 
          v-else-if="activeTab === 'settings'"
          :chrome-a-i-status="chromeAIStatus"
          :ollama-status="ollamaStatus"
          :available-models="availableModels"
          :selected-model="selectedModel"
          :show-model-dropdown="showModelDropdown"
          :model-dropdown-pos="modelDropdownPos"
          :summary-mode="summaryMode"
          :min-auto-words="minAutoWords"
          :allowlist-input="allowlistInput"
          :denylist-input="denylistInput"
          :word-popup-enabled="wordPopupEnabled"
          :preferred-provider="preferredProvider"
          :downloaded-languages="downloadedLanguages"
          :selected-languages="selectedLanguages"
          :download-progress="downloadProgress"
          @toggle-dropdown="toggleModelDropdown"
          @select-model="handleSelectModel"
          @update:summary-mode="(v) => summaryMode = v"
          @update:min-auto-words="(v) => minAutoWords = v"
          @update:allowlist-input="(v) => allowlistInput = v"
          @update:denylist-input="(v) => denylistInput = v"
          @toggle-word-popup="toggleWordPopup"
          @set-provider="setProviderPreference"
          @toggle-language="toggleLanguage"
          @delete-language="deleteLanguageData"
          @clear-cache="clearCache"
          @refresh-ollama="retryDetection"
          @open-welcome="openWelcomePage"
          :font-size="fontSize"
          @update:font-size="setFontSize"
        />

        <!-- error state -->
        <div v-else-if="ollamaStatus === 'error'" class="flex flex-col items-center justify-center h-full p-6">
          <X :size="32" class="mb-3 text-destructive/50" />
          <p class="text-[12px] text-foreground/70 mb-3">connection error</p>
          <button @click="retryDetection" class="px-3 py-1.5 rounded-md text-[11px] bg-primary text-primary-foreground hover:bg-primary/90 transition-colors flex items-center gap-1.5">
            <RefreshCw :size="12" />
            retry
          </button>
        </div>

        <!-- chrome ai unavailable state -->
        <div v-else-if="preferredProvider === 'chrome-ai' && chromeAIStatus === 'unavailable'" class="flex flex-col items-center justify-center h-full p-6">
          <X :size="32" class="mb-3 text-foreground/30" />
          <p class="text-[13px] font-medium text-foreground/80 mb-2">chrome ai unavailable</p>
          <p class="text-[11px] text-foreground/50 text-center mb-4 max-w-[200px]">gemini nano is not available on this device. try switching to ollama in settings.</p>
          <button @click="switchTab('settings')" class="px-3 py-1.5 rounded-md text-[11px] bg-primary text-primary-foreground hover:bg-primary/90 transition-colors flex items-center gap-1.5">
            <Settings :size="12" />
            open settings
          </button>
        </div>

        <!-- fallback: no ai available -->
        <div v-else class="flex flex-col items-center justify-center h-full p-6">
          <Loader2 class="w-6 h-6 mb-3 text-foreground/30" :stroke-width="2" />
          <p class="text-[11px] text-foreground/50">waiting for ai...</p>
        </div>
      </Transition>
    </main>
  </div>
</template>

<style scoped>
.content-down {
  padding-top: 44px;
  transition: padding-top 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.content-up {
  padding-top: 16px;
  transition: padding-top 180ms cubic-bezier(0.4, 0, 0.2, 1);
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
</style>
