<script setup>
import { ref, onMounted, onUnmounted, nextTick, computed, watch } from 'vue';
import { useThemeStore } from './stores/theme.js';
import { OllamaClient } from './lib/OllamaClient.js';
import { CacheManager } from './lib/CacheManager.js';
import { ModelRouter } from './lib/ModelRouter.js';
import HistoryManager from './components/HistoryManager.vue';
import { dictionaryDB, SUPPORTED_LANGUAGES } from './lib/DictionaryDB.js';
import { 
  Sparkles, BarChart3, Settings, Loader2, ChevronDown, Check, 
  Send, Trash2, X, RefreshCw, Globe, Database, 
  Zap, Server, Circle, MessageCircle
} from 'lucide-vue-next';

const themeStore = useThemeStore();
const theme = computed(() => themeStore.colors);

const activeTab = ref('summary');
const ollamaStatus = ref('checking');
const availableModels = ref([]);
const selectedModel = ref('');
const historyRef = ref(null);
const showModelDropdown = ref(false);
const showWordLookupDropdown = ref(false);
const showDictionaryDropdown = ref(false);

// dropdown position refs for fixed positioning
const modelDropdownPos = ref({ top: 0, left: 0, width: 0 });
const wordLookupDropdownPos = ref({ top: 0, left: 0, width: 0 });
const dictionaryDropdownPos = ref({ top: 0, left: 0, width: 0 });

// theme video refs
const themeVideoRefs = ref({});

// summary/chat state
const pageSummary = ref(null);
const summaryLoading = ref(false);
const summaryError = ref(null);
const expandedSections = ref(new Set());
const chatMessages = ref([]);
const chatInput = ref('');
const chatLoading = ref(false);

// word lookup settings
const wordPopupEnabled = ref(true);
const wordLookupPreference = ref('auto');
const dictionarySource = ref('api');

// dictionary settings
const downloadedLanguages = ref([]);
const selectedLanguages = ref(['en']);
const downloadProgress = ref({});

const setupCommands = `curl -fsSL https://ollama.com/install.sh | sh
OLLAMA_ORIGINS="chrome-extension://*" ollama serve`;

const client = new OllamaClient();
const cache = new CacheManager();
const router = new ModelRouter(client);

// custom dropdown logic
const updateDropdownPosition = (buttonElement, posRef) => {
  if (!buttonElement) return;
  const rect = buttonElement.getBoundingClientRect();
  posRef.value = {
    top: rect.bottom + 8,
    left: rect.left,
    width: rect.width,
  };
};

const toggleModelDropdown = () => {
  if (!showModelDropdown.value) {
    const btn = document.querySelector('.model-selector-btn');
    updateDropdownPosition(btn, modelDropdownPos);
  }
  showModelDropdown.value = !showModelDropdown.value;
};

const selectModel = async (model) => {
  selectedModel.value = model;
  showModelDropdown.value = false;
  try {
    await chrome.storage.local.set({ selectedModel: model });
    console.log('metldr: saved model selection:', model);
  } catch (error) {
    console.error('metldr: failed to save model selection:', error);
  }
};

// word lookup dropdown handlers
const toggleWordLookupDropdown = () => {
  if (!showWordLookupDropdown.value) {
    const btn = document.querySelector('.word-lookup-btn');
    updateDropdownPosition(btn, wordLookupDropdownPos);
  }
  showWordLookupDropdown.value = !showWordLookupDropdown.value;
};

const selectWordLookupMode = async (mode) => {
  wordLookupPreference.value = mode;
  showWordLookupDropdown.value = false;
  await saveWordLookupPreference();
};

// dictionary source dropdown handlers
const toggleDictionaryDropdown = () => {
  if (!showDictionaryDropdown.value) {
    const btn = document.querySelector('.dictionary-source-btn');
    updateDropdownPosition(btn, dictionaryDropdownPos);
  }
  showDictionaryDropdown.value = !showDictionaryDropdown.value;
};

const selectDictionarySource = async (source) => {
  dictionarySource.value = source;
  showDictionaryDropdown.value = false;
  await saveDictionarySource();
};

// theme video handlers
const handleVideoHover = (event, themeKey) => {
  const video = event.currentTarget?.querySelector('video') || event.target;
  if (video) {
    console.log(`metldr: hovering theme card ${themeKey}, attempting play`);
    video.play().then(() => {
      console.log(`metldr: video ${themeKey} playing on hover`);
    }).catch((err) => {
      console.error(`metldr: failed to play video ${themeKey} on hover:`, err);
    });
  }
};

const handleVideoLeave = (event, themeKey) => {
  const video = event.currentTarget?.querySelector('video') || event.target;
  if (video && themeStore.currentTheme !== themeKey) {
    console.log(`metldr: leaving theme card ${themeKey}, pausing`);
    video.pause();
    video.currentTime = 0;
  }
};

const setVideoRef = (el, themeKey) => {
  if (el) {
    themeVideoRefs.value[themeKey] = el;
    console.log(`metldr: video ref set for ${themeKey}, src=${el.src}`);
    
    // listen for load events
    el.addEventListener('loadstart', () => console.log(`metldr: loadstart for ${themeKey}`));
    el.addEventListener('loadedmetadata', () => console.log(`metldr: loadedmetadata for ${themeKey}, duration=${el.duration}`));
    el.addEventListener('canplay', () => console.log(`metldr: canplay for ${themeKey}`));
    el.addEventListener('error', (e) => console.error(`metldr: video error for ${themeKey}:`, e.target.error));
    
    // auto-play the active theme video after a brief delay to allow loading
    if (themeStore.currentTheme === themeKey) {
      setTimeout(() => {
        el.play().then(() => {
          console.log(`metldr: auto-started video for active theme ${themeKey}`);
        }).catch((err) => {
          console.error(`metldr: failed to auto-play video ${themeKey}:`, err);
        });
      }, 100);
    }
  }
};

