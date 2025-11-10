<script setup>
import { ref, onMounted, onUnmounted, nextTick, computed } from 'vue';
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
const toggleModelDropdown = () => {
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
    <!-- header with tabs -->
    <header 
      v-if="ollamaStatus === 'ready'"
      class="shrink-0 px-3 py-3 glass border-b border-base-300/50 sticky top-0 z-10"
      style="min-height: var(--header-height)"
    >
      <div role="tablist" class="tabs tabs-boxed p-0.5 bg-base-100/80 gap-0.5 shadow-depth-1">
        <button 
          role="tab"
          :class="['tab gap-1.5 h-8 text-sm', activeTab === 'summary' && 'tab-active']"
          @click="switchTab('summary')"
        >
          <Sparkles :size="14" :stroke-width="2.5" />
          <span class="font-medium">summary</span>
        </button>
        
        <button 
          role="tab"
          :class="['tab gap-1.5 h-8 text-sm', activeTab === 'stats' && 'tab-active']"
          @click="switchTab('stats')"
        >
          <BarChart3 :size="14" :stroke-width="2.5" />
          <span class="font-medium">stats</span>
        </button>
        
        <button 
          role="tab"
          :class="['tab gap-1.5 h-8 text-sm', activeTab === 'settings' && 'tab-active']"
          @click="switchTab('settings')"
        >
          <Settings :size="14" :stroke-width="2.5" />
          <span class="font-medium">settings</span>
        </button>
      </div>
    </header>

    <!-- main content -->
    <main class="flex-1 overflow-y-auto p-3">
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
          <div class="card bg-base-200 shadow-depth-2">
            <div class="card-body p-4">
              <h2 class="card-title text-sm leading-tight mb-2">
                <Server :size="14" :stroke-width="2.5" />
                model
              </h2>
              
              <div class="relative">
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

                <!-- dropdown menu -->
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
                    class="model-dropdown menu absolute top-full left-0 right-0 mt-1.5 p-1.5 bg-base-200 rounded-lg z-50 shadow-depth-3 border border-base-300 max-h-52 overflow-y-auto"
                  >
                    <li v-for="model in availableModels" :key="model">
                      <a 
                        @click="selectModel(model)"
                        :class="{ 'active': selectedModel === model }"
                        class="font-mono text-xs py-1.5"
                      >
                        {{ router.formatModelName(model) }}
                        <Check v-if="selectedModel === model" :size="14" :stroke-width="2.5" />
                      </a>
                    </li>
                  </ul>
                </Transition>
              </div>
            </div>
          </div>

          <!-- word lookup -->
          <div class="card bg-base-200 shadow-depth-2">
            <div class="card-body p-4">
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
              
              <div v-if="wordPopupEnabled" class="space-y-2 mt-2 pt-2 border-t border-base-300">
                <div class="form-control">
                  <label class="label py-1 px-0">
                    <span class="label-text text-xs opacity-60">behavior</span>
                  </label>
                  <select 
                    v-model="wordLookupPreference"
                    @change="saveWordLookupPreference"
                    class="select select-bordered select-sm text-sm"
                  >
                    <option value="auto">auto</option>
                    <option value="definition">define</option>
                    <option value="translation">translate</option>
                  </select>
                </div>
                
                <div class="form-control">
                  <label class="label py-1 px-0">
                    <span class="label-text text-xs opacity-60">source</span>
                  </label>
                  <select 
                    v-model="dictionarySource"
                    @change="saveDictionarySource"
                    class="select select-bordered select-sm text-sm"
                  >
                    <option value="api">online</option>
                    <option value="local">offline</option>
                  </select>
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
              <h2 class="card-title text-sm leading-tight mb-2">
                <Sparkles :size="14" :stroke-width="2.5" />
                appearance
              </h2>
              <div class="grid grid-cols-2 gap-1.5">
                <button
                  v-for="(themeData, key) in themeStore.themes"
                  :key="key"
                  @click="themeStore.setTheme(key)"
                  class="btn btn-sm text-xs"
                  :class="{ 'btn-primary': themeStore.currentTheme === key, 'btn-outline': themeStore.currentTheme !== key }"
                >
                  {{ themeData.name }}
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
</style>
