<script setup>
import { ref, onMounted, onUnmounted, computed, nextTick } from 'vue';
import { useThemeStore } from '../stores/theme.js';
import { gsap } from 'gsap';
import { Mail, Zap, Database, Loader2, ChevronRight, Sparkles } from 'lucide-vue-next';

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
    const request = indexedDB.open('metldr_summaries', 2);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onupgradeneeded = (event) => {
      const db = event.target.result;
      if (!db.objectStoreNames.contains('summaries')) {
        db.createObjectStore('summaries', { keyPath: 'emailId' });
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

// gsap
async function animateStats() {
  await nextTick();
  
  const cards = document.querySelectorAll('.stat-card');
  const values = document.querySelectorAll('.stat-value');
  const bars = document.querySelectorAll('.stat-bar');
  
  // only animate if elements exist
  if (cards.length === 0) return;
  
  gsap.from(cards, {
    scale: 0.9,
    opacity: 0,
    duration: 0.4,
    stagger: 0.1,
    ease: 'back.out(1.4)'
  });
  
  if (values.length > 0) {
    values.forEach((el, i) => {
      const target = parseFloat(el.textContent);
      gsap.from(el, {
        textContent: 0,
        duration: 1,
        delay: i * 0.15,
        ease: 'power2.out',
        snap: { textContent: target < 10 ? 0.1 : 1 },
        onUpdate: function() {
          if (target < 10) {
            el.textContent = parseFloat(this.targets()[0].textContent).toFixed(1);
          } else {
            el.textContent = Math.round(this.targets()[0].textContent);
          }
        }
      });
    });
  }
  
  if (bars.length > 0) {
    gsap.from(bars, {
      scaleX: 0,
      duration: 1.2,
      stagger: 0.15,
      ease: 'expo.out',
      transformOrigin: 'left center'
    });
    
    gsap.to(bars, {
      boxShadow: `0 0 20px currentColor, 0 0 30px currentColor`,
      duration: 1.5,
      repeat: -1,
      yoyo: true,
      ease: 'sine.inOut'
    });
  }
}

onMounted(async () => {
  history.value = await loadHistory();
  animateStats();
  
  const refreshInterval = setInterval(async () => {
    history.value = await loadHistory();
    animateStats();
  }, 30000);
  
  onUnmounted(() => {
    clearInterval(refreshInterval);
  });
});

defineExpose({
  refresh: async () => {
    history.value = await loadHistory();
    animateStats();
  },
  stats
});
</script>

<template>
  <div class="space-y-4">
    <!-- stats -->
    <div class="grid grid-cols-3 gap-2.5">
      <!-- today count -->
      <div 
        class="stat-card glass-strong rounded-xl px-4 py-4 border overflow-hidden transition-all duration-200 hover:scale-[1.02]"
        title="emails summarised today"
        :style="{ 
          borderColor: theme.primary,
          boxShadow: `0 4px 16px color-mix(in oklch, ${theme.primary} 20%, transparent)`
        }"
      >
        <div class="flex flex-col items-center text-center relative z-10">
          <Mail :size="16" :stroke-width="2.5" class="mb-2" :style="{ color: theme.primary }" />
          <div class="stat-value text-2xl font-bold font-mono tabular-nums" :style="{ color: theme.primary }">
            {{ stats.todayCount }}
          </div>
          <div class="text-[10px] font-semibold mt-1 uppercase tracking-wider" :style="{ color: theme.textMuted }">
            today
          </div>
        </div>
        <div 
          class="stat-bar absolute bottom-0 left-0 right-0 h-0.5"
          :style="{ 
            background: `linear-gradient(90deg, transparent, ${theme.primary}, transparent)`
          }"
        ></div>
      </div>

      <!-- avg speed -->
      <div 
        class="stat-card glass-strong rounded-xl px-4 py-4 border overflow-hidden transition-all duration-200 hover:scale-[1.02]"
        title="average processing time"
        :style="{ 
          borderColor: theme.secondary,
          boxShadow: `0 4px 16px color-mix(in oklch, ${theme.secondary} 20%, transparent)`
        }"
      >
        <div class="flex flex-col items-center text-center relative z-10">
          <Zap :size="16" :stroke-width="2.5" class="mb-2" :style="{ color: theme.secondary }" />
          <div class="stat-value text-2xl font-bold font-mono tabular-nums" :style="{ color: theme.secondary }">
            {{ (stats.avgTime / 1000).toFixed(1) }}s
          </div>
          <div class="text-[10px] font-semibold mt-1 uppercase tracking-wider" :style="{ color: theme.textMuted }">
            speed
          </div>
        </div>
        <div 
          class="stat-bar absolute bottom-0 left-0 right-0 h-0.5"
          :style="{ 
            background: `linear-gradient(90deg, transparent, ${theme.secondary}, transparent)`
          }"
        ></div>
      </div>

      <!-- cache -->
      <div 
        class="stat-card glass-strong rounded-xl px-4 py-4 border overflow-hidden transition-all duration-200 hover:scale-[1.02]"
        title="cache efficiency"
        :style="{ 
          borderColor: theme.accent,
          boxShadow: `0 4px 16px color-mix(in oklch, ${theme.accent} 20%, transparent)`
        }"
      >
        <div class="flex flex-col items-center text-center relative z-10">
          <Database :size="16" :stroke-width="2.5" class="mb-2" :style="{ color: theme.accent }" />
          <div class="stat-value text-2xl font-bold font-mono tabular-nums" :style="{ color: theme.accent }">
            {{ stats.cacheHitRate.toFixed(0) }}%
          </div>
          <div class="text-[10px] font-semibold mt-1 uppercase tracking-wider" :style="{ color: theme.textMuted }">
            cache
          </div>
        </div>
        <div 
          class="stat-bar absolute bottom-0 left-0 right-0 h-0.5"
          :style="{ 
            background: `linear-gradient(90deg, transparent, ${theme.accent}, transparent)`
          }"
        ></div>
      </div>
    </div>

    <div>
      <div class="flex items-center gap-2 mb-3">
        <Sparkles :size="12" :stroke-width="2.5" :style="{ color: theme.textMuted }" />
        <h3 
          class="text-[11px] font-semibold uppercase tracking-wider"
          :style="{ color: theme.textMuted }"
        >
          recent summaries
        </h3>
      </div>
      
      <div v-if="loading" class="flex items-center justify-center py-12">
        <Loader2 
          class="w-8 h-8 animate-spin" 
          :stroke-width="2"
          :style="{ color: theme.primary }"
        />
      </div>

      <div 
        v-else-if="history.length === 0" 
        class="text-center py-8 px-5 rounded-xl glass border"
        :style="{ borderColor: theme.borderSubtle }"
      >
        <Mail :size="32" :stroke-width="2" class="mx-auto mb-3 opacity-40" :style="{ color: theme.textMuted }" />
        <p class="text-[14px] mb-1.5 font-semibold" :style="{ color: theme.text }">
          no summaries yet
        </p>
        <p class="text-[12px] leading-relaxed" :style="{ color: theme.textMuted }">
          visit gmail to generate your first summary
        </p>
      </div>

      <div v-else class="space-y-2">
        <div 
          v-for="item in history" 
          :key="item.emailId"
          class="history-item group flex items-center gap-3 px-4 py-3 rounded-xl cursor-pointer transition-all duration-200 hover:scale-[1.01] active:scale-[0.99] glass border"
          :style="{ 
            borderColor: theme.borderSubtle
          }"
          @click="openThread(item.emailId)"
        >
          <div class="flex-1 min-w-0">
            <p class="text-[13px] line-clamp-2 mb-1.5 font-medium leading-relaxed" :style="{ color: theme.text }">
              {{ item.summary?.summary || 'no summary available' }}
            </p>
            <div class="flex items-center gap-2.5 text-[11px]">
              <span class="font-medium" :style="{ color: theme.textMuted }">
                {{ formatTimestamp(item.timestamp) }}
              </span>
              <span 
                v-if="item.summary?.cached" 
                class="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[10px] font-semibold glass border"
                :style="{ 
                  color: theme.accent,
                  borderColor: theme.accent
                }"
              >
                <Database :size="10" :stroke-width="2.5" />
                cached
              </span>
            </div>
          </div>
          <ChevronRight 
            :size="16" 
            :stroke-width="2.5"
            class="arrow-icon shrink-0 transition-all duration-200 group-hover:translate-x-1"
            :style="{ color: theme.primary }"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stat-card {
  position: relative;
}

.stat-bar {
  position: absolute;
}
</style>