// watch for theme changes to manage video playback
watch(() => themeStore.currentTheme, (newTheme, oldTheme) => {
  // pause old theme video
  if (oldTheme && themeVideoRefs.value[oldTheme]) {
    const oldVideo = themeVideoRefs.value[oldTheme];
    oldVideo.pause();
    oldVideo.currentTime = 0;
  }
  
  // play new theme video
  if (newTheme && themeVideoRefs.value[newTheme]) {
    nextTick(() => {
      const newVideo = themeVideoRefs.value[newTheme];
      newVideo.play().catch(() => {});
    });
  }
});

async function checkOllama() {
  try {
    await cache.init();
    
    const { connected, models } = await client.checkConnection();
    
    if (connected) {
      ollamaStatus.value = 'ready';
      availableModels.value = models;
      await router.detectModels();
      
      try {
        const result = await chrome.storage.local.get(['selectedModel']);
        if (result.selectedModel && models.includes(result.selectedModel)) {
          selectedModel.value = result.selectedModel;
        } else {
          selectedModel.value = router.getModel('email_summary');
        }
      } catch (error) {
        selectedModel.value = router.getModel('email_summary');
      }
      
      return true;
    }
    
    ollamaStatus.value = 'error';
    return false;
  } catch (error) {
    ollamaStatus.value = 'not-found';
    return false;
  }
}

function switchTab(tab) {
  if (activeTab.value === tab) return;
  activeTab.value = tab;
}

async function retryDetection() {
  ollamaStatus.value = 'checking';
  await checkOllama();
}

function copySetupCommands() {
  navigator.clipboard.writeText(setupCommands);
}

function toggleSection(index) {
  if (expandedSections.value.has(index)) {
    expandedSections.value.delete(index);
  } else {
    expandedSections.value.add(index);
  }
}

async function sendChatMessage() {
  if (!chatInput.value.trim() || chatLoading.value) return;
  
  const userMessage = chatInput.value.trim();
  chatInput.value = '';
  
  chatMessages.value.push({
    role: 'user',
    content: userMessage
  });
  
  chatLoading.value = true;
  
  try {
    const model = selectedModel.value || availableModels.value[0];
    
    let context = '';
    if (pageSummary.value) {
      context = `page context: ${pageSummary.value.metadata?.title}\n`;
      context += `summary: ${pageSummary.value.bullets.join('. ')}\n\n`;
    }
    
    const prompt = `${context}user question: ${userMessage}\n\nprovide a concise, helpful answer based on the page context.`;
    
    const response = await client.generate({
      model,
      prompt,
      stream: false
    });
    
    chatMessages.value.push({
      role: 'assistant',
      content: response.response
    });
  } catch (error) {
    console.error('metldr: chat error:', error);
    chatMessages.value.push({
      role: 'assistant',
      content: 'sorry, i couldn\'t process that request.'
    });
  } finally {
    chatLoading.value = false;
  }
}

async function toggleWordPopup() {
  wordPopupEnabled.value = !wordPopupEnabled.value;
  
  try {
    await chrome.storage.local.set({ wordPopupEnabled: wordPopupEnabled.value });
    console.log('metldr: word popup toggled to:', wordPopupEnabled.value);
    
    // notify all tabs about the change
    const tabs = await chrome.tabs.query({});
    tabs.forEach(tab => {
      chrome.tabs.sendMessage(tab.id, {
        type: 'WORD_POPUP_TOGGLED',
        enabled: wordPopupEnabled.value
      }).catch(() => {}); // ignore errors for tabs without content script
    });
  } catch (error) {
    console.error('metldr: failed to save word popup setting:', error);
  }
}

async function saveWordLookupPreference() {
  try {
    await chrome.storage.local.set({ wordLookupPreference: wordLookupPreference.value });
  } catch (error) {
    console.error('metldr: failed to save word lookup preference:', error);
  }
}

async function saveDictionarySource() {
  try {
    await chrome.storage.local.set({ dictionarySource: dictionarySource.value });
  } catch (error) {
    console.error('metldr: failed to save dictionary source:', error);
  }
}

async function clearCache() {
  if (!confirm('clear all cached summaries?')) return;
  
  try {
    await cache.init();
    alert('cache cleared!');
    if (historyRef.value) {
      historyRef.value.refresh();
    }
  } catch (error) {
    console.error('failed to clear cache:', error);
    alert('failed to clear cache');
  }
}

async function loadDictionarySettings() {
  try {
    await dictionaryDB.init();
    downloadedLanguages.value = await dictionaryDB.getDownloadedLanguages();
    
    const settings = await chrome.storage.local.get(['selectedLanguages']);
    if (settings.selectedLanguages && settings.selectedLanguages.length > 0) {
      selectedLanguages.value = settings.selectedLanguages;
    }
    
    if (!downloadedLanguages.value.includes('en')) {
      startDownload('en');
    }
  } catch (error) {
    console.error('metldr: failed to load dictionary settings:', error);
  }
}

