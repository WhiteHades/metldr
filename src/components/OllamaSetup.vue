<script setup lang="ts">
import { ref } from 'vue'
import { Zap, Circle, RefreshCw, Check } from 'lucide-vue-next'
import { Button } from '@/components/ui'

const emit = defineEmits<{
  'retry': []
  'open-welcome': []
}>()

const copiedSetup = ref<boolean>(false)

const isWindows = typeof navigator !== 'undefined' && navigator.platform?.toLowerCase().includes('win')
const setupCommands = isWindows
  ? `ollama serve`
  : `OLLAMA_ORIGINS="chrome-extension://*" ollama serve`

function copySetupCommands() {
  navigator.clipboard.writeText(setupCommands).then(() => {
    copiedSetup.value = true
    setTimeout(() => { copiedSetup.value = false }, 2000)
  })
}
</script>

<template>
  <div class="p-4 h-full overflow-y-auto">
    <div class="rounded-xl bg-linear-to-br from-primary/10 via-card to-secondary/5 p-5 border border-primary/20 shadow-lg">
      <!-- header -->
      <div class="flex items-center gap-3 mb-5">
        <div class="w-10 h-10 rounded-xl bg-primary/20 flex items-center justify-center shrink-0 ring-2 ring-primary/10">
          <Zap :size="20" class="text-primary" />
        </div>
        <div>
          <h2 class="text-[15px] font-semibold text-foreground">start ollama</h2>
          <p class="text-(length:--font-text-secondary) text-foreground/60">run this command to connect</p>
        </div>
      </div>
      
      <!-- main command -->
      <div class="bg-background/80 backdrop-blur-sm rounded-lg p-4 border border-border mb-4">
        <div class="flex items-center justify-between mb-2">
          <span class="text-(length:--font-text-secondary) font-medium text-foreground/50 uppercase tracking-wide">
            {{ isWindows ? 'powershell' : 'terminal' }}
          </span>
          <button 
            @click="copySetupCommands" 
            class="text-(length:--font-text-secondary) text-primary hover:text-primary/80 transition-colors flex items-center gap-1"
          >
            <Check v-if="copiedSetup" :size="10" />
            <span>{{ copiedSetup ? 'copied!' : 'copy' }}</span>
          </button>
        </div>
        <pre class="font-mono text-(length:--font-text-secondary) text-foreground whitespace-pre-wrap leading-relaxed">{{ setupCommands }}</pre>
      </div>
      
      <!-- auto-retry indicator -->
      <div class="flex items-center gap-2 mb-4 px-1">
        <div class="relative flex items-center justify-center">
          <Circle :size="8" class="text-primary animate-pulse" fill="currentColor" />
          <div class="absolute inset-0 rounded-full bg-primary/30 animate-ping"></div>
        </div>
        <span class="text-(length:--font-text-secondary) text-foreground/60">auto-detecting ollama...</span>
      </div>
      
      <!-- retry button -->
      <Button 
        @click="emit('retry')" 
        variant="outline"
        class="w-full mb-4 h-9 text-(length:--font-text-secondary) border-primary/30 hover:bg-primary/10 hover:border-primary/50"
      >
        <RefreshCw :size="13" class="mr-2" />
        check connection now
      </Button>
      
      <!-- fallback section -->
      <div class="pt-4 border-t border-border/50">
        <p class="text-(length:--font-text-secondary) text-foreground/50 text-center">
          don't have ollama installed?
          <button 
            @click="emit('open-welcome')" 
            class="text-primary hover:text-primary/80 hover:underline transition-colors font-medium ml-1"
          >
            view setup guide →
          </button>
        </p>
      </div>
    </div>
  </div>
</template>
