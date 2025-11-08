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

const selectModel = (model) => {
  selectedModel.value = model;
  showModelDropdown.value = false;
};

async function checkOllama() {
  try {
    await cache.init();
    
    const { connected, models } = await client.checkConnection();
    
    if (connected) {
      ollamaStatus.value = 'ready';
      availableModels.value = models;
      await router.detectModels();
      
      selectedModel.value = router.getModel('email_summary');
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
    class="fixed inset-0 text-zinc-100 flex flex-col overflow-hidden transition-colors duration-200"
    :style="{ 
      background: `linear-gradient(135deg, ${theme.bg}, ${theme.bgSecondary})`
    }"
  >
    <header 
      class="shrink-0 px-5 pt-4 pb-3 transition-all duration-200"
      :style="{ 
        background: `linear-gradient(180deg, ${theme.bgSecondary}60 0%, transparent 100%)`,
        borderBottom: `0.5px solid ${theme.border}40`
      }"
    >
      <p class="text-[9px] font-semibold mb-3 transition-colors duration-200 tracking-tight" :style="{ color: theme.textMuted }">
        MeTLDR · Local & Private
      </p>
      
      <div v-if="ollamaStatus === 'ready'" class="flex gap-2 mt-3">
        <button 
          v-for="tab in ['dashboard', 'settings']" 
          :key="tab"
          @click="switchTab(tab)"
          class="relative px-4 py-2 rounded-xl text-xs font-semibold transition-all duration-150  "
          :style="activeTab === tab 
            ? { 
                color: theme.primary, 
                background: `linear-gradient(135deg, ${theme.bgSecondary}80, ${theme.bgSecondary}60)`,
                boxShadow: `0 2px 12px ${theme.glow}20, inset 0 1px 0 ${theme.border}30`,
                borderWidth: '0.5px',
                borderColor: `${theme.border}60`,
                backdropFilter: 'blur(20px)'
              }
            : { 
                color: theme.textMuted, 
                background: 'transparent'
              }
          "
        >
          <span class="relative z-10">{{ tab }}</span>
        </button>
      </div>
    </header>

    <main class="flex-1 overflow-y-auto px-6 py-4 scroll-smooth">
      <!-- checking state -->
      <Transition name="fade" mode="out-in">
        <div v-if="ollamaStatus === 'checking'" class="flex flex-col items-center justify-center py-20">
          <div 
            class="w-8 h-8 border-2 border-t-transparent rounded-full animate-spin mb-4 transition-all duration-200"
            :style="{ borderColor: `${theme.primary}40`, borderTopColor: 'transparent', boxShadow: `0 0 15px ${theme.glow}` }"
          ></div>
          <p class="text-sm transition-colors duration-200" :style="{ color: theme.textMuted }">checking for ollama...</p>
        </div>

        <!-- not found state -->
        <div v-else-if="ollamaStatus === 'not-found'" class="space-y-4">
          <div 
            class="rounded-xl p-5 backdrop-blur-sm transition-all duration-200 "
            :style="{ backgroundColor: `${theme.bgSecondary}80`, borderWidth: '1px', borderColor: theme.border, boxShadow: `0 4px 20px ${theme.glow}` }"
          >
            <p class="text-sm mb-4 transition-colors duration-200" :style="{ color: theme.text }">ollama not detected</p>
            <div 
              class="rounded-lg p-3 mb-4 transition-all duration-200"
              :style="{ backgroundColor: `${theme.bg}60`, borderWidth: '1px', borderColor: theme.border }"
            >
              <p class="text-[10px] mb-2 transition-colors duration-200" :style="{ color: theme.textMuted }">to get started:</p>
              <pre 
                class="text-[11px] font-mono overflow-x-auto whitespace-pre-wrap transition-colors duration-200"
                :style="{ color: theme.primary }"
              >{{ setupCommands }}</pre>
              <button 
                @click="copySetupCommands" 
                class="mt-3 px-3 py-1.5 rounded-md text-[10px] font-medium transition-all duration-150 hover:scale-105 active:scale-95"
                :style="{ 
                  backgroundColor: `${theme.primary}15`, 
                  color: theme.primary,
                  borderWidth: '1px',
                  borderColor: `${theme.primary}40`
                }"
              >
                copy commands
              </button>
            </div>
            <button 
              @click="retryDetection" 
              class="px-4 py-2 rounded-lg text-xs font-medium transition-all duration-150 hover:scale-105 active:scale-95"
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
        </div>

        <!-- dashboard -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'dashboard'" class="dashboard-content space-y-3">
          <div 
            class="flex items-center gap-2 px-3 py-2 rounded-xl transition-all duration-200"
            :style="{ 
              background: theme.bgSecondary,
              boxShadow: `0 1px 8px ${theme.glow}10, inset 0 0.5px 0 ${theme.border}30`,
              borderWidth: '0.5px', 
              borderColor: `${theme.primary}30`,
              boxShadow: `0 2px 10px ${theme.glow}`
            }"
          >
            <div 
              class="w-2 h-2 rounded-full animate-pulse transition-all duration-200"
              :style="{ backgroundColor: theme.primary, boxShadow: `0 0 8px ${theme.primary}` }"
            ></div>
            <span class="text-xs font-medium transition-colors duration-200" :style="{ color: theme.primary }">
              {{ router.formatModelName(selectedModel) }}
            </span>
          </div>

          <HistoryManager ref="historyRef" :limit="10" />

          <div 
            class="text-center py-5 px-4 rounded-xl transition-all duration-200"
            :style="{ 
              background: theme.bgSecondary,
              boxShadow: `0 1px 8px ${theme.glow}08, inset 0 0.5px 0 ${theme.border}20`,
              borderWidth: '0.5px', 
              borderColor: `${theme.border}40`
            }"
          >
            <p class="text-xs font-medium transition-colors duration-200" :style="{ color: theme.textMuted }">
              Visit Gmail to generate summaries
            </p>
          </div>
        </div>

        <!-- settings -->
        <div v-else-if="ollamaStatus === 'ready' && activeTab === 'settings'" class="settings-content space-y-4">
          <h2 
            class="text-sm font-semibold mb-3 pb-2 transition-all duration-200 tracking-tight"
            :style="{ color: theme.text, borderBottomWidth: '0.5px', borderColor: `${theme.border}40` }"
          >
            Settings
          </h2>

          <div class="space-y-2 relative">
            <label class="text-[10px] font-semibold uppercase tracking-wide transition-colors duration-200" :style="{ color: theme.textMuted }">
              Model
            </label>
            
            <!-- custom dropdown trigger -->
            <button 
              @click="toggleModelDropdown"
              class="model-selector-btn w-full px-3 py-2.5 rounded-xl text-xs text-left flex items-center justify-between transition-all duration-150  "
              :style="{ 
                background: theme.bgSecondary,
                boxShadow: showModelDropdown ? `0 2px 12px ${theme.glow}20, inset 0 0.5px 0 ${theme.border}30` : `0 1px 8px ${theme.glow}08`,
                borderWidth: '0.5px', 
                borderColor: showModelDropdown ? `${theme.primary}60` : `${theme.border}40`,
                color: theme.text
              }"
            >
              <span class="font-mono font-semibold">{{ router.formatModelName(selectedModel) }}</span>
              <svg 
                class="w-4 h-4 transition-all duration-150"
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
              enter-from-class="opacity-0 -translate-y-2 scale-95"
              enter-to-class="opacity-100 translate-y-0 scale-100"
              leave-active-class="transition-all duration-150 ease-in"
              leave-from-class="opacity-100 translate-y-0 scale-100"
              leave-to-class="opacity-0 -translate-y-2 scale-95"
            >
              <div 
                v-if="showModelDropdown" 
                class="model-dropdown absolute top-full left-0 right-0 mt-1 rounded-xl overflow-y-auto z-50 transition-all duration-200"
                :style="{ 
                  maxHeight: '240px', 
                  background: theme.bgSecondary,
                  borderWidth: '0.5px', 
                  borderColor: `${theme.border}60`,
                  boxShadow: `0 8px 32px ${theme.glow}20, 0 2px 16px ${theme.bg}40, inset 0 0.5px 0 ${theme.border}30`
                }"
              >
                <div 
                  v-for="model in availableModels" 
                  :key="model"
                  @click="selectModel(model)"
                  class="model-option px-3 py-2 text-xs cursor-pointer transition-all duration-150 last:border-0"
                  :style="{ 
                    color: selectedModel === model ? theme.primary : theme.text,
                    background: selectedModel === model ? `linear-gradient(135deg, ${theme.primary}15, ${theme.primary}08)` : 'transparent',
                    borderBottomWidth: '0.5px',
                    borderColor: `${theme.border}30`
                  }"
                >
                  <div class="flex items-center justify-between gap-3">
                    <span class="font-mono text-[11px] font-medium leading-relaxed break-all">{{ router.formatModelName(model) }}</span>
                    <span 
                      v-if="selectedModel === model" 
                      class="text-[10px] shrink-0 transition-colors duration-200"
                      :style="{ color: theme.primary }"
                    >✓</span>
                  </div>
                </div>
              </div>
            </Transition>
            
            <p class="text-[10px] transition-colors duration-200" :style="{ color: theme.textMuted }">
              choose which ollama model to use for summaries
            </p>
          </div>

          <div class="space-y-2">
            <label class="text-xs font-medium transition-colors duration-200" :style="{ color: theme.textMuted }">
              cache management
            </label>
            <div 
              class="flex items-center justify-between px-3 py-2.5 rounded-lg backdrop-blur-sm transition-all duration-200"
              :style="{ backgroundColor: `${theme.bgSecondary}60`, borderWidth: '1px', borderColor: theme.border }"
            >
              <span class="text-xs transition-colors duration-200" :style="{ color: theme.text }">
                {{ availableModels.length }} models available
              </span>
              <button 
                @click="clearCache" 
                class="clear-cache-btn px-3 py-1.5 rounded-md text-[10px] font-medium transition-all duration-150 hover:scale-105 active:scale-95"
                :style="{ 
                  backgroundColor: `${theme.secondary}15`, 
                  color: theme.secondary,
                  borderWidth: '1px',
                  borderColor: `${theme.secondary}40`
                }"
              >
                clear cache
              </button>
            </div>
            <p class="text-[10px] transition-colors duration-200" :style="{ color: theme.textMuted }">
              clear cached summaries to free up space
            </p>
          </div>

          <!-- theme selector -->
          <div class="space-y-2">
            <label class="text-[10px] font-semibold uppercase tracking-wide transition-colors duration-200" :style="{ color: theme.textMuted }">Appearance</label>
          <div class="grid grid-cols-3 gap-2">
            <button
              v-for="(themeData, key) in themeStore.themes"
              :key="key"
              @click="themeStore.setTheme(key)"
              class="px-3 py-3 rounded-xl text-[10px] font-semibold transition-all duration-150  "
              :style="{
                background: themeStore.currentTheme === key 
                  ? `${themeData.primary}25`
                  : `${themeData.primary}10`,
                boxShadow: themeStore.currentTheme === key 
                  ? `0 2px 12px ${themeData.primary}30, inset 0 0.5px 0 ${themeData.primary}40`
                  : `0 1px 8px ${themeData.primary}10`,
                borderWidth: '0.5px',
                borderColor: themeStore.currentTheme === key ? `${themeData.primary}60` : `${themeData.primary}30`,
                color: themeData.primary
              }"
            >
              {{ themeData.name.split(' ')[0] }}
            </button>
          </div>
          <p class="text-[9px] transition-colors duration-200" :style="{ color: theme.textMuted }">Changes apply instantly</p>
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
  transition: all 0.4s cubic-bezier(0.4, 0, 0.2, 1);
}