async function startDownload(langCode) {
  downloadProgress.value[langCode] = {
    progress: 0,
    letter: 'a',
    entries: 0
  };
  
  try {
    await dictionaryDB.downloadLanguage(langCode, (progressData) => {
      downloadProgress.value[langCode] = {
        progress: progressData.progress,
        letter: progressData.letter,
        entries: progressData.entriesProcessed
      };
    });
    
    delete downloadProgress.value[langCode];
    downloadedLanguages.value = await dictionaryDB.getDownloadedLanguages();
  } catch (error) {
    console.error('metldr: download failed for', langCode, ':', error);
    delete downloadProgress.value[langCode];
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
    await dictionaryDB.deleteLanguage(langCode);
    downloadedLanguages.value = await dictionaryDB.getDownloadedLanguages();
    
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

async function handleBackgroundDownload(langCode) {
  if (downloadedLanguages.value.includes(langCode) || downloadProgress.value[langCode]) {
    return;
  }
  
  if (!selectedLanguages.value.includes(langCode)) {
    selectedLanguages.value.push(langCode);
    await chrome.storage.local.set({ selectedLanguages: [...selectedLanguages.value] });
  }
  
  startDownload(langCode);
}

onMounted(async () => {
  await themeStore.loadSavedTheme();
  
  try {
    const settings = await chrome.storage.local.get(['wordPopupEnabled', 'wordLookupPreference', 'dictionarySource']);
    if (settings.wordPopupEnabled !== undefined) {
      wordPopupEnabled.value = settings.wordPopupEnabled;
    }
    if (settings.wordLookupPreference) {
      wordLookupPreference.value = settings.wordLookupPreference;
    }
    if (settings.dictionarySource) {
      dictionarySource.value = settings.dictionarySource;
    }
  } catch (error) {
    console.error('metldr: failed to load word popup settings:', error);
  }
  
  await loadDictionarySettings();
  await checkOllama();

  document.addEventListener('click', (e) => {
    if (showModelDropdown.value && !e.target.closest('.model-selector-btn') && !e.target.closest('.model-dropdown')) {
      showModelDropdown.value = false;
    }
    if (showWordLookupDropdown.value && !e.target.closest('.word-lookup-btn') && !e.target.closest('.word-lookup-dropdown')) {
      showWordLookupDropdown.value = false;
    }
    if (showDictionaryDropdown.value && !e.target.closest('.dictionary-source-btn') && !e.target.closest('.dictionary-source-dropdown')) {
      showDictionaryDropdown.value = false;
    }
  });

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message.type === 'PAGE_SUMMARY') {
      pageSummary.value = message.summary;
      summaryLoading.value = false;
      summaryError.value = null;
    }
    
    if (message.action === 'startBackgroundDownload') {
      handleBackgroundDownload(message.language);
    }
  });

  const statusCheckInterval = setInterval(async () => {
    if (ollamaStatus.value !== 'ready') {
      await checkOllama();
    }
  }, 5000);

  onUnmounted(() => {
    clearInterval(statusCheckInterval);
  });
});
</script>

