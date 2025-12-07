<script setup>
import { ref, onMounted, onUnmounted, nextTick, computed, watch } from 'vue';
import { useThemeStore } from './stores/theme.js';
import { StorageManager, SUPPORTED_LANGUAGES } from './lib/StorageManager.js';
import { SummaryPrefs } from './lib/summaryPrefs.js';
import { formatTime, stripThinking } from './lib/textUtils.js';
import { getSetupCommands } from './utils/platformUtils.js';
import HistoryManager from './components/HistoryManager.vue';
import { marked } from 'marked';
import { 
  Sparkles, BarChart3, Settings, Loader2, ChevronDown, ChevronUp, Check, 
  Send, Trash2, X, RefreshCw, Database,
  Zap, Server, Circle, MessageCircle, FileText, AlertCircle
} from 'lucide-vue-next';

marked.setOptions({
  breaks: true,
  gfm: true
});

function renderMarkdown(text) {
  if (!text) return '';
  const cleaned = stripThinking(text);
  return marked.parseInline(cleaned);
}

const themeStore = useThemeStore();

const activeTab = ref('summary');
const ollamaStatus = ref('checking');
const availableModels = ref([]);
const selectedModel = ref('');
const historyRef = ref(null);
const showModelDropdown = ref(false);

const modelDropdownPos = ref({ top: 0, left: 0, width: 0 });
const chatContainer = ref(null);
const chatInputRef = ref(null);

// page summary state
const pageSummary = ref(null);
const pageMetadata = ref(null);
const summaryLoading = ref(false);
const summaryError = ref(null);
const currentTabId = ref(null);
const currentTabUrl = ref(null);
const summaryCollapsed = ref(false);
const summaryPrompt = ref(null);
const summaryMode = ref('manual');
const allowlistInput = ref(SummaryPrefs.ALLOWLIST.join('\n'));
const denylistInput = ref(SummaryPrefs.DENYLIST.join('\n'));
const minAutoWords = ref(SummaryPrefs.DEFAULT_PREFS.minAutoWords);
const minPromptWords = ref(SummaryPrefs.DEFAULT_PREFS.minPromptWords);

// chat state
const chatMessages = ref([]);
const chatInput = ref('');
const chatLoading = ref(false);

const wordPopupEnabled = ref(true);

const downloadedLanguages = ref([]);
const selectedLanguages = ref(['en']);
const downloadProgress = ref({});

const platformSetup = computed(() => getSetupCommands());

const storage = new StorageManager();

function getTabStorageKey(url) {
  if (!url) return null;
  // normalise url to remove query params for consistency
  try {
    const u = new URL(url);
    return `tab_session_${u.origin}${u.pathname}`;
  } catch {
    return `tab_session_${url}`;
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
    console.warn('metldr: failed to save tab session:', err.message);
  }
}

async function loadTabSession(url) {
  const key = getTabStorageKey(url);
  if (!key) return false;
  
  try {
    const result = await chrome.storage.local.get([key]);
    const session = result[key];
    
    // check if session exists and is not too old 24 hours
    if (session && (Date.now() - session.timestamp) < 86400000) {
      const msgs = session.chatMessages;
      chatMessages.value = Array.isArray(msgs) ? msgs : [];
      pageSummary.value = session.pageSummary || null;
      summaryCollapsed.value = session.summaryCollapsed || false;
      return true;
    }
  } catch (err) {
    console.warn('metldr: failed to load tab session:', err.message);
  }
  return false;
}

async function loadSummaryPrefs() {
  try {
    const stored = await chrome.storage.local.get(['summaryPrefs']);
    const prefs = SummaryPrefs.buildPrefs(stored?.summaryPrefs || {});
    summaryMode.value = prefs.mode || 'manual';
    allowlistInput.value = prefs.allowlist.join('\n');
    denylistInput.value = prefs.denylist.join('\n');
    minAutoWords.value = prefs.minAutoWords;
    minPromptWords.value = prefs.minPromptWords;
  } catch (err) {
    console.warn('metldr: failed to load summary prefs:', err.message);
    summaryMode.value = SummaryPrefs.DEFAULT_PREFS.mode;
    allowlistInput.value = SummaryPrefs.ALLOWLIST.join('\n');
    denylistInput.value = SummaryPrefs.DENYLIST.join('\n');
    minAutoWords.value = SummaryPrefs.DEFAULT_PREFS.minAutoWords;
    minPromptWords.value = SummaryPrefs.DEFAULT_PREFS.minPromptWords;
  }
}

async function saveSummaryPrefs() {
  try {
    const prefs = {
      mode: summaryMode.value,
      allowlist: SummaryPrefs.parseListInput(allowlistInput.value, SummaryPrefs.ALLOWLIST),
      denylist: SummaryPrefs.parseListInput(denylistInput.value, SummaryPrefs.DENYLIST),
      minAutoWords: Number.isFinite(Number(minAutoWords.value)) ? Number(minAutoWords.value) : SummaryPrefs.DEFAULT_PREFS.minAutoWords,
      minPromptWords: Number.isFinite(Number(minPromptWords.value)) ? Number(minPromptWords.value) : SummaryPrefs.DEFAULT_PREFS.minPromptWords
    };
    await chrome.storage.local.set({ summaryPrefs: prefs });
  } catch (err) {
    console.warn('metldr: failed to save summary prefs:', err.message);
  }
}

async function sendToBackground(message, retries = 2) {
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const response = await chrome.runtime.sendMessage(message);
      return response;
    } catch (error) {
      if (error.message?.includes('Receiving end does not exist') && attempt < retries) {
        await new Promise(resolve => setTimeout(resolve, 100 * (attempt + 1)));
        continue;
      }
      throw error;
    }
  }
}