.fade-enter-from {
  opacity: 0;
  transform: translateY(10px);
}

.fade-leave-to {
  opacity: 0;
  transform: translateY(-10px);
}

* {
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: optimizeLegibility;
  backface-visibility: hidden;
}

button,
.model-option,
.stat-card,
.history-item {
  transition: background-color 0.2s cubic-bezier(0.4, 0, 0.2, 1),
              border-color 0.2s cubic-bezier(0.4, 0, 0.2, 1),
              box-shadow 0.2s cubic-bezier(0.4, 0, 0.2, 1),
              transform 0.2s cubic-bezier(0.4, 0, 0.2, 1),
              opacity 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  transform: translate3d(0, 0, 0);
}

button:hover,
.model-option:hover {
  transform: translate3d(0, -2px, 0);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15) !important;
  filter: brightness(1.15) saturate(1.2);
}

.stat-card:hover {
  transform: translate3d(0, -3px, 0);
  box-shadow: 0 6px 16px rgba(0, 0, 0, 0.2) !important;
  filter: brightness(1.2) saturate(1.3);
}

.history-item:hover {
  transform: translate3d(3px, 0, 0);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15) !important;
  filter: brightness(1.15) saturate(1.2);
}

button:active,
.model-option:active,
.stat-card:active,
.history-item:active {
  transform: translate3d(0, 0, 0);
  filter: brightness(0.9);
  transition-duration: 0.05s;
}
</style>