<template>
  <div 
    class="h-screen flex flex-col bg-base-100 text-base-content overflow-hidden"
  >
    <!-- teleport target for dropdowns (above main content) -->
    <div id="dropdown-portal" class="fixed inset-0 pointer-events-none z-50"></div>

    <!-- header with tabs -->
    <header 
      v-if="ollamaStatus === 'ready'"
      class="shrink-0 sticky top-0 z-10"
    >
      <div class="tabs tabs-box gap-2 p-3 bg-base-100/50 backdrop-blur-md border-b border-base-300/10">
        <input 
          type="radio" 
          name="main_tabs" 
          class="tab font-semibold transition-all"
          aria-label="summary"
          :checked="activeTab === 'summary'"
          @change="switchTab('summary')"
        />
        <input 
          type="radio" 
          name="main_tabs" 
          class="tab font-semibold transition-all"
          aria-label="stats"
          :checked="activeTab === 'stats'"
          @change="switchTab('stats')"
        />
        <input 
          type="radio" 
          name="main_tabs" 
          class="tab font-semibold transition-all"
          aria-label="settings"
          :checked="activeTab === 'settings'"
          @change="switchTab('settings')"
        />
      </div>
    </header>

    <!-- main content -->
    <main class="flex-1 overflow-x-hidden overflow-y-auto p-2 relative">
      <!-- checking state -->
      <Transition name="fade" mode="out-in">
        <div v-if="ollamaStatus === 'checking'" class="flex flex-col items-center justify-center h-full">
          <div class="relative">
            <div class="absolute inset-0 blur-xl opacity-30 bg-primary rounded-full animate-pulse"></div>
            <Loader2 class="w-10 h-10 mb-4 animate-spin text-primary relative z-10" :stroke-width="2" />
          </div>
          <p class="text-sm font-medium text-base-content/70">checking for ollama</p>
        </div>

        <!-- not found state -->
        <div v-else-if="ollamaStatus === 'not-found'" class="max-w-xl mx-auto">
          <div class="card bg-base-200 shadow-depth-2">
            <div class="card-body p-4">
              <h2 class="card-title text-sm leading-tight text-error mb-3">
                <Server :size="16" :stroke-width="2.5" />
                ollama not detected
              </h2>
              
              <div class="mockup-code mt-2 text-xs">
                <pre><code>{{ setupCommands }}</code></pre>
              </div>
              
              <div class="card-actions justify-end mt-3 gap-1.5">
                <button @click="copySetupCommands" class="btn btn-outline btn-primary btn-sm">
                  <Check :size="14" :stroke-width="2.5" />
                  copy commands
                </button>
                <button @click="retryDetection" class="btn btn-primary btn-sm">
                  <RefreshCw :size="14" :stroke-width="2.5" />
                  check again
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- summary tab -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'summary'" class="max-w-2xl mx-auto space-y-3">
          <!-- page context -->
          <div v-if="pageSummary" class="alert bg-base-200/80 border-base-300/50 py-2 min-h-0 shadow-depth-1">
            <Globe :size="14" :stroke-width="2.5" class="text-accent shrink-0" />
            <div class="flex-1 min-w-0">
              <h3 class="font-semibold text-sm leading-tight truncate">{{ pageSummary.metadata?.title || 'untitled page' }}</h3>
              <p class="text-xs opacity-60 leading-snug">{{ pageSummary.pageType }} • {{ pageSummary.metadata?.domain }}</p>
            </div>
          </div>

          <!-- loading state -->
          <div v-if="summaryLoading" class="alert bg-base-200/80 py-2 min-h-0 shadow-depth-1">
            <Loader2 class="w-4 h-4 animate-spin text-primary" :stroke-width="2.5" />
            <span class="text-sm font-medium">analysing page</span>
          </div>

          <!-- summary card -->
          <div v-else-if="pageSummary" class="card bg-base-200 shadow-depth-2">
            <div class="card-body p-4">
              <h2 class="card-title text-base leading-tight mb-2">
                <Sparkles :size="16" :stroke-width="2.5" class="text-primary" />
                <span>instant summary</span>
              </h2>
              
              <ul class="space-y-1.5 my-2">
                <li v-for="(bullet, i) in pageSummary.bullets" :key="i" class="flex gap-2.5 text-sm leading-relaxed">
                  <Circle :size="5" :fill="theme.primary" :stroke-width="0" class="mt-1.5 shrink-0" />
                  <span>{{ bullet }}</span>
                </li>
              </ul>
              
              <div class="flex items-center gap-3 pt-2 mt-2 border-t border-base-300">
                <div class="badge badge-accent badge-outline badge-sm gap-1">
                  <Zap :size="10" :stroke-width="2.5" />
                  {{ pageSummary.confidence }}% confidence
                </div>
                <span class="text-xs opacity-60">
                  {{ pageSummary.readTime || 'n/a' }} read
                </span>
              </div>
            </div>
          </div>

          <!-- sections -->
          <div v-if="pageSummary?.sections && pageSummary.sections.length > 0" class="space-y-1.5">
            <h3 class="text-xs font-semibold uppercase tracking-wider opacity-50 px-0.5 mb-1">sections</h3>
            <div 
              v-for="(section, i) in pageSummary.sections" 
              :key="i"
              class="collapse collapse-arrow bg-base-200/80 rounded-lg shadow-depth-1 hover:shadow-depth-2 transition-shadow"
            >
              <input type="checkbox" :checked="expandedSections.has(i)" @change="toggleSection(i)" />
              <div class="collapse-title font-medium text-sm py-2.5 min-h-0">{{ section.title }}</div>
              <div class="collapse-content pb-2.5">
                <p class="text-sm opacity-75 leading-relaxed">{{ section.content }}</p>
              </div>
            </div>
          </div>

          <!-- no summary state -->
          <div v-else-if="!summaryLoading && !pageSummary" class="alert bg-base-200/80 py-3 min-h-0 shadow-depth-1">
            <div class="flex-1 text-center">
              <p class="text-sm opacity-60">browse any page to see auto-summary</p>
            </div>
          </div>

          <!-- chat interface -->
          <div class="card bg-base-200 shadow-depth-2 mt-6">
            <div class="card-body p-4">
              <h2 class="card-title text-sm leading-tight mb-2">
                <MessageCircle :size="14" :stroke-width="2.5" />
                chat (optional)
              </h2>
              
              <!-- messages -->
              <div class="space-y-1.5 my-3 max-h-52 overflow-y-auto">
                <div v-for="(msg, i) in chatMessages" :key="i">
                  <div v-if="msg.role === 'user'" class="chat chat-end">
                    <div class="chat-bubble chat-bubble-primary text-sm py-2 px-3">{{ msg.content }}</div>
                  </div>
                  <div v-else class="chat chat-start">
                    <div class="chat-bubble text-sm py-2 px-3">{{ msg.content }}</div>
                  </div>
                </div>
                <div v-if="chatLoading" class="chat chat-start">
                  <div class="chat-bubble flex gap-1 py-2 px-3">
                    <span class="loading loading-dots loading-sm"></span>
                  </div>
                </div>
              </div>

              <!-- input -->
              <div class="join w-full">
                <input 
                  v-model="chatInput"
                  @keypress.enter="sendChatMessage"
                  type="text"
                  placeholder="ask anything..."
                  class="input input-bordered input-sm join-item flex-1 text-sm"
                />
                <button 
                  @click="sendChatMessage"
                  :disabled="!chatInput.trim() || chatLoading"
                  class="btn btn-primary btn-sm join-item"
                >
                  <Send :size="14" :stroke-width="2.5" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- stats tab -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'stats'" class="max-w-2xl mx-auto">
          <HistoryManager ref="historyRef" :limit="10" />
        </div>

        <!-- settings -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'settings'" class="max-w-2xl mx-auto space-y-3">
          <!-- model selection -->
          <div class="card bg-base-200 shadow-depth-2 overflow-visible">
            <div class="card-body p-4 overflow-visible">
              <h2 class="card-title text-sm leading-tight mb-2">
                <Server :size="14" :stroke-width="2.5" />
                model
              </h2>
              
              <div class="relative overflow-visible">
                <button 
                  @click="toggleModelDropdown"
                  class="model-selector-btn btn btn-sm btn-block justify-between"
                  :class="{ 'btn-primary': showModelDropdown }"
                >
                  <span class="font-mono text-xs">{{ router.formatModelName(selectedModel) }}</span>
                  <ChevronDown 
                    :size="14" 
                    :stroke-width="2.5"
                    class="transition-transform"
                    :class="{ 'rotate-180': showModelDropdown }"
                  />
                </button>

                <!-- dropdown menu (teleported to root) -->
                <Teleport to="#dropdown-portal">
                  <Transition
                    enter-active-class="transition-all duration-200 ease-out"
                    enter-from-class="opacity-0 scale-95 -translate-y-2"
                    enter-to-class="opacity-100 scale-100 translate-y-0"
                    leave-active-class="transition-all duration-150 ease-in"
                    leave-from-class="opacity-100 scale-100 translate-y-0"
                    leave-to-class="opacity-0 scale-95 -translate-y-2"
                  >
                    <ul 
                      v-if="showModelDropdown" 
                      class="model-dropdown menu fixed p-1.5 rounded-lg shadow-depth-3 border max-h-52 overflow-y-auto pointer-events-auto"
                      :style="{ 
                        top: modelDropdownPos.top + 'px', 
                        left: modelDropdownPos.left + 'px', 
                        width: modelDropdownPos.width + 'px', 
                        background: `linear-gradient(135deg, oklch(from var(--b2) l c h / 0.5), oklch(from var(--primary) l c h / 0.08))`,
                        backdropFilter: 'blur(24px)',
                        borderColor: `oklch(from var(--primary) l c h / 0.25)`,
                        boxShadow: `0 8px 32px oklch(from var(--primary) l c h / 0.15), inset 0 1px 0 oklch(from var(--bc) l c h / 0.1)`
                      }"
                    >
                      <li v-for="model in availableModels" :key="model">
                        <a 
                          @click="selectModel(model)"
                          :class="{ 'active': selectedModel === model }"
                          class="font-mono text-xs py-1.5"
                          :style="{ 
                            color: selectedModel === model ? 'oklch(from var(--bc) l c h)' : 'oklch(from var(--text) l c h)',
                            backgroundColor: selectedModel === model ? 'oklch(from var(--primary) l c h / 0.2)' : 'transparent',
                            borderRadius: '6px'
                          }"
                        >
                          {{ router.formatModelName(model) }}
                          <Check v-if="selectedModel === model" :size="14" :stroke-width="2.5" />
                        </a>
                      </li>
                    </ul>
                  </Transition>
                </Teleport>
              </div>
            </div>
          </div>

          <!-- word lookup -->
          <div class="card bg-base-200 shadow-depth-2 overflow-visible">
            <div class="card-body p-4 overflow-visible">
              <h2 class="card-title text-sm leading-tight mb-2">word lookup</h2>
              
              <div class="form-control">
                <label class="label cursor-pointer justify-between py-1.5 px-0">
                  <span class="label-text text-sm">enable popup</span>
                  <input 
                    type="checkbox" 
                    class="toggle toggle-primary toggle-sm" 
                    :checked="wordPopupEnabled"
                    @click="toggleWordPopup"
                  />
                </label>
              </div>
              
              <div v-if="wordPopupEnabled" class="space-y-2 mt-2 pt-2 border-t border-base-300 overflow-visible">
                <div class="form-control overflow-visible">
                  <label class="label py-1 px-0">
                    <span class="label-text text-xs opacity-60">behavior</span>
                  </label>
                  
                  <div class="relative overflow-visible">
                    <button 
                      @click="toggleWordLookupDropdown"
                      class="word-lookup-btn btn btn-sm btn-block justify-between"
                      :class="{ 'btn-primary': showWordLookupDropdown }"
                    >
                      <span class="font-mono text-xs capitalize">{{ wordLookupPreference }}</span>
                      <ChevronDown 
                        :size="14" 
                        :stroke-width="2.5"
                        class="transition-transform"
                        :class="{ 'rotate-180': showWordLookupDropdown }"
                      />
                    </button>

                    <!-- dropdown menu (teleported to root) -->
                    <Teleport to="#dropdown-portal">
                      <Transition
                        enter-active-class="transition-all duration-200 ease-out"
                        enter-from-class="opacity-0 scale-95 -translate-y-2"
                        enter-to-class="opacity-100 scale-100 translate-y-0"
                        leave-active-class="transition-all duration-150 ease-in"
                        leave-from-class="opacity-100 scale-100 translate-y-0"
                        leave-to-class="opacity-0 scale-95 -translate-y-2"
                      >
                        <ul 
                          v-if="showWordLookupDropdown" 
                          class="word-lookup-dropdown menu fixed p-1.5 rounded-lg shadow-depth-3 pointer-events-auto"
                          :style="{ 
                            top: wordLookupDropdownPos.top + 'px', 
                            left: wordLookupDropdownPos.left + 'px', 
                            width: wordLookupDropdownPos.width + 'px',
                            background: `linear-gradient(135deg, oklch(from var(--b2) l c h / 0.5), oklch(from var(--secondary) l c h / 0.08))`,
                            backdropFilter: 'blur(24px)',
                            borderColor: `oklch(from var(--secondary) l c h / 0.25)`,
                            boxShadow: `0 8px 32px oklch(from var(--secondary) l c h / 0.15), inset 0 1px 0 oklch(from var(--bc) l c h / 0.1)`
                          }"
                        >
                          <li>
                            <a 
                              @click="selectWordLookupMode('auto')"
                              :class="{ 'active': wordLookupPreference === 'auto' }"
                              class="font-mono text-xs py-1.5"
                              :style="{ 
                                color: wordLookupPreference === 'auto' ? 'oklch(from var(--bc) l c h)' : 'oklch(from var(--text) l c h)',
                                backgroundColor: wordLookupPreference === 'auto' ? 'oklch(from var(--secondary) l c h / 0.2)' : 'transparent',
                                borderRadius: '6px'
                              }"
                            >
                              auto
                              <Check v-if="wordLookupPreference === 'auto'" :size="14" :stroke-width="2.5" />
                            </a>
                          </li>
                          <li>
                            <a 
                              @click="selectWordLookupMode('definition')"
                              :class="{ 'active': wordLookupPreference === 'definition' }"
                              class="font-mono text-xs py-1.5"
                              :style="{ 
                                color: wordLookupPreference === 'definition' ? 'oklch(from var(--bc) l c h)' : 'oklch(from var(--text) l c h)',
                                backgroundColor: wordLookupPreference === 'definition' ? 'oklch(from var(--secondary) l c h / 0.2)' : 'transparent',
                                borderRadius: '6px'
                              }"
                            >
                              define
                              <Check v-if="wordLookupPreference === 'definition'" :size="14" :stroke-width="2.5" />
                            </a>
                          </li>
                          <li>
                            <a 
                              @click="selectWordLookupMode('translation')"
                              :class="{ 'active': wordLookupPreference === 'translation' }"
                              class="font-mono text-xs py-1.5"
                              :style="{ 
                                color: wordLookupPreference === 'translation' ? 'oklch(from var(--bc) l c h)' : 'oklch(from var(--text) l c h)',
                                backgroundColor: wordLookupPreference === 'translation' ? 'oklch(from var(--secondary) l c h / 0.2)' : 'transparent',
                                borderRadius: '6px'
                              }"
                            >
                            translate
                            <Check v-if="wordLookupPreference === 'translation'" :size="14" :stroke-width="2.5" />
                          </a>
                        </li>
                      </ul>
                    </Transition>
                    </Teleport>
                  </div>
                </div>
                
                <div class="form-control overflow-visible">
                  <label class="label py-1 px-0">
                    <span class="label-text text-xs opacity-60">source</span>
                  </label>
                  
                  <div class="relative overflow-visible">
                    <button 
                      @click="toggleDictionaryDropdown"
                      class="dictionary-source-btn btn btn-sm btn-block justify-between"
                      :class="{ 'btn-primary': showDictionaryDropdown }"
                    >
                      <span class="font-mono text-xs capitalize">{{ dictionarySource }}</span>
                      <ChevronDown 
                        :size="14" 
                        :stroke-width="2.5"
                        class="transition-transform"
                        :class="{ 'rotate-180': showDictionaryDropdown }"
                      />
                    </button>
                    <!-- dropdown menu (teleported to root) -->
                    <Teleport to="#dropdown-portal">
                      <Transition
                        enter-active-class="transition-all duration-200 ease-out"
                        enter-from-class="opacity-0 scale-95 -translate-y-2"
                        enter-to-class="opacity-100 scale-100 translate-y-0"
                        leave-active-class="transition-all duration-150 ease-in"
                        leave-from-class="opacity-100 scale-100 translate-y-0"
                        leave-to-class="opacity-0 scale-95 -translate-y-2"
                      >
                        <ul 
                          v-if="showDictionaryDropdown" 
                          class="dictionary-source-dropdown menu fixed p-1.5 rounded-lg shadow-depth-3 pointer-events-auto"
                          :style="{ 
                            top: dictionaryDropdownPos.top + 'px', 
                            left: dictionaryDropdownPos.left + 'px', 
                            width: dictionaryDropdownPos.width + 'px',
                            background: `linear-gradient(135deg, oklch(from var(--b2) l c h / 0.5), oklch(from var(--accent) l c h / 0.08))`,
                            backdropFilter: 'blur(24px)',
                            borderColor: `oklch(from var(--accent) l c h / 0.25)`,
                            boxShadow: `0 8px 32px oklch(from var(--accent) l c h / 0.15), inset 0 1px 0 oklch(from var(--bc) l c h / 0.1)`
                          }"
                        >
                          <li>
                            <a 
                              @click="selectDictionarySource('api')"
                              :class="{ 'active': dictionarySource === 'api' }"
                              class="font-mono text-xs py-1.5"
                              :style="{ 
                                color: dictionarySource === 'api' ? 'oklch(from var(--bc) l c h)' : 'oklch(from var(--text) l c h)',
                                backgroundColor: dictionarySource === 'api' ? 'oklch(from var(--accent) l c h / 0.2)' : 'transparent',
                                borderRadius: '6px'
                              }"
                            >
                              online
                              <Check v-if="dictionarySource === 'api'" :size="14" :stroke-width="2.5" />
                            </a>
                          </li>
                          <li>
                            <a 
                              @click="selectDictionarySource('local')"
                              :class="{ 'active': dictionarySource === 'local' }"
                              class="font-mono text-xs py-1.5"
                              :style="{ 
                                color: dictionarySource === 'local' ? 'oklch(from var(--bc) l c h)' : 'oklch(from var(--text) l c h)',
                                backgroundColor: dictionarySource === 'local' ? 'oklch(from var(--accent) l c h / 0.2)' : 'transparent',
                                borderRadius: '6px'
                              }"
                            >
                              offline
                              <Check v-if="dictionarySource === 'local'" :size="14" :stroke-width="2.5" />
                            </a>
                          </li>
                        </ul>
                      </Transition>
                    </Teleport>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- offline dictionaries -->
          <div class="card bg-base-200 shadow-depth-2">
            <div class="card-body p-4">
              <h2 class="card-title text-sm leading-tight mb-2">
                <Database :size="14" :stroke-width="2.5" />
                offline dictionaries
              </h2>
              
              <div class="space-y-1.5 max-h-60 overflow-y-auto">
                <div v-for="lang in SUPPORTED_LANGUAGES" :key="lang.code" class="form-control">
                  <label class="label cursor-pointer justify-start gap-2.5 py-1.5 px-0">
                    <input 
                      type="checkbox"
                      :checked="selectedLanguages.includes(lang.code)"
                      @change="toggleLanguage(lang.code)"
                      class="checkbox checkbox-primary checkbox-sm"
                    />
                    <span class="label-text text-sm flex-1">{{ lang.name }}</span>
                    
                    <!-- status badges -->
                    <span 
                      v-if="downloadProgress[lang.code]"
                      class="badge badge-warning badge-xs"
                    >
                      {{ downloadProgress[lang.code].progress.toFixed(0) }}%
                    </span>
                    <span 
                      v-else-if="downloadedLanguages.includes(lang.code)"
                      class="badge badge-success badge-xs"
                    >
                      ✓
                    </span>
                    
                    <button
                      v-if="downloadedLanguages.includes(lang.code)"
                      @click.stop="deleteLanguageData(lang.code)"
                      class="btn btn-ghost btn-xs text-error h-6 min-h-0 px-1.5"
                    >
                      ✕
                    </button>
                  </label>
                  
                  <!-- progress bar -->
                  <div v-if="downloadProgress[lang.code]" class="ml-7 space-y-0.5">
                    <progress 
                      class="progress progress-primary w-full h-1.5" 
                      :value="downloadProgress[lang.code].progress" 
                      max="100"
                    ></progress>
                    <div class="flex justify-between text-xs opacity-50">
                      <span>{{ downloadProgress[lang.code].letter }}</span>
                      <span>{{ downloadProgress[lang.code].entries.toLocaleString() }}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- cache -->
          <div class="card bg-base-200 shadow-depth-2">
            <div class="card-body p-4">
              <h2 class="card-title text-sm leading-tight mb-2">
                <Database :size="14" :stroke-width="2.5" />
                cache
              </h2>
              <button @click="clearCache" class="btn btn-outline btn-error btn-sm">
                <Trash2 :size="14" :stroke-width="2.5" />
                clear all summaries
              </button>
            </div>
          </div>

          <!-- theme selector -->
          <div class="card bg-base-200 shadow-depth-2">
            <div class="card-body p-4">
              <h2 class="card-title text-sm leading-tight mb-3">
                <Sparkles :size="14" :stroke-width="2.5" />
                appearance
              </h2>
              <div class="grid grid-cols-1 gap-3">
                <button
                  v-for="(themeData, key) in themeStore.themes"
                  :key="key"
                  @click="themeStore.setTheme(key)"
                  @mouseenter="(e) => handleVideoHover(e, key)"
                  @mouseleave="(e) => handleVideoLeave(e, key)"
                  class="theme-card-btn group"
                  :class="{ 'is-active': themeStore.currentTheme === key }"
                >
                  <!-- video background -->
                  <div class="theme-video-container">
                    <video
                      :src="themeData.video"
                      muted
                      loop
                      playsinline
                      class="theme-video"
                      :ref="(el) => setVideoRef(el, key)"
                    ></video>
                    <div class="theme-video-overlay"></div>
                  </div>
                  
                  <!-- theme name -->
                  <div class="theme-text-container">
                    <span class="theme-name">{{ themeData.name }}</span>
                    <div v-if="themeStore.currentTheme === key" class="theme-check">
                      <Check :size="12" :stroke-width="3" />
                    </div>
                  </div>
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- error state -->
        <div v-else-if="ollamaStatus === 'error'" class="flex flex-col items-center justify-center h-full">
          <X :size="40" :stroke-width="2" class="mb-3 text-error" />
          <p class="text-base font-semibold text-error mb-3 leading-tight">connection error</p>
          <button @click="retryDetection" class="btn btn-error btn-outline btn-sm">
            <RefreshCw :size="14" :stroke-width="2.5" />
            check again
          </button>
        </div>
      </Transition>
    </main>
  </div>