const updateDropdownPosition = (buttonElement, posRef) => {
  if (!buttonElement) return;
  const rect = buttonElement.getBoundingClientRect();
  posRef.value = {
    top: rect.bottom + 8,
    left: rect.left,
    width: rect.width,
  };
};

const toggleModelDropdown = async () => {
  if (!showModelDropdown.value) {
    const btn = document.querySelector('.model-selector-btn');
    updateDropdownPosition(btn, modelDropdownPos);
    await refreshModels();
  }
  showModelDropdown.value = !showModelDropdown.value;
};

async function refreshModels() {
  try {
    const response = await sendToBackground({ type: 'CHECK_OLLAMA_HEALTH' });
    if (response?.connected && response?.models) {
      const newModels = response.models;
      const oldModels = availableModels.value;
      
      const modelsChanged = newModels.length !== oldModels.length ||
        newModels.some((m, i) => m !== oldModels[i]);
      
      if (modelsChanged) {
        availableModels.value = newModels;
        
        if (selectedModel.value && !newModels.includes(selectedModel.value)) {
          console.log('metldr: selected model removed, switching to first available');
          if (newModels.length > 0) {
            await selectModel(newModels[0]);
          } else {
            selectedModel.value = '';
          }
        }
        
        if (!selectedModel.value && newModels.length > 0) {
          await selectModel(newModels[0]);
        }
      }
      
      if (ollamaStatus.value !== 'ready' && newModels.length > 0) {
        ollamaStatus.value = 'ready';
      }
      
      return true;
    } else if (response?.connected && (!response?.models || response.models.length === 0)) {
      availableModels.value = [];
      selectedModel.value = '';
      return true;
    } else if (!response?.connected) {
      ollamaStatus.value = 'not-found';
      return false;
    }
    return false;
  } catch (error) {
    console.warn('metldr: failed to refresh models:', error.message);
    if (ollamaStatus.value === 'ready') {
      ollamaStatus.value = 'not-found';
    }
    return false;
  }
}

const selectModel = async (model) => {
  selectedModel.value = model;
  showModelDropdown.value = false;
  try {
    await chrome.storage.local.set({ selectedModel: model });
  } catch (error) {
    console.error('metldr: failed to save model selection:', error);
  }
};

async function checkOllama(showChecking = true) {
  const wasReady = ollamaStatus.value === 'ready';
  if (showChecking && !wasReady) {
    ollamaStatus.value = 'checking';
  }
  
  try {
    storage.initDictionary().catch(err => {
      console.warn('metldr: dict init failed:', err.message);
    });
    
    let response = null;
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        response = await sendToBackground({ type: 'CHECK_OLLAMA_HEALTH' });
        if (response && response.success !== undefined) break;
        await new Promise(r => setTimeout(r, 150 * (attempt + 1)));
      } catch (err) {
        console.warn('metldr: ollama check attempt', attempt + 1, 'failed:', err.message);
        await new Promise(r => setTimeout(r, 150 * (attempt + 1)));
      }
    }
    
    if (!response || response.success === undefined) {
      console.warn('metldr: no valid response from background');
      if (!wasReady) {
        ollamaStatus.value = 'not-found';
      }
      return wasReady;
    }
    
    const { connected, models } = response;
    
    if (connected && models && models.length > 0) {
      ollamaStatus.value = 'ready';
      availableModels.value = models;
      
      try {
        const result = await chrome.storage.local.get(['selectedModel']);
        if (result.selectedModel && models.includes(result.selectedModel)) {
          selectedModel.value = result.selectedModel;
        } else if (!selectedModel.value || !models.includes(selectedModel.value)) {
          selectedModel.value = models[0];
        }
      } catch (error) {
        if (!selectedModel.value) selectedModel.value = models[0];
      }
      
      if (!wasReady && summaryMode.value !== 'manual') {
        await fetchCurrentPageSummary(false, 'auto');
      }
      return true;
    }
    
    if (!wasReady) {
      ollamaStatus.value = 'not-found';
    }
    return wasReady;
  } catch (error) {
    console.error('metldr: ollama check failed:', error);
    if (!wasReady) {
      ollamaStatus.value = 'not-found';
    }
    return wasReady;
  }
}

