<script setup>
import { ref, onMounted, onUnmounted, computed } from 'vue';
import { FileText, Clock, Zap, Loader2, ChevronRight, TrendingUp, Trash2, Database } from 'lucide-vue-next';

const props = defineProps({
  limit: {
    type: Number,
    default: 10
  }
});

const history = ref([]);
const loading = ref(true);
let messageListener = null;

async function openDB() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('metldr_cache', 4);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onupgradeneeded = (event) => {
      const db = event.target.result;
      if (!db.objectStoreNames.contains('summaries')) {
        db.createObjectStore('summaries', { keyPath: 'emailId' });
      }
      if (!db.objectStoreNames.contains('page_cache')) {
        db.createObjectStore('page_cache', { keyPath: 'url' });
      }
      if (!db.objectStoreNames.contains('reply_suggestions')) {
        db.createObjectStore('reply_suggestions', { keyPath: 'emailId' });
      }
    };
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
  const total = history.value.length;  
  const timeSavedMins = total * 2.5;
  const totalTime = history.value.reduce((sum, item) => {
    return sum + (item.summary?.time_ms || 2000);
  }, 0);
  const avgTime = total > 0 ? totalTime / total : 0;

  const weekAgo = Date.now() - (7 * 24 * 60 * 60 * 1000);
  const thisWeek = history.value.filter(item => item.timestamp > weekAgo).length;

  return {
    total,
    timeSaved: timeSavedMins,
    avgSpeed: avgTime,
    thisWeek
  };
});

async function clearHistory() {
  if (!confirm('clear all summary history?')) return;
  try {
    const db = await openDB();
    const tx = db.transaction(['summaries'], 'readwrite');
    tx.objectStore('summaries').clear();
    history.value = [];
  } catch (err) {
    console.error('metldr: failed to clear history:', err);
  }
}

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
  
  messageListener = (message) => {
    if (message.type === 'SUMMARY_ADDED') {
      loadHistory().then(data => {
        history.value = data;
      });
    }
  };
  chrome.runtime.onMessage.addListener(messageListener);
});

onUnmounted(() => {
  if (messageListener) {
    chrome.runtime.onMessage.removeListener(messageListener);
  }
});

defineExpose({
  refresh: async () => {
    history.value = await loadHistory();
  },
  stats
});
</script>

<template>
  <div class="space-y-3">
    <!-- stats row -->
    <div class="flex items-center gap-2 px-1">
      <div 
        class="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-primary/10 border border-primary/20 cursor-help"
        title="total emails summarized"
      >
        <FileText :size="12" :stroke-width="2" class="text-primary" />
        <span class="text-sm font-semibold tabular-nums text-primary">{{ stats.total }}</span>
      </div>
      <div 
        class="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-secondary/10 border border-secondary/20 cursor-help"
        title="estimated reading time saved"
      >
        <Clock :size="12" :stroke-width="2" class="text-secondary" />
        <span class="text-sm font-semibold tabular-nums text-secondary">{{ stats.timeSaved >= 60 ? Math.round(stats.timeSaved / 60) + 'h' : Math.round(stats.timeSaved) + 'm' }}</span>
      </div>
      <div 
        class="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-accent/10 border border-accent/20 cursor-help"
        title="summaries this week"
      >
        <TrendingUp :size="12" :stroke-width="2" class="text-accent" />
        <span class="text-sm font-semibold tabular-nums text-accent">{{ stats.thisWeek }}</span>
        <span class="text-[10px] text-accent/70">/wk</span>
      </div>
    </div>

    <div>
      <div class="flex items-center justify-between mb-3 px-1">
        <div class="flex items-center gap-2">
          <Zap :size="12" :stroke-width="2.5" class="text-base-content/40" />
          <h3 class="text-[11px] font-semibold uppercase tracking-wider text-base-content/40">
            Recent
          </h3>
        </div>
        <button 
          v-if="history.length > 0"
          @click="clearHistory"
          class="flex items-center gap-1 text-[10px] text-base-content/30 hover:text-error/60 transition-colors"
        >
          <Trash2 :size="10" />
          clear
        </button>
      </div>
      
      <div v-if="loading" class="flex items-center justify-center py-12">
        <div class="w-10 h-10 rounded-full bg-base-200 flex items-center justify-center">
          <Loader2 class="w-5 h-5 animate-spin text-primary" :stroke-width="2" />
        </div>
      </div>

      <div 
        v-else-if="history.length === 0" 
        class="flex flex-col items-center justify-center py-10 px-5 rounded-xl bg-base-200/50 border border-base-300/20"
      >
        <div class="w-14 h-14 rounded-2xl bg-base-300/50 flex items-center justify-center mb-3">
          <FileText :size="24" :stroke-width="1.5" class="text-base-content/25" />
        </div>
        <p class="text-sm font-medium text-base-content/70 mb-1">
          No summaries yet
        </p>
        <p class="text-xs text-base-content/40 text-center max-w-[200px]">
          Browse articles to start building your reading history
        </p>
      </div>

      <div v-else class="space-y-2">
        <div 
          v-for="item in history" 
          :key="item.emailId"
          class="group flex items-center gap-3 px-3.5 py-3 rounded-xl cursor-pointer transition-all duration-200 hover:scale-[1.01] active:scale-[0.99] bg-base-200/50 border border-base-300/20 hover:bg-base-200/80 hover:border-base-300/40"
          @click="openThread(item.emailId)"
        >
          <div class="flex-1 min-w-0">
            <p class="text-sm line-clamp-2 mb-1.5 font-medium leading-relaxed text-base-content/85">
              {{ item.summary?.summary || 'No summary available' }}
            </p>
            <div class="flex items-center gap-2 text-[11px]">
              <span class="text-base-content/50">
                {{ formatTimestamp(item.timestamp) }}
              </span>
              <span 
                v-if="item.summary?.cached" 
                class="flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium bg-accent/10 text-accent"
              >
                <Database :size="9" :stroke-width="2.5" />
                cached
              </span>
            </div>
          </div>
          <ChevronRight 
            :size="16" 
            :stroke-width="2.5"
            class="shrink-0 text-primary/50 transition-all duration-200 group-hover:translate-x-0.5 group-hover:text-primary"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
</style>