</template>

<style scoped>
/* scrollbar */
main::-webkit-scrollbar {
  width: 6px;
}

main::-webkit-scrollbar-track {
  background: transparent;
}

main::-webkit-scrollbar-thumb {
  background: oklch(from var(--bc) l c h / 0.2);
  border-radius: 4px;
}

main::-webkit-scrollbar-thumb:hover {
  background: oklch(from var(--bc) l c h / 0.3);
}

/* dropdown overflow fix: ensure dropdowns don't get clipped by parent overflow */
.card.overflow-visible {
  overflow: visible !important;
}

.card-body.overflow-visible {
  overflow: visible !important;
}

.form-control.overflow-visible {
  overflow: visible !important;
}

.space-y-2.overflow-visible {
  overflow: visible !important;
}

.relative.overflow-visible {
  overflow: visible !important;
}

/* ensure dropdown menus stack above everything */
.model-dropdown,
.word-lookup-dropdown,
.dictionary-source-dropdown {
  z-index: 9999 !important;
  position: absolute !important;
}

.fade-enter-active,
.fade-leave-active {
  transition: all var(--transition-normal) cubic-bezier(0.32, 0.72, 0, 1);
}

.fade-enter-from {
  opacity: 0;
  transform: translateY(8px) scale(0.98);
}