async function fetchCurrentPageSummary(force = false, trigger = 'auto') {
  try {
    summaryPrompt.value = null;
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tabs.length) {
      summaryError.value = 'no active tab';
      return;
    }
    
    const tab = tabs[0];
    currentTabId.value = tab.id;
    currentTabUrl.value = tab.url;
    
    if (!tab.url || tab.url.startsWith('chrome://') || tab.url.startsWith('edge://') || tab.url.startsWith('about:')) {
      summaryError.value = 'system page';
      return;
    }
    
    if (!force) {
      const hasSession = await loadTabSession(tab.url);
      if (hasSession && pageSummary.value) {
        pageMetadata.value = { title: pageSummary.value.title, url: tab.url };
        return;
      }
    }
    
    if (force) {
      chatMessages.value = [];
    }
    
    summaryLoading.value = true;
    summaryError.value = null;
    
    const response = await sendToBackground({
      type: 'EXTRACT_AND_SUMMARIZE',
      tabId: tab.id,
      force,
      trigger
    });
    if (!response || !response.success) {
      if (response?.prompt) {
        summaryPrompt.value = {
          url: tab.url,
          reason: response.reason || 'needs approval'
        };
        summaryError.value = null;
      } else if (response?.skip) {
        summaryError.value = response.reason || 'page skipped';
      } else {
        summaryError.value = response?.error || 'summarisation failed';
      }
      summaryLoading.value = false;
      return;
    }
    
    pageSummary.value = response.summary;
    pageMetadata.value = { title: response.summary.title, url: tab.url };
    summaryError.value = null;
    
    await saveTabSession();
    
  } catch (error) {
    console.error('metldr: page summary failed:', error);
    summaryError.value = error.message || 'unknown error';
  } finally {
    summaryLoading.value = false;
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

function handleKeydown(e) {
  if (e.ctrlKey && e.shiftKey && e.key?.toLowerCase?.() === 'l') {
    e.preventDefault();
    triggerManualSummary();
  }
}

function switchTab(tab) {
  if (activeTab.value === tab) return;
  activeTab.value = tab;
}

function onBeforeEnter(el) {
  el.style.height = '0';
  el.style.opacity = '0';
}

function onEnter(el, done) {
  el.offsetHeight;
  el.style.transition = 'height 250ms cubic-bezier(0.4, 0, 0.2, 1), opacity 200ms ease-out';
  el.style.height = el.scrollHeight + 'px';
  el.style.opacity = '1';
  
  el.addEventListener('transitionend', function handler(e) {
    if (e.propertyName === 'height') {
      el.removeEventListener('transitionend', handler);
      done();
    }
  });
}

function onAfterEnter(el) {
  el.style.height = 'auto';
  el.style.transition = '';
}

function onBeforeLeave(el) {
  el.style.height = el.scrollHeight + 'px';
  el.style.opacity = '1';
}

function onLeave(el, done) {
  el.offsetHeight;
  el.style.transition = 'height 200ms cubic-bezier(0.4, 0, 0.2, 1), opacity 150ms ease-in';
  el.style.height = '0';
  el.style.opacity = '0';
  
  el.addEventListener('transitionend', function handler(e) {
    if (e.propertyName === 'height') {
      el.removeEventListener('transitionend', handler);
      done();
    }
  });
}

function onAfterLeave(el) {
  el.style.height = '';
  el.style.transition = '';
}

async function retryDetection() {
  ollamaStatus.value = 'checking';
  await checkOllama(false);
}

function copyToClipboard(text) {
  navigator.clipboard.writeText(text);
}

async function sendChatMessage() {
  if (!chatInput.value.trim() || chatLoading.value) return;
  
  const userMessage = chatInput.value.trim();
  chatInput.value = '';
  
  if (!Array.isArray(chatMessages.value)) {
    chatMessages.value = [];
  }
  
  chatMessages.value.push({ role: 'user', content: userMessage });
  
  await nextTick();
  if (chatContainer.value) {
    chatContainer.value.scrollTop = chatContainer.value.scrollHeight;
  }
  
  if (chatInputRef.value) {
    chatInputRef.value.focus();
  }
  
  chatLoading.value = true;
  
  try {
    const model = selectedModel.value || availableModels.value[0];
    
    // last 6 messages for context
    const recentMessages = chatMessages.value.slice(-6).map(m => ({
      role: m.role,
      content: m.content
    }));
    
    // full page context
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
    });
    
    if (response?.ok) {
      chatMessages.value.push({ 
        role: 'assistant', 
        content: response.content,
        timing: response.timing || null
      });
    } else {
      throw new Error(response?.error || 'chat failed');
    }
  } catch (error) {
    console.warn('metldr: chat error:', error.message);
    chatMessages.value.push({
      role: 'assistant',
      content: 'sorry, something went wrong. try again.'
    });
  } finally {
    await nextTick();
    if (chatContainer.value) {
      chatContainer.value.scrollTop = chatContainer.value.scrollHeight;
    }
    chatLoading.value = false;
    
    await saveTabSession();
    
    if (chatInputRef.value) {
      chatInputRef.value.focus();
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

async function toggleWordPopup() {
  wordPopupEnabled.value = !wordPopupEnabled.value;
  
  try {
    await chrome.storage.local.set({ wordPopupEnabled: wordPopupEnabled.value });
    
    const tabs = await chrome.tabs.query({});
    tabs.forEach(tab => {
      chrome.tabs.sendMessage(tab.id, {
        type: 'WORD_POPUP_TOGGLED',
        enabled: wordPopupEnabled.value
      }).catch(() => {});
    });
  } catch (error) {
    console.error('metldr: failed to save word popup setting:', error);
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

function openSetupGuide() {
  chrome.tabs.create({ url: chrome.runtime.getURL('welcome.html') });
}

async function loadDictionarySettings() {
  try {
    console.log('[metldr] loading dictionary settings...');
    await storage.initDictionary();
    const persisted = await chrome.storage.local.get(['dictDownloadProgress', 'downloadingLanguages']);
    const persistedProgress = persisted.dictDownloadProgress || {};
    const persistedDownloading = Array.isArray(persisted.downloadingLanguages) ? persisted.downloadingLanguages : [];
    downloadProgress.value = { ...persistedProgress };
    downloadedLanguages.value = await storage.dictGetDownloadedLanguages();
    console.log('[metldr] downloaded languages:', downloadedLanguages.value);
    
    const settings = await chrome.storage.local.get(['selectedLanguages']);
    if (settings.selectedLanguages && settings.selectedLanguages.length > 0) {
      selectedLanguages.value = settings.selectedLanguages;
    }
    console.log('[metldr] selected languages:', selectedLanguages.value);
    
    persistedDownloading.forEach(lang => {
      if (!downloadProgress.value[lang]) {
        downloadProgress.value = { ...downloadProgress.value, [lang]: { progress: 0, letter: 'a', entries: 0 } };
      }
      startDownload(lang);
    });

    selectedLanguages.value.forEach(lang => {
      if (!downloadedLanguages.value.includes(lang) && !downloadProgress.value[lang]) {
        startDownload(lang);
      }
    });

    if (!downloadedLanguages.value.includes('en')) {
      console.log('[metldr] english not downloaded, starting download...');
      startDownload('en');
    } else {
      console.log('[metldr] english already downloaded');
    }
  } catch (error) {
    console.error('metldr: failed to load dictionary settings:', error);
  }
}

async function startDownload(langCode) {
  console.log('[metldr] starting download for:', langCode);
  downloadProgress.value = { ...downloadProgress.value, [langCode]: { progress: 0, letter: 'a', entries: 0 } };
  
  try {
    await storage.dictDownloadLanguage(langCode, (progressData) => {
      downloadProgress.value = { ...downloadProgress.value, [langCode]: {
        progress: progressData.progress,
        letter: progressData.letter,
        entries: progressData.entriesProcessed
      } };
    });
    
    console.log('[metldr] download completed for:', langCode);
    const { [langCode]: _, ...rest } = downloadProgress.value;
    downloadProgress.value = rest;
    downloadedLanguages.value = await storage.dictGetDownloadedLanguages();

    if (!selectedLanguages.value.includes(langCode)) {
      selectedLanguages.value.push(langCode);
      await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] });
    }
  } catch (error) {
    console.error('[metldr] download failed for', langCode, ':', error);
    const { [langCode]: _, ...rest } = downloadProgress.value;
    downloadProgress.value = rest;
  }
}

async function toggleLanguage(langCode) {
  const index = selectedLanguages.value.indexOf(langCode);
  
  if (index === -1) {
    selectedLanguages.value.push(langCode);
    await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] });
    
    if (!downloadedLanguages.value.includes(langCode) && !downloadProgress.value[langCode]) {
      startDownload(langCode);
    }
  } else {
    selectedLanguages.value.splice(index, 1);
    await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] });
  }
}

