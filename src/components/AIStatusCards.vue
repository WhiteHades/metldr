<script setup lang="ts">
import { ChevronDown, Check, RefreshCw, Binary, HelpCircle } from 'lucide-vue-next'
import { ScrollArea } from '@/components/ui'
import type { DropdownPos } from '@/types'
import type { AIProviderPreference } from '@/composables/useSettings'
import { computed } from 'vue'

interface Props {
  chromeAIStatus: 'available' | 'downloadable' | 'downloading' | 'unavailable' | 'checking'
  ollamaStatus: 'checking' | 'ready' | 'not-found' | 'error'
  availableModels: string[]
  selectedModel: string
  showModelDropdown: boolean
  modelDropdownPos: DropdownPos
  preferredProvider?: AIProviderPreference
}

const props = defineProps<Props>()

const emit = defineEmits<{
  'toggle-dropdown': []
  'select-model': [model: string]
  'refresh-ollama': []
  'set-provider': [provider: AIProviderPreference]
  'open-welcome': []
}>()

const chromeSelected = computed(() => props.preferredProvider === 'chrome-ai')
const ollamaSelected = computed(() => props.preferredProvider === 'ollama')
</script>

<template>
  <!-- consolidated ai card: status + provider selection -->
  <div class="rounded-xl bg-card p-4 border border-border space-y-3">
    <div class="flex items-center gap-2.5">
      <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/25">
        <Binary :size="12" class="text-primary" />
      </div>
      <span class="text-[12px] font-medium text-foreground tracking-wide">ai provider</span>
    </div>

    <!-- chrome ai row -->
    <button 
      @click="emit('set-provider', 'chrome-ai')"
      class="w-full flex items-center justify-between py-2 px-3 rounded-lg transition-all"
      :class="chromeSelected ? 'bg-primary/10 ring-1 ring-primary/30' : 'bg-muted/30 hover:bg-muted/50'"
    >
      <div class="flex items-center gap-2">
        <span class="text-[11px] font-medium" :class="chromeSelected ? 'text-primary' : 'text-foreground/70'">gemini nano</span>
        <span class="text-[9px] px-1.5 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400/70">easy</span>
      </div>
      <span 
        class="text-[9px] px-1.5 py-0.5 rounded-full font-medium"
        :class="{
          'bg-emerald-500/20 text-emerald-400': chromeAIStatus === 'available',
          'bg-amber-500/20 text-amber-400': chromeAIStatus === 'downloading',
          'bg-red-500/20 text-red-400': chromeAIStatus === 'unavailable',
          'bg-muted text-muted-foreground': chromeAIStatus === 'checking'
        }"
      >
        {{ chromeAIStatus === 'available' ? 'ready' : chromeAIStatus }}
      </span>
    </button>

    <!-- ollama row -->
    <div>
      <button 
        @click="emit('set-provider', 'ollama')"
        :disabled="ollamaStatus !== 'ready'"
        class="w-full flex items-center justify-between py-2 px-3 rounded-lg transition-all"
        :class="[
          ollamaSelected ? 'bg-secondary/10 ring-1 ring-secondary/30' : 'bg-muted/30 hover:bg-muted/50',
          ollamaStatus !== 'ready' ? 'opacity-50 cursor-not-allowed' : ''
        ]"
      >
        <div class="flex items-center gap-2">
          <span class="text-[11px] font-medium" :class="ollamaSelected ? 'text-secondary' : 'text-foreground/70'">ollama</span>
          <span class="text-[9px] px-1.5 py-0.5 rounded-full bg-blue-500/15 text-blue-400/70">advanced</span>
        </div>
        <div class="flex items-center gap-1.5">
          <span 
            class="text-[9px] px-1.5 py-0.5 rounded-full font-medium"
            :class="{
              'bg-emerald-500/20 text-emerald-400': ollamaStatus === 'ready',
              'bg-red-500/20 text-red-400': ollamaStatus === 'not-found',
              'bg-muted text-muted-foreground': ollamaStatus === 'checking'
            }"
          >
            {{ ollamaStatus === 'ready' ? 'connected' : ollamaStatus === 'not-found' ? 'offline' : ollamaStatus }}
          </span>
          <RefreshCw 
            v-if="ollamaStatus !== 'ready'" 
            :size="10" 
            class="text-foreground/40"
            @click.stop="emit('refresh-ollama')"
          />
        </div>
      </button>
      
      <!-- model selector (only when ollama selected and ready) -->
      <div v-if="ollamaStatus === 'ready' && ollamaSelected" class="relative mt-2">
        <button 
          @click="emit('toggle-dropdown')"
          class="model-selector-btn w-full flex items-center justify-between px-3 py-2 bg-muted/50 hover:bg-muted rounded-lg border border-input transition-colors"
        >
          <span class="font-mono text-[10px] text-foreground truncate">{{ selectedModel }}</span>
          <ChevronDown 
            :size="12" 
            class="text-foreground/50 transition-transform shrink-0"
            :class="{ 'rotate-180': showModelDropdown }"
          />
        </button>

        <Transition
            enter-active-class="transition-all duration-150"
            enter-from-class="opacity-0 scale-95 translate-y-1"
            enter-to-class="opacity-100 scale-100 translate-y-0"
            leave-active-class="transition-all duration-100"
            leave-from-class="opacity-100 scale-100"
            leave-to-class="opacity-0 scale-95"
          >
            <div 
              v-if="showModelDropdown" 
              class="absolute top-full left-0 right-0 mt-1 rounded-lg bg-popover border border-border shadow-lg z-50 overflow-hidden"
            >
              <div class="max-h-48 overflow-y-auto py-1">
                <button 
                  v-for="model in availableModels" 
                  :key="model"
                  @click="emit('select-model', model)"
                  class="w-full flex items-center justify-between px-3 py-2 text-[11px] font-mono hover:bg-muted transition-colors"
                  :class="{ 'bg-muted': selectedModel === model }"
                >
                  <span class="text-foreground truncate">{{ model }}</span>
                  <Check v-if="selectedModel === model" :size="12" class="text-primary shrink-0 ml-2" />
                </button>
              </div>
            </div>
          </Transition>
        </div>
      </div>

    <!-- hint text -->
    <p class="text-[10px] text-foreground/50 pt-1">
      <template v-if="chromeSelected">built-in chrome ai. fast & private.</template>
      <template v-else>
        local ollama models. 
        <button @click="emit('open-welcome')" class="text-primary hover:underline">setup guide</button>
      </template>
    </p>
  </div>
</template>

<style scoped>
.model-dropdown {
  z-index: 9999 !important;
}
</style>
