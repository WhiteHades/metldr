<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useThemeStore } from '../stores/theme.js';

const props = defineProps({
  word: String,
  position: Object,
  type: String // definition or translation
});

const emit = defineEmits(['close']);

const themeStore = useThemeStore();
const theme = computed(() => themeStore.colors);

const loading = ref(true);
const result = ref(null);
const error = ref(null);

// position popup near selection
const popupStyle = computed(() => {
  if (!props.position) return {};
  
  return {
    position: 'fixed',
    top: `${props.position.top}px`,
    left: `${props.position.left}px`,
    zIndex: '999999'
  };
});

async function fetchDefinition() {
  loading.value = true;
  error.value = null;
  
  try {
    // send message to background for ollama lookup
    const response = await chrome.runtime.sendMessage({
      type: 'WORD_LOOKUP',
      word: props.word,
      lookupType: props.type
    });
    
    if (response.error) {
      error.value = response.error;
    } else {
      result.value = response.result;
    }
  } catch (e) {
    error.value = 'lookup failed';
    console.error('metldr: word lookup error:', e);
  } finally {
    loading.value = false;
  }
}

function handleClickOutside(e) {
  const popup = document.querySelector('.metldr-inline-popup');
  if (popup && !popup.contains(e.target)) {
    emit('close');
  }
}

onMounted(() => {
  fetchDefinition();
  document.addEventListener('click', handleClickOutside);
});

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside);
});
</script>

<template>
  <div 
    class="metldr-inline-popup animate-in fade-in slide-in-from-top-2 duration-200"
    :style="{
      ...popupStyle,
      background: theme.bgSecondary,
      borderWidth: '1px',
      borderColor: theme.border,
      boxShadow: `0 4px 16px ${theme.shadow}`,
      borderRadius: '8px',
      maxWidth: '320px',
      minWidth: '240px'
    }"
  >
    <!-- header -->
    <div 
      class="flex items-center justify-between px-3 py-2 transition-colors duration-200"
      :style="{ 
        borderBottomWidth: '1px', 
        borderColor: theme.border,
        background: theme.bg
      }"
    >
      <span class="text-xs font-mono font-semibold" :style="{ color: theme.primary }">
        {{ word }}
      </span>
      <button 
        @click="emit('close')"
        class="text-xs transition-all duration-150 hover:scale-110"
        :style="{ color: theme.textMuted }"
      >
        ✕
      </button>
    </div>

    <!-- content -->
    <div class="px-3 py-3">
      <!-- loading state -->
      <div v-if="loading" class="flex items-center gap-2">
        <div 
          class="w-3 h-3 border border-t-transparent rounded-full animate-spin"
          :style="{ borderColor: theme.primary }"
        ></div>
        <span class="text-xs" :style="{ color: theme.textMuted }">looking up...</span>
      </div>

      <!-- error state -->
      <div v-else-if="error" class="text-xs" :style="{ color: theme.secondary }">
        {{ error }}
      </div>

      <!-- result -->
      <div v-else-if="result" class="space-y-2">
        <!-- definition -->
        <div v-if="type === 'definition'" class="space-y-1">
          <p class="text-xs leading-relaxed" :style="{ color: theme.text }">
            {{ result.definition }}
          </p>
          <div v-if="result.partOfSpeech" class="text-[10px] font-mono" :style="{ color: theme.textMuted }">
            {{ result.partOfSpeech }}
          </div>
        </div>

        <!-- translation -->
        <div v-else-if="type === 'translation'" class="space-y-1">
          <div class="text-xs" :style="{ color: theme.textMuted }">
            {{ result.sourceLang }} → {{ result.targetLang }}
          </div>
          <p class="text-sm font-medium" :style="{ color: theme.text }">
            {{ result.translation }}
          </p>
          <p v-if="result.context" class="text-[10px] leading-relaxed" :style="{ color: theme.textMuted }">
            {{ result.context }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.metldr-inline-popup {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  backdrop-filter: blur(8px);
}

.animate-in {
  animation-duration: 200ms;
  animation-fill-mode: both;
}

.fade-in {
  animation-name: fadeIn;
}

.slide-in-from-top-2 {
  animation-name: slideInFromTop;
}

@keyframes fadeIn {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

@keyframes slideInFromTop {
  from {
    transform: translateY(-8px);
  }
  to {
    transform: translateY(0);
  }
}
</style>