.fade-leave-to {
  opacity: 0;
  transform: translateY(-8px) scale(0.98);
}

/* refined interactions */
.tab {
  transition: all var(--transition-fast);
}

.tab:hover {
  transform: translateY(-1px);
}

.btn {
  transition: all var(--transition-fast);
}

.btn:hover {
  transform: translateY(-1px);
  box-shadow: var(--shadow-sm);
}

.btn:active {
  transform: translateY(0);
}

.card {
  transition: box-shadow var(--transition-normal);
  background: oklch(from var(--b2) l c h / 0.6);
  backdrop-filter: blur(12px);
  border: 1px solid oklch(from var(--bc) l c h / 0.05);
}

.card-body {
  padding: 1rem;
}

.card-title {
  gap: 8px;
}

.model-dropdown::-webkit-scrollbar {
  width: 6px;
}

.model-dropdown::-webkit-scrollbar-track {
  background: transparent;
}

.model-dropdown::-webkit-scrollbar-thumb {
  background: oklch(from var(--bc) l c h / 0.2);
  border-radius: 4px;
}

.model-dropdown::-webkit-scrollbar-thumb:hover {
  background: oklch(from var(--bc) l c h / 0.3);
}

/* theme selector styles - following design philosophy */
.theme-card-btn {
  position: relative;
  display: block;
  width: 100%;
  height: 80px;
  border-radius: 16px;
  overflow: hidden;
  border: 2px solid transparent;
  cursor: pointer;
  transition: all 280ms cubic-bezier(0.32, 0.72, 0, 1);
  background: var(--b2);
  /* depth: multi-shadow technique */
  box-shadow: 
    0 2px 4px oklch(from var(--bc) l c h / 0.08),
    0 8px 16px oklch(from var(--bc) l c h / 0.12);
}

