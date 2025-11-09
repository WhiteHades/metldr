<script setup>
import { ref, onMounted, onUnmounted, nextTick, computed } from 'vue';
import { useThemeStore } from './stores/theme.js';
import { OllamaClient } from './lib/OllamaClient.js';
import { CacheManager } from './lib/CacheManager.js';
import { ModelRouter } from './lib/ModelRouter.js';
import HistoryManager from './components/HistoryManager.vue';

const themeStore = useThemeStore();
const theme = computed(() => themeStore.colors);

const activeTab = ref('dashboard');
const ollamaStatus = ref('checking');
const availableModels = ref([]);
const selectedModel = ref('');
const historyRef = ref(null);
const showModelDropdown = ref(false);

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
  // persist model selection to chrome.storage
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
      
      // load saved model selection from chrome.storage
      try {
        const result = await chrome.storage.local.get(['selectedModel']);
        if (result.selectedModel && models.includes(result.selectedModel)) {
          selectedModel.value = result.selectedModel;
          console.log('metldr: loaded saved model:', result.selectedModel);
        } else {
          selectedModel.value = router.getModel('email_summary');
        }
      } catch (error) {
        console.error('metldr: failed to load saved model:', error);
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

onMounted(async () => {
  // load saved theme
  await themeStore.loadSavedTheme();
  
  await checkOllama();

  // close dropdown on click outside
  document.addEventListener('click', (e) => {
    if (showModelDropdown.value && !e.target.closest('.model-selector-btn') && !e.target.closest('.model-dropdown')) {
      showModelDropdown.value = false;
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
    class="fixed inset-0 flex flex-col overflow-hidden transition-colors duration-200"
    :style="{ 
      background: theme.bg,
      color: theme.text
    }"
  >
    <header 
      class="shrink-0 px-4 pt-3 pb-3 transition-all duration-150"
      :style="{ 
        borderBottom: `1px solid ${theme.border}`,
        background: theme.bgSecondary
      }"
    >
      <div v-if="ollamaStatus === 'ready'" class="flex gap-2">
        <button 
          v-for="tab in ['dashboard', 'settings']" 
          :key="tab"
          @click="switchTab(tab)"
          class="relative px-4 py-2 rounded-lg text-sm font-medium transition-all duration-200 capitalize"
          :style="activeTab === tab 
            ? { 
                color: theme.primary, 
                background: theme.bg,
                boxShadow: `0 1px 3px ${theme.shadow}`,
                borderWidth: '1px',
                borderColor: theme.border
              }
            : { 
                color: theme.textMuted, 
                background: 'transparent'
              }
          "
        >
          {{ tab }}
        </button>
      </div>
    </header>

    <main class="flex-1 overflow-y-auto px-4 py-4 scroll-smooth">
      <!-- checking state -->
      <Transition name="fade" mode="out-in">
        <div v-if="ollamaStatus === 'checking'" class="flex flex-col items-center justify-center py-24">
          <div 
            class="w-10 h-10 border-2 border-t-transparent rounded-full animate-spin mb-5 transition-all duration-200"
            :style="{ borderColor: theme.border, borderTopColor: theme.primary }"
          ></div>
          <p class="text-sm font-medium transition-colors duration-200" :style="{ color: theme.textMuted }">checking for ollama...</p>
        </div>

        <!-- not found state -->
        <div v-else-if="ollamaStatus === 'not-found'" class="space-y-4">
          <div 
            class="rounded-lg p-4 transition-all duration-200"
            :style="{ backgroundColor: theme.bgSecondary, borderWidth: '1px', borderColor: theme.border, boxShadow: `0 2px 8px ${theme.shadow}` }"
          >
            <p class="text-sm font-medium mb-4 transition-colors duration-200" :style="{ color: theme.text }">ollama not detected</p>
            <div 
              class="rounded-lg p-3 mb-4 transition-all duration-200"
              :style="{ backgroundColor: theme.bg, borderWidth: '1px', borderColor: theme.border }"
            >
              <p class="text-[10px] font-medium mb-2 uppercase tracking-wide transition-colors duration-200" :style="{ color: theme.textMuted }">setup instructions</p>
              <pre 
                class="text-xs font-mono overflow-x-auto whitespace-pre-wrap transition-colors duration-200 leading-relaxed"
                :style="{ color: theme.text }"
              >{{ setupCommands }}</pre>
              <button 
                @click="copySetupCommands" 
                class="mt-3 px-3 py-1.5 rounded-lg text-xs font-medium transition-all duration-200"
                :style="{ 
                  backgroundColor: `${theme.primary}10`, 
                  color: theme.primary,
                  borderWidth: '1px',
                  borderColor: `${theme.primary}30`
                }"
              >
                copy commands
              </button>
            </div>
            <button 
              @click="retryDetection" 
              class="px-4 py-2 rounded-lg text-sm font-medium transition-all duration-200"
              :style="{ 
                backgroundColor: theme.primary, 
                color: '#ffffff',
                boxShadow: `0 2px 4px ${theme.shadow}`
              }"
            >
              check again
            </button>
          </div>
        </div>

        <!-- dashboard -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'dashboard'" class="dashboard-content space-y-3">
          <div 
            class="flex items-center gap-2 px-3 py-2 rounded-lg transition-all duration-200"
            :style="{ 
              background: theme.bgSecondary,
              borderWidth: '1px', 
              borderColor: theme.border,
              boxShadow: `0 1px 2px ${theme.shadow}`
            }"
          >
            <div 
              class="w-1.5 h-1.5 rounded-full animate-pulse transition-all duration-200"
              :style="{ backgroundColor: theme.primary }"
            ></div>
            <span class="text-sm font-medium transition-colors duration-200" :style="{ color: theme.text }">
              {{ router.formatModelName(selectedModel) }}
            </span>
          </div>

          <HistoryManager ref="historyRef" :limit="10" />

          <div 
            class="text-center py-4 px-4 rounded-lg transition-all duration-200"
            :style="{ 
              background: theme.bgSecondary,
              borderWidth: '1px', 
              borderColor: theme.border
            }"
          >
            <p class="text-sm transition-colors duration-200" :style="{ color: theme.textMuted }">
              visit gmail to generate summaries
            </p>
          </div>
        </div>

        <!-- settings -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'settings'" class="settings-content space-y-4">
          <h2 
            class="text-sm font-semibold mb-3 pb-2 transition-all duration-200"
            :style="{ color: theme.text, borderBottomWidth: '1px', borderColor: theme.border }"
          >
            settings
          </h2>

          <div class="space-y-2 relative">
            <label class="text-[10px] font-semibold uppercase tracking-wider transition-colors duration-200" :style="{ color: theme.textMuted }">
              model selection
            </label>
            
            <!-- custom dropdown trigger -->
            <button 
              @click="toggleModelDropdown"
              class="model-selector-btn w-full px-3 py-2 rounded-lg text-sm text-left flex items-center justify-between transition-all duration-200"
              :style="{ 
                background: theme.bgSecondary,
                borderWidth: '1px', 
                borderColor: showModelDropdown ? theme.primary : theme.border,
                color: theme.text,
                boxShadow: showModelDropdown ? `0 2px 8px ${theme.shadow}` : 'none'
              }"
            >
              <span class="font-mono font-medium">{{ router.formatModelName(selectedModel) }}</span>
              <svg 
                class="w-4 h-4 transition-transform duration-200"
                :class="{ 'rotate-180': showModelDropdown }"
                :style="{ color: theme.primary }"
              fill="none" 
              stroke="currentColor" 
              viewBox="0 0 24 24"
            >
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path>
            </svg>
          </button>

            <!-- custom dropdown menu -->
            <Transition
              enter-active-class="transition-all duration-200 ease-out"
              enter-from-class="opacity-0 -translate-y-2"
              enter-to-class="opacity-100 translate-y-0"
              leave-active-class="transition-all duration-150 ease-in"
              leave-from-class="opacity-100 translate-y-0"
              leave-to-class="opacity-0 -translate-y-2"
            >
              <div 
                v-if="showModelDropdown" 
                class="model-dropdown absolute top-full left-0 right-0 mt-1 rounded-lg overflow-hidden z-50 transition-all duration-200"
                :style="{ 
                  maxHeight: '200px', 
                  overflowY: 'auto',
                  background: theme.bgSecondary,
                  borderWidth: '1px', 
                  borderColor: theme.border,
                  boxShadow: `0 4px 12px ${theme.shadow}`
                }"
              >
                <div 
                  v-for="model in availableModels" 
                  :key="model"
                  @click="selectModel(model)"
                  class="model-option px-3 py-2 text-sm cursor-pointer transition-all duration-150 last:border-0"
                  :style="{ 
                    color: selectedModel === model ? theme.primary : theme.text,
                    background: selectedModel === model ? `${theme.primary}10` : 'transparent',
                    borderBottomWidth: '1px',
                    borderColor: theme.border
                  }"
                >
                  <div class="flex items-center justify-between gap-3">
                    <span class="font-mono text-sm font-medium leading-relaxed break-all">{{ router.formatModelName(model) }}</span>
                    <span 
                      v-if="selectedModel === model" 
                      class="text-sm shrink-0 transition-colors duration-200"
                      :style="{ color: theme.primary }"
                    >✓</span>
                  </div>
                </div>
              </div>
            </Transition>
            
            <p class="text-[10px] leading-relaxed transition-colors duration-200" :style="{ color: theme.textMuted }">
              choose which ollama model to use for summaries
            </p>
          </div>

          <div class="space-y-2">
            <label class="text-[10px] font-semibold uppercase tracking-wider transition-colors duration-200" :style="{ color: theme.textMuted }">
              cache management
            </label>
            <div 
              class="flex items-center justify-between px-3 py-2 rounded-lg transition-all duration-200"
              :style="{ backgroundColor: theme.bgSecondary, borderWidth: '1px', borderColor: theme.border }"
            >
              <span class="text-sm transition-colors duration-200" :style="{ color: theme.text }">
                {{ availableModels.length }} models available
              </span>
              <button 
                @click="clearCache" 
                class="clear-cache-btn px-3 py-1.5 rounded-lg text-xs font-medium transition-all duration-200"
                :style="{ 
                  backgroundColor: `${theme.secondary}15`, 
                  color: theme.secondary,
                  borderWidth: '1px',
                  borderColor: `${theme.secondary}30`
                }"
              >
                clear cache
              </button>
            </div>
            <p class="text-[10px] leading-relaxed transition-colors duration-200" :style="{ color: theme.textMuted }">
              clear cached summaries to free up storage space
            </p>
          </div>

          <!-- theme selector -->
          <div class="space-y-2">
            <label class="text-[10px] font-semibold uppercase tracking-wider transition-colors duration-200" :style="{ color: theme.textMuted }">appearance</label>
          <div class="grid grid-cols-2 gap-2">
            <button
              v-for="(themeData, key) in themeStore.themes"
              :key="key"
              @click="themeStore.setTheme(key)"
              class="px-3 py-2 rounded-lg text-xs font-medium transition-all duration-200"
              :style="{
                background: themeStore.currentTheme === key 
                  ? `${themeData.primary}15`
                  : theme.bgSecondary,
                borderWidth: '1px',
                borderColor: themeStore.currentTheme === key ? themeData.primary : theme.border,
                color: themeStore.currentTheme === key ? themeData.primary : theme.text,
                boxShadow: themeStore.currentTheme === key ? `0 1px 3px ${themeData.shadow || theme.shadow}` : 'none'
              }"
            >
              {{ themeData.name }}
            </button>
          </div>
          <p class="text-[10px] leading-relaxed transition-colors duration-200" :style="{ color: theme.textMuted }">changes apply instantly across all views</p>
        </div>
      </div>

        <div v-else-if="ollamaStatus === 'error'" class="flex flex-col items-center justify-center py-20">
          <p class="mb-4 transition-colors duration-200" :style="{ color: theme.secondary }">connection error</p>
          <button 
            @click="retryDetection" 
            class="px-4 py-2 rounded-lg text-sm font-medium transition-all duration-150 hover:scale-105 active:scale-95"
            :style="{ 
              backgroundColor: `${theme.secondary}15`, 
              color: theme.secondary,
              borderWidth: '1px',
              borderColor: `${theme.secondary}40`
            }"
          >
            check again
          </button>
        </div>
      </Transition>
    </main>
  </div>
</template>

<style scoped>
* {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

.font-mono,
code,
pre {
  font-family: 'Courier New', 'Consolas', monospace !important;
  font-variant-ligatures: none;
}

main {
  scroll-behavior: smooth;
  overscroll-behavior: contain;
}

::-webkit-scrollbar {
  width: 6px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.1);
  border-radius: 3px;
  transition: background 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.2);
}

::selection {
  background: rgba(255, 255, 255, 0.2);
  color: inherit;
}

.fade-enter-active,
.fade-leave-active {
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.fade-enter-from {
  opacity: 0;
  transform: translateY(8px);
}

.fade-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}

* {
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: optimizeLegibility;
}

button,
.model-option {
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

button:hover:not(:disabled),
.model-option:hover {
  transform: translateY(-1px);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.1) !important;
}

button:active:not(:disabled),
.model-option:active {
  transform: translateY(0);
  transition-duration: 0.1s;
}
</style>
