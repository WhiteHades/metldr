<script setup>
import { ref, onMounted, onUnmounted, computed } from 'vue';
import { useThemeStore } from '../stores/theme.js';

const themeStore = useThemeStore();
const theme = computed(() => themeStore.colors);

const props = defineProps({
  limit: {
    type: Number,
    default: 10
  }
});

const history = ref([]);
const loading = ref(true);

async function openDB() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('metldr_summaries', 1);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function loadHistory() {
  try {
    loading.value = true;
    const db = await openDB();
    const transaction = db.transaction(['summaries'], 'readonly');
    const store = transaction.objectStore('summaries');
    const request = store.getAll();

    return new Promise((resolve) => {
      request.onsuccess = () => {
        const allSummaries = request.result || [];
        const sorted = allSummaries
          .sort((a, b) => b.timestamp - a.timestamp)
          .slice(0, props.limit);
        resolve(sorted);
      };
      request.onerror = () => resolve([]);
    });
  } catch (error) {
    console.error('metldr: failed to load history:', error);
    return [];
  } finally {
    loading.value = false;
  }
}

const stats = computed(() => {
  const today = new Date().setHours(0, 0, 0, 0);
  const todayItems = history.value.filter(item => {
    const itemDate = new Date(item.timestamp).setHours(0, 0, 0, 0);
    return itemDate === today;
  });

  const totalTime = todayItems.reduce((sum, item) => {
    return sum + (item.summary?.time_ms || 0);
  }, 0);

  const avgTime = todayItems.length > 0 ? totalTime / todayItems.length : 0;

  const cacheHits = todayItems.filter(item => item.summary?.cached).length;

  return {
    todayCount: todayItems.length,
    avgTime: avgTime,
    cacheHitRate: todayItems.length > 0 ? (cacheHits / todayItems.length) * 100 : 0
  };
});

function formatTimestamp(ts) {
  const date = new Date(ts);
  const now = new Date();
  const diffMs = now - date;
  const diffMins = Math.floor(diffMs / 60000);
  
  if (diffMins < 1) return 'just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${diffHours}h ago`;
  
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 7) return `${diffDays}d ago`;
  
  return date.toLocaleDateString();
}

function openThread(emailId) {
  const url = `https://mail.google.com/mail/u/0/#inbox/${emailId}`;
  chrome.tabs.create({ url });
}

onMounted(async () => {
  history.value = await loadHistory();
  
  const refreshInterval = setInterval(async () => {
    history.value = await loadHistory();
  }, 30000);
  
  onUnmounted(() => {
    clearInterval(refreshInterval);
  });
});

defineExpose({
  refresh: loadHistory,
  stats
});
</script>

<template>
  <div class="space-y-3">
    <div class="grid grid-cols-3 gap-2">
      <div 
        class="stat-card px-3 py-2 rounded-lg text-center transition-all duration-200"
        title="number of emails summarized today"
        :style="{ 
          background: `${theme.primary}10`,
          borderWidth: '1px', 
          borderColor: `${theme.primary}30`,
          boxShadow: `0 1px 2px ${theme.shadow || 'rgba(0,0,0,0.05)'}`
        }"
      >
        <div class="text-xl font-bold transition-colors duration-200" :style="{ color: theme.primary }">
          {{ stats.todayCount }}
        </div>
        <div class="text-[10px] font-medium mt-0.5 transition-colors duration-200" :style="{ color: theme.textMuted }">
          today
        </div>
      </div>
      <div 
        class="stat-card px-3 py-2 rounded-lg text-center transition-all duration-200"
        title="average processing time per summary"
        :style="{ 
          background: `${theme.secondary}10`,
          borderWidth: '1px', 
          borderColor: `${theme.secondary}30`,
          boxShadow: `0 1px 2px ${theme.shadow || 'rgba(0,0,0,0.05)'}`
        }"
      >
        <div class="text-xl font-bold transition-colors duration-200" :style="{ color: theme.secondary }">
          {{ (stats.avgTime / 1000).toFixed(1) }}s
        </div>
        <div class="text-[10px] font-medium mt-0.5 transition-colors duration-200" :style="{ color: theme.textMuted }">
          speed
        </div>
      </div>
      <div 
        class="stat-card px-3 py-2 rounded-lg text-center transition-all duration-200"
        title="percentage of summaries loaded from cache"
        :style="{ 
          background: `${theme.accent}10`,
          borderWidth: '1px', 
          borderColor: `${theme.accent}30`,
          boxShadow: `0 1px 2px ${theme.shadow || 'rgba(0,0,0,0.05)'}`
        }"
      >
        <div class="text-xl font-bold transition-colors duration-200" :style="{ color: theme.accent }">
          {{ stats.cacheHitRate.toFixed(0) }}%
        </div>
        <div class="text-[10px] font-medium mt-0.5 transition-colors duration-200" :style="{ color: theme.textMuted }">
          cache
        </div>
      </div>
    </div>

    <div>
      <h3 
        class="text-[10px] font-semibold uppercase tracking-wider mb-2 transition-all duration-200"
        :style="{ color: theme.textMuted }"
      >
        recent summaries
      </h3>
      
      <div v-if="loading" class="flex items-center justify-center py-10">
        <div 
          class="w-6 h-6 border-2 border-t-transparent rounded-full animate-spin transition-all duration-200"
          :style="{ borderColor: theme.border, borderTopColor: theme.primary }"
        ></div>
      </div>

      <div 
        v-else-if="history.length === 0" 
        class="text-center py-6 px-4 rounded-lg transition-all duration-200"
        :style="{ backgroundColor: theme.bgSecondary, borderWidth: '1px', borderColor: theme.border }"
      >
        <p class="text-sm mb-1 font-medium transition-colors duration-200" :style="{ color: theme.text }">
          no summaries yet
        </p>
        <p class="text-xs leading-relaxed transition-colors duration-200" :style="{ color: theme.textMuted }">
          visit gmail to generate your first summary
        </p>
      </div>

      <div v-else class="space-y-1.5">
        <div 
          v-for="item in history" 
          :key="item.emailId"
          class="history-item group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-all duration-200"
          :style="{ 
            background: theme.bgSecondary,
            borderWidth: '1px', 
            borderColor: theme.border,
            boxShadow: `0 1px 2px ${theme.shadow || 'rgba(0,0,0,0.05)'}`
          }"
          @click="openThread(item.emailId)"
        >
          <div class="flex-1 min-w-0">
            <p class="text-sm line-clamp-2 mb-1 font-medium transition-colors duration-200" :style="{ color: theme.text }">
              {{ item.summary?.summary || 'no summary available' }}
            </p>
            <div class="flex items-center gap-2 text-[10px]">
              <span class="font-medium transition-colors duration-200" :style="{ color: theme.textMuted }">
                {{ formatTimestamp(item.timestamp) }}
              </span>
              <span 
                v-if="item.summary?.cached" 
                class="px-1.5 py-0.5 rounded text-[10px] transition-all duration-200"
                :style="{ 
                  backgroundColor: `${theme.accent}15`, 
                  color: theme.accent,
                  borderWidth: '1px',
                  borderColor: `${theme.accent}30`
                }"
              >
                cached
              </span>
            </div>
          </div>
          <span 
            class="arrow-icon text-sm transition-all duration-200"
            :style="{ color: theme.primary }"
          >
            →
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history-item,
.stat-card {
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

.history-item:hover {
  transform: translateX(2px);
}

.history-item:hover .arrow-icon {
  transform: translateX(4px);
}

.stat-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 3px 8px rgba(0, 0, 0, 0.12) !important;
}
</style>