async function deleteLanguageData(langCode) {
  if (!confirm(`delete ${langCode} dictionary data?`)) return;
  
  try {
    downloadedLanguages.value = downloadedLanguages.value.filter(l => l !== langCode);
    const { [langCode]: _, ...rest } = downloadProgress.value;
    downloadProgress.value = rest;
    await chrome.storage.local.set({
      downloadingLanguages: Object.keys(rest),
      dictDownloading: false
    });

    await storage.dictDeleteLanguage(langCode);
    downloadedLanguages.value = await storage.dictGetDownloadedLanguages();
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

watch([summaryMode, allowlistInput, denylistInput, minAutoWords, minPromptWords], () => {
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
      
      if (summaryMode.value !== 'manual') {
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
      
      if (summaryMode.value !== 'manual') {
        await fetchCurrentPageSummary(urlChanged, 'auto');
      }
    }
  });
}

onMounted(async () => {
  await themeStore.loadSavedTheme();
  
  try {
    const settings = await chrome.storage.local.get(['wordPopupEnabled']);
    if (settings.wordPopupEnabled !== undefined) {
      wordPopupEnabled.value = settings.wordPopupEnabled;
    }
  } catch (error) {
    console.error('metldr: failed to load word popup settings:', error);
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

  window.addEventListener('keydown', handleKeydown);

  const dropdownClickHandler = (e) => {
    if (showModelDropdown.value && !e.target.closest('.model-selector-btn') && !e.target.closest('.model-dropdown')) {
      showModelDropdown.value = false;
    }
  };

  document.addEventListener('click', dropdownClickHandler);

  const statusCheckInterval = setInterval(async () => {
    if (ollamaStatus.value !== 'ready') {
      await checkOllama(false); 
    } else {
      await refreshModels();
    }
  }, 3000);

  onUnmounted(() => {
    clearInterval(statusCheckInterval);
    document.removeEventListener('click', dropdownClickHandler);
    window.removeEventListener('keydown', handleKeydown);
  });
});
</script>

<template>
  <div class="h-screen flex flex-col bg-base-100 text-base-content overflow-hidden">
    <!-- teleport target for dropdowns -->
    <div id="dropdown-portal" class="fixed inset-0 pointer-events-none z-50"></div>

    <!-- header with tabs -->
    <header v-if="ollamaStatus === 'ready'" class="shrink-0 sticky top-0 z-10 border-b border-primary/10">
      <div class="flex gap-1.5 p-2 bg-base-100/95 backdrop-blur-lg">
        <button 
          v-for="(tab, idx) in [{key: 'summary', label: 'Summary', icon: FileText, color: 'primary'}, {key: 'stats', label: 'Stats', icon: BarChart3, color: 'secondary'}, {key: 'settings', label: 'Settings', icon: Settings, color: 'accent'}]"
          :key="tab.key"
          @click="switchTab(tab.key)"
          class="flex-1 flex items-center justify-center gap-1.5 py-2 px-3 text-[11px] font-medium rounded-lg transition-all duration-200"
          :class="[
            activeTab === tab.key 
              ? `bg-${tab.color}/15 text-${tab.color} border border-${tab.color}/20` 
              : 'text-base-content/50 hover:text-base-content/70 hover:bg-base-content/5 border border-transparent'
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
          <Loader2 class="w-8 h-8 mb-3 animate-spin text-base-content/40" :stroke-width="2" />
          <p class="text-[12px] text-base-content/50">connecting to ollama...</p>
        </div>

        <!-- not found state -->
        <div v-else-if="ollamaStatus === 'not-found'" class="p-3 h-full overflow-y-auto">
          <div class="rounded-xl bg-base-content/5 p-4 space-y-4">
            <!-- header with pulsing indicator -->
            <div class="flex items-start gap-3">
              <div class="w-8 h-8 rounded-lg bg-warning/10 flex items-center justify-center shrink-0 relative">
                <Server :size="16" class="text-warning/70" />
                <span class="absolute -top-0.5 -right-0.5 w-2 h-2 bg-warning rounded-full animate-pulse"></span>
              </div>
              <div>
                <h2 class="text-[13px] font-medium text-base-content/80 mb-0.5">waiting for ollama...</h2>
                <p class="text-[10px] text-base-content/40">auto-detecting every 2s · will connect when ready</p>
              </div>
            </div>
            
            <!-- step 1: start ollama -->
            <div class="rounded-lg bg-primary/5 border border-primary/10 p-3">
              <div class="text-[11px] font-medium text-base-content/70 mb-2 flex items-center gap-2">
                <span class="w-4 h-4 rounded-full bg-primary/20 text-primary text-[9px] font-bold flex items-center justify-center">1</span>
                start ollama with extension support
              </div>
              <p class="text-[10px] text-base-content/50 mb-2">open {{ platformSetup.terminalName }} and run:</p>
              <div class="flex items-center gap-2 bg-base-100/50 rounded-lg p-2">
                <code class="flex-1 font-mono text-[10px] text-base-content/80 overflow-x-auto whitespace-nowrap">{{ platformSetup.serve }}</code>
                <button @click="copyToClipboard(platformSetup.serve)" class="btn btn-xs btn-primary text-[9px] shrink-0">
                  copy
                </button>
              </div>
              <p class="text-[9px] text-base-content/40 mt-2">💡 keep this terminal open while using metldr</p>
            </div>
            
            <!-- step 2: pull a model (if needed) -->
            <details class="rounded-lg bg-secondary/5 border border-secondary/10">
              <summary class="p-3 text-[11px] font-medium text-base-content/70 cursor-pointer hover:bg-secondary/5 flex items-center gap-2">
                <span class="w-4 h-4 rounded-full bg-secondary/20 text-secondary text-[9px] font-bold flex items-center justify-center">2</span>
                no models? pull one first
                <ChevronDown :size="12" class="ml-auto text-base-content/40" />
              </summary>
              <div class="px-3 pb-3 space-y-2">
                <p class="text-[10px] text-base-content/50">in a new terminal, run:</p>
                <div class="flex items-center gap-2 bg-base-100/50 rounded-lg p-2">
                  <code class="flex-1 font-mono text-[10px] text-base-content/80">ollama pull gemma3:1b</code>
                  <button @click="copyToClipboard('ollama pull gemma3:1b')" class="btn btn-xs btn-ghost text-[9px] shrink-0">copy</button>
                </div>
                <p class="text-[9px] text-base-content/40">this downloads a fast, lightweight model (~500mb)</p>
              </div>
            </details>
            
            <!-- step 3: install ollama (if needed) -->
            <details class="rounded-lg bg-accent/5 border border-accent/10">
              <summary class="p-3 text-[11px] font-medium text-base-content/70 cursor-pointer hover:bg-accent/5 flex items-center gap-2">
                <span class="w-4 h-4 rounded-full bg-accent/20 text-accent text-[9px] font-bold flex items-center justify-center">?</span>
                don't have ollama installed?
                <ChevronDown :size="12" class="ml-auto text-base-content/40" />
              </summary>
              <div class="px-3 pb-3 space-y-2">
                <p class="text-[10px] text-base-content/50">install with this command:</p>
                <div class="flex items-center gap-2 bg-base-100/50 rounded-lg p-2">
                  <code class="flex-1 font-mono text-[10px] text-base-content/80 overflow-x-auto">{{ platformSetup.install }}</code>
                  <button @click="copyToClipboard(platformSetup.install)" class="btn btn-xs btn-ghost text-[9px] shrink-0">copy</button>
                </div>
                <p class="text-[9px] text-base-content/40">or download from <a href="https://ollama.com" target="_blank" class="text-primary/70 hover:text-primary underline">ollama.com</a></p>
              </div>
            </details>
            
            <!-- manual retry + setup guide -->
            <div class="flex items-center justify-between pt-2 border-t border-base-content/5">
              <button @click="retryDetection" class="btn btn-sm bg-base-content/10 hover:bg-base-content/15 border-0 text-[11px]">
                <RefreshCw :size="12" />
                check now
              </button>
              <button @click="openSetupGuide" class="text-[10px] text-base-content/40 hover:text-base-content/60">
                full setup guide →
              </button>
            </div>
          </div>
        </div>

        <!-- summary tab -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'summary'" :class="['flex flex-col h-full relative', summaryPrompt ? 'pb-16' : '']">
          
          <div v-if="summaryPrompt" class="absolute right-2 bottom-2 max-w-xs rounded-lg border border-warning/30 bg-warning/5 px-3 py-2 text-[10px] shadow-lg z-10">
            <div class="flex items-start gap-2">
              <div class="flex-1 min-w-0">
                <div class="font-medium text-base-content/70 truncate">summarise this page?</div>
                <div class="text-base-content/50 truncate">{{ summaryPrompt.reason }}</div>
              </div>
              <div class="flex gap-1 shrink-0">
                <button class="btn btn-ghost btn-2xs text-[10px]" @click="declineSummaryPrompt">no</button>
                <button class="btn btn-2xs bg-warning/20 border-warning/40 text-[10px]" @click="acceptSummaryPrompt">yes</button>
              </div>
            </div>
          </div>
          
          <!-- collapsible summary area -->
          <div class="shrink-0 p-2 pb-0">
            <!-- summary card with collapse toggle -->
            <div v-if="pageSummary" class="rounded-lg bg-primary/5 border border-primary/10">
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
                  <h3 class="text-[12px] font-medium text-base-content/80 truncate leading-tight">
                    {{ pageSummary.title || 'untitled' }}
                  </h3>
                  <p v-if="summaryCollapsed" class="text-[10px] text-base-content/50 truncate">
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
                  <div class="px-3 pb-3 pt-1 border-t border-primary/10">
                    <!-- metadata -->
                    <p v-if="pageSummary.publication || pageSummary.author" class="text-[10px] text-base-content/50 mb-2">
                      {{ pageSummary.publication }}{{ pageSummary.author ? ` · ${pageSummary.author}` : '' }}
                    </p>
                    
                    <!-- bullets -->
                    <ul class="space-y-1.5">
                      <li v-for="(bullet, i) in pageSummary.bullets" :key="i" class="flex gap-2 text-[11px] leading-relaxed">
                        <span class="text-primary/50 shrink-0 mt-0.5">•</span>
                        <span class="text-base-content/75 markdown-content" v-html="renderMarkdown(bullet)"></span>
                      </li>
                    </ul>
                    
                    <div class="flex items-center justify-between pt-2 mt-2 border-t border-primary/10">
                      <span class="text-[10px] text-base-content/50">{{ pageSummary.readTime || 'n/a' }} read</span>
                      <div 
                        v-if="pageSummary.timing"
                        class="timing-badge group relative cursor-help"
                      >
                        <div class="flex items-center gap-1 text-[10px] text-base-content/50 hover:text-base-content/70 transition-colors">
                          <span>{{ formatTime(pageSummary.timing.total) }}</span>
                          <span v-if="pageSummary.timing.cached" class="text-primary/70">· cached</span>
                        </div>
                        <!-- hover breakdown tooltip -->
                        <div class="timing-tooltip absolute bottom-full right-0 mb-2 px-2.5 py-2 rounded-lg bg-base-200 border border-base-content/15 shadow-lg opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-150 z-50 min-w-[140px]">
                          <div class="text-[10px] font-medium text-base-content/70 mb-1.5 uppercase tracking-wide">time breakdown</div>
                          <div class="space-y-1 text-[10px]">
                            <div class="flex justify-between gap-3">
                              <span class="text-base-content/60">extraction</span>
                              <span class="text-base-content/80 font-mono">{{ formatTime(pageSummary.timing.extraction || 0) }}</span>
                            </div>
                            <div class="flex justify-between gap-3">
                              <span class="text-base-content/60">llm</span>
                              <span class="text-base-content/80 font-mono">{{ formatTime(pageSummary.timing.llm || 0) }}</span>
                            </div>
                            <div class="flex justify-between gap-3 pt-1 border-t border-base-content/15">
                              <span class="text-base-content/70">total</span>
                              <span class="text-primary font-mono font-medium">{{ formatTime(pageSummary.timing.total) }}</span>
                            </div>
                            <div v-if="pageSummary.timing.model" class="pt-1 text-base-content/55 truncate">
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
            <div v-else-if="summaryLoading" class="flex items-center gap-2.5 p-3 rounded-lg bg-primary/5 border border-primary/10">
              <Loader2 class="w-4 h-4 animate-spin text-primary/60" />
              <span class="text-[11px] text-base-content/60">analysing...</span>
            </div>

            <!-- error state -->
            <div v-else-if="summaryError" class="flex items-center gap-2.5 p-3 rounded-lg bg-error/5 border border-error/10">
              <AlertCircle :size="13" class="text-error/60 shrink-0" />
              <div>
                <p class="text-[11px] text-base-content/70">{{ summaryError }}</p>
                <p class="text-[10px] text-base-content/50">navigate to an article to see a summary</p>
              </div>
            </div>

            <!-- no page state -->
            <div v-else class="flex items-center gap-2.5 p-3 rounded-lg bg-base-content/5 border border-base-content/10">
              <FileText :size="13" class="text-base-content/40 shrink-0" />
              <p class="text-[11px] text-base-content/55">browse a page to get a summary</p>
            </div>
          </div>

          <!-- chat ui -->
          <div class="flex-1 flex flex-col min-h-0 p-2">
            <div class="flex-1 flex flex-col rounded-lg bg-secondary/5 border border-secondary/10 p-3 min-h-0">
              <div class="flex items-center justify-between mb-2 shrink-0">
                <div class="flex items-center gap-1.5">
                  <MessageCircle :size="11" class="text-secondary/70" />
                  <span class="text-[10px] font-medium text-secondary/70 uppercase tracking-wide">chat</span>
                </div>
                <button 
                  v-if="chatMessages.length > 0"
                  @click="clearChat" 
                  class="text-[10px] text-base-content/50 hover:text-error/70 transition-colors"
                >
                  clear
                </button>
              </div>
              
              <!-- messages container -->
              <div 
                ref="chatContainer"
                class="flex-1 overflow-y-auto space-y-2 min-h-0"
              >
                <!-- empty state -->
                <div v-if="chatMessages.length === 0 && !chatLoading" class="flex items-center justify-center h-full">
                  <p class="text-[10px] text-base-content/50">ask about the article</p>
                </div>
                
                <!-- messages -->
                <div v-for="(msg, i) in chatMessages" :key="i" class="message-wrapper">
                  <div v-if="msg.role === 'user'" class="flex justify-end">
                    <div class="max-w-[85%] bg-primary/10 border border-primary/20 rounded-xl rounded-br-sm px-2.5 py-1.5 text-[11px] text-base-content/80">
                      {{ msg.content }}
                    </div>
                  </div>
                  <div v-else class="flex flex-col items-start max-w-[85%]">
                    <div class="bg-secondary/10 border border-secondary/20 rounded-xl rounded-bl-sm px-2.5 py-1.5 text-[11px] text-base-content/75 markdown-content" v-html="renderMarkdown(msg.content)">
                    </div>
                    <div 
                      v-if="msg.timing" 
                      class="flex items-center gap-1 mt-0.5 ml-1 text-[9px] text-base-content/45 hover:text-base-content/60 transition-colors cursor-default group relative"
                    >
                      <span>{{ formatTime(msg.timing.total) }}</span>
                      <div class="chat-timing-tooltip absolute left-full top-1/2 -translate-y-1/2 ml-2 px-2 py-1.5 rounded bg-base-200 border border-base-content/15 shadow-md opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-150 z-50 whitespace-nowrap">
                        <div class="text-[9px] text-base-content/60">
                          <span class="text-base-content/80">{{ formatTime(msg.timing.total) }}</span>
                          <span v-if="msg.timing.model" class="text-base-content/55"> · {{ msg.timing.model }}</span>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
                
                <!-- loading -->
                <div v-if="chatLoading" class="flex justify-start">
                  <div class="bg-secondary/10 border border-secondary/20 rounded-xl rounded-bl-sm px-3 py-2">
                    <div class="flex gap-1">
                      <span class="w-1.5 h-1.5 bg-secondary/50 rounded-full animate-bounce" style="animation-delay: 0ms"></span>
                      <span class="w-1.5 h-1.5 bg-secondary/50 rounded-full animate-bounce" style="animation-delay: 150ms"></span>
                      <span class="w-1.5 h-1.5 bg-secondary/50 rounded-full animate-bounce" style="animation-delay: 300ms"></span>
                    </div>
                  </div>
                </div>
              </div>

              <!-- input -->
              <div class="flex items-center gap-2 mt-2 shrink-0">
                <input 
                  ref="chatInputRef"
                  v-model="chatInput"
                  @keydown.enter.prevent="sendChatMessage"
                  type="text"
                  placeholder="ask something..."
                  class="flex-1 bg-base-content/5 border-0 rounded-lg px-3 py-2 text-[11px] focus:outline-none focus:ring-1 focus:ring-primary/30 placeholder:text-base-content/30"
                />
                <button 
                  @click="sendChatMessage"
                  :disabled="!chatInput.trim() || chatLoading"
                  class="flex items-center justify-center w-8 h-8 rounded-lg bg-primary/10 hover:bg-primary/20 border border-primary/20 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                >
                  <Send :size="14" class="text-primary" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- stats tab -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'stats'" class="p-3 h-full overflow-y-auto">
          <HistoryManager ref="historyRef" :limit="10" />
        </div>

        <!-- settings tab -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'settings'" class="p-3 space-y-3 h-full overflow-y-auto">
          <!-- model selection -->
          <div class="rounded-xl bg-base-content/5 p-4 border border-primary/10">
            <div class="flex items-center gap-2.5 mb-3">
              <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/15">
                <Server :size="12" class="text-primary" />
              </div>
              <span class="text-[12px] font-medium text-base-content/70 tracking-wide">ai model</span>
            </div>
            
            <div class="relative">
              <button 
                @click="toggleModelDropdown"
                class="model-selector-btn w-full flex items-center justify-between px-3 py-2.5 bg-base-content/5 hover:bg-base-content/10 rounded-lg transition-colors"
              >
                <span class="font-mono text-[12px] text-base-content/70">{{ selectedModel }}</span>
                <ChevronDown 
                  :size="14" 
                  class="text-base-content/40 transition-transform"
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
                  <div 
                    v-if="showModelDropdown" 
                    class="model-dropdown fixed rounded-lg bg-base-100 border border-base-content/10 shadow-xl max-h-48 overflow-y-auto pointer-events-auto"
                    :style="{ 
                      top: modelDropdownPos.top + 'px', 
                      left: modelDropdownPos.left + 'px', 
                      width: modelDropdownPos.width + 'px'
                    }"
                  >
                    <button 
                      v-for="model in availableModels" 
                      :key="model"
                      @click="selectModel(model)"
                      class="w-full flex items-center justify-between px-3 py-2 text-[12px] font-mono hover:bg-base-content/5 transition-colors"
                      :class="{ 'bg-base-content/10': selectedModel === model }"
                    >
                      <span class="text-base-content/70">{{ model }}</span>
                      <Check v-if="selectedModel === model" :size="14" class="text-base-content/50" />
                    </button>
                  </div>
                </Transition>
              </Teleport>
            </div>
          </div>

          <!-- summary preferences -->
          <div class="rounded-xl bg-base-content/5 p-4 border border-primary/10 space-y-3">
            <div class="flex items-center gap-2.5">
              <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/15">
                <FileText :size="12" class="text-primary" />
              </div>
              <span class="text-[12px] font-medium text-base-content/70 tracking-wide">summaries</span>
            </div>

            <div class="flex gap-2">
              <button
                class="btn btn-xs flex-1"
                :class="summaryMode === 'manual' ? 'bg-primary/15 border-primary/30 text-primary' : 'btn-ghost border-base-content/10'"
                @click="summaryMode = 'manual'"
              >manual</button>
              <button
                class="btn btn-xs flex-1"
                :class="summaryMode === 'smart' ? 'bg-primary/15 border-primary/30 text-primary' : 'btn-ghost border-base-content/10'"
                @click="summaryMode = 'smart'"
              >smart (ask)</button>
              <button
                class="btn btn-xs flex-1"
                :class="summaryMode === 'auto' ? 'bg-primary/15 border-primary/30 text-primary' : 'btn-ghost border-base-content/10'"
                @click="summaryMode = 'auto'"
              >auto (trusted)</button>
            </div>

            <div class="grid grid-cols-2 gap-2">
              <label class="text-[11px] text-base-content/60 flex items-center gap-2">
                <span class="w-28">auto min words</span>
                <input type="number" min="0" class="input input-xs input-bordered w-full" v-model.number="minAutoWords" />
              </label>
              <label class="text-[11px] text-base-content/60 flex items-center gap-2">
                <span class="w-28">prompt min words</span>
                <input type="number" min="0" class="input input-xs input-bordered w-full" v-model.number="minPromptWords" />
              </label>
            </div>

            <div class="grid grid-cols-2 gap-3">
              <div>
                <div class="flex items-center justify-between mb-1">
                  <span class="text-[11px] text-base-content/60">allowlist</span>
                  <span class="text-[10px] text-base-content/40">one per line</span>
                </div>
                <textarea 
                  class="textarea textarea-bordered textarea-xs w-full h-28"
                  v-model="allowlistInput"
                  placeholder="example.com"
                ></textarea>
              </div>
              <div>
                <div class="flex items-center justify-between mb-1">
                  <span class="text-[11px] text-base-content/60">denylist</span>
                  <span class="text-[10px] text-base-content/40">one per line</span>
                </div>
                <textarea 
                  class="textarea textarea-bordered textarea-xs w-full h-28"
                  v-model="denylistInput"
                  placeholder="dashboard"
                ></textarea>
              </div>
            </div>

            <p class="text-[10px] text-base-content/45">manual = only on click; smart = prompt on medium-confidence pages; auto = run on trusted/high-confidence pages.</p>
          </div>

          <!-- word lookup toggle -->
          <div class="rounded-xl bg-base-content/5 p-4 border border-secondary/10">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2.5 group relative cursor-help" title="show definitions on text selection">
                <div class="flex items-center justify-center w-6 h-6 rounded-md bg-secondary/15">
                  <Sparkles :size="12" class="text-secondary" />
                </div>
                <span class="text-[12px] font-medium text-base-content/70 tracking-wide">word lookup</span>
              </div>
              <input 
                type="checkbox" 
                class="toggle toggle-sm toggle-secondary" 
                :checked="wordPopupEnabled"
                @click="toggleWordPopup"
              />
            </div>
          </div>

          <!-- dictionaries -->
          <div class="rounded-xl bg-base-content/5 p-4 border border-accent/10">
            <div class="flex items-center gap-2.5 mb-3">
              <div class="flex items-center justify-center w-6 h-6 rounded-md bg-accent/15">
                <Database :size="12" class="text-accent" />
              </div>
              <span class="text-[12px] font-medium text-base-content/70 tracking-wide">dictionaries</span>
            </div>
            
            <div class="space-y-1 max-h-48 overflow-y-auto">
              <label 
                v-for="lang in SUPPORTED_LANGUAGES" 
                :key="lang.code" 
                class="flex items-center gap-2.5 py-1.5 cursor-pointer hover:bg-base-content/5 rounded-lg px-2 -mx-2 transition-colors"
              >
                <input 
                  type="checkbox"
                  :checked="selectedLanguages.includes(lang.code)"
                  @change="toggleLanguage(lang.code)"
                  class="checkbox checkbox-sm"
                />
                <span class="text-[12px] text-base-content/70 flex-1">{{ lang.name }}</span>
                
                <span 
                  v-if="downloadProgress[lang.code]"
                  class="text-[10px] text-base-content/55"
                >
                  {{ Number(downloadProgress[lang.code].progress || 0).toFixed(0) }}%
                </span>
                
                <button
                  v-if="downloadedLanguages.includes(lang.code)"
                  @click.stop="deleteLanguageData(lang.code)"
                  class="btn btn-ghost btn-xs text-base-content/30 hover:text-error/70"
                >
                  <X :size="12" />
                </button>
              </label>
            </div>
          </div>

          <!-- theme -->
          <div class="rounded-xl bg-base-content/5 p-4 border border-primary/10">
            <div class="flex items-center gap-2.5 mb-3">
              <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/15">
                <Sparkles :size="12" class="text-primary" />
              </div>
              <span class="text-[12px] font-medium text-base-content/70 tracking-wide">theme</span>
            </div>
            <div class="grid grid-cols-3 gap-2">
              <button
                v-for="(themeData, key) in themeStore.themes"
                :key="key"
                @click="themeStore.setTheme(key)"
                class="flex flex-col items-center gap-1.5 p-2 rounded-lg transition-all"
                :class="[
                  themeStore.currentTheme === key 
                    ? 'bg-base-content/10 ring-1 ring-base-content/20' 
                    : 'hover:bg-base-content/5'
                ]"
              >
                <div class="flex gap-0.5">
                  <div class="w-3 h-3 rounded-full" :style="{ background: themeData.primary }"></div>
                  <div class="w-3 h-3 rounded-full" :style="{ background: themeData.secondary }"></div>
                  <div class="w-3 h-3 rounded-full" :style="{ background: themeData.accent }"></div>
                </div>
                <span class="text-[10px] text-base-content/50">{{ themeData.name }}</span>
              </button>
            </div>
          </div>

          <!-- cache -->
          <div 
            class="rounded-xl bg-base-content/5 p-4 border border-error/10 group cursor-help"
            title="clear cached summaries"
          >
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2.5">
                <div class="flex items-center justify-center w-6 h-6 rounded-md bg-error/15">
                  <Trash2 :size="12" class="text-error" />
                </div>
                <span class="text-[12px] font-medium text-base-content/70 tracking-wide">cache</span>
              </div>
              <button @click="clearCache" class="btn btn-sm btn-ghost text-[11px] text-error/70 hover:text-error hover:bg-error/10">
                clear
              </button>
            </div>
          </div>

          <!-- subtle setup guide link -->
          <div class="pt-2 border-t border-base-content/5 mt-2">
            <button 
              @click="openSetupGuide" 
              class="text-[10px] text-base-content/30 hover:text-base-content/50 transition-colors"
            >
              need help? view setup guide →
            </button>
          </div>
        </div>

        <!-- error state -->
        <div v-else-if="ollamaStatus === 'error'" class="flex flex-col items-center justify-center h-full p-6">
          <X :size="32" class="mb-3 text-error/50" />
          <p class="text-[12px] text-base-content/50 mb-3">connection error</p>
          <button @click="retryDetection" class="btn btn-sm btn-ghost text-[11px]">
            <RefreshCw :size="12" />
            retry
          </button>
        </div>
      </Transition>
    </main>
  </div>
</template>

<style scoped>
::-webkit-scrollbar {
  width: 3px;
  height: 3px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: oklch(from var(--bc) l c h / 0.08);
  border-radius: 3px;
}

::-webkit-scrollbar-thumb:hover {
  background: oklch(from var(--bc) l c h / 0.15);
}

* {
  scrollbar-width: thin;
  scrollbar-color: oklch(from var(--bc) l c h / 0.08) transparent;
}

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