.theme-card-btn:hover {
  transform: translateY(-2px);
  /* deeper shadow on hover */
  box-shadow: 
    0 4px 8px oklch(from var(--bc) l c h / 0.12),
    0 12px 24px oklch(from var(--bc) l c h / 0.16);
  border-color: oklch(from var(--p) l c h / 0.3);
}

.theme-card-btn.is-active {
  border-color: var(--p);
  /* raised effect for active state */
  box-shadow: 
    0 0 0 1px var(--p),
    0 4px 8px oklch(from var(--p) l c h / 0.2),
    0 12px 24px oklch(from var(--p) l c h / 0.25);
  transform: translateY(-1px);
}

.theme-card-btn:active {
  transform: translateY(0) scale(0.99);
}

/* video background container */
.theme-video-container {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
  border-radius: 14px;
}

.theme-video {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  transition: all 400ms cubic-bezier(0.32, 0.72, 0, 1);
  filter: brightness(0.7) saturate(0.8) grayscale(0.3);
}

/* hover state: brighten and enhance video */
.theme-card-btn:hover .theme-video {
  filter: brightness(1) saturate(1.1) grayscale(0);
  transform: scale(1.05);
}

/* active state: keep video bright */
.theme-card-btn.is-active .theme-video {
  filter: brightness(0.9) saturate(1) grayscale(0);
}

/* non-active state: heavily greyed out */
.theme-card-btn:not(.is-active):not(:hover) .theme-video {
  filter: brightness(0.4) saturate(0.3) grayscale(0.7);
}

/* overlay for text legibility - gradient from bottom */
.theme-video-overlay {
  position: absolute;
  inset: 0;
  background: linear-gradient(
    to top,
    oklch(0 0 0 / 0.85) 0%,
    oklch(0 0 0 / 0.5) 50%,
    oklch(0 0 0 / 0.2) 100%
  );
  transition: opacity 280ms ease;
}

.theme-card-btn:hover .theme-video-overlay {
  background: linear-gradient(
    to top,
    oklch(0 0 0 / 0.75) 0%,
    oklch(0 0 0 / 0.4) 50%,
    oklch(0 0 0 / 0.15) 100%
  );
}

/* text container - positioned at bottom */
.theme-text-container {
  position: relative;
  z-index: 10;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  height: 100%;
  align-items: flex-end;
}

/* theme name typography - following design philosophy */
.theme-name {
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0.01em;
  line-height: 1.3;
  color: oklch(0.95 0.02 265);
  text-shadow: 
    0 1px 2px oklch(0 0 0 / 0.4),
    0 2px 8px oklch(0 0 0 / 0.3);
  transition: all 280ms ease;
  /* enhanced contrast for accessibility */
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

.theme-card-btn:hover .theme-name {
  color: oklch(1 0 0);
  text-shadow: 
    0 1px 3px oklch(0 0 0 / 0.5),
    0 3px 12px oklch(0 0 0 / 0.4);
  transform: translateX(2px);
}

.theme-card-btn.is-active .theme-name {
  color: oklch(1 0 0);
  font-weight: 700;
}

/* check indicator for active theme */
.theme-check {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--p);
  color: var(--pc);
  box-shadow: 
    0 2px 4px oklch(from var(--p) l c h / 0.3),
    0 0 0 2px oklch(1 0 0 / 0.2);
  animation: checkPop 300ms cubic-bezier(0.68, -0.55, 0.265, 1.55);
}

@keyframes checkPop {
  0% {
    transform: scale(0);
    opacity: 0;
  }
  50% {
    transform: scale(1.2);
  }
  100% {
    transform: scale(1);
    opacity: 1;
  }
}

/* header tabs styling - minimal, clean, glass-feel */
header .tabs {
  gap: 4px;
}

/* hide tab content areas (we handle them separately) */
header .tab-content {
  display: none;
}

/* enhance tab styling - inspired by theme card buttons */
header .tab {
  padding: 10px 16px;
  border-radius: 10px;
  transition: all 280ms cubic-bezier(0.32, 0.72, 0, 1);
  background: oklch(from var(--bc) l c h / 0.06);
  border: 1.5px solid transparent;
}

header .tab:hover {
  background: oklch(from var(--bc) l c h / 0.1);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px oklch(from var(--bc) l c h / 0.1);
}

header .tab:checked {
  background: oklch(from var(--p) l c h / 0.15);
  border-color: oklch(from var(--p) l c h / 0.4);
  color: var(--p);
  font-weight: 700;
  transform: translateY(-2px);
  box-shadow: 0 4px 16px oklch(from var(--p) l c h / 0.15);
}

header .tab:checked:hover {
  background: oklch(from var(--p) l c h / 0.2);
  transform: translateY(-3px);
}
</style>
