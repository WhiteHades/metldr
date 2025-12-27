<script setup lang="ts">
import { useThemeStore } from '@/stores/theme'
import { SUPPORTED_LANGUAGES } from '@/utils/storage'
import { 
  FileText, BookOpen, Database, Trash2, HelpCircle, X, Cpu, Zap, Palette, Loader2
} from 'lucide-vue-next'
import { Toggle, ScrollArea, Checkbox, Textarea, Input } from '@/components/ui'
import AIStatusCards from './AIStatusCards.vue'
import type { DropdownPos, DownloadProgressItem } from '@/types'
import type { AIProviderPreference } from '@/composables/useSettings'

interface Props {
  chromeAIStatus: 'available' | 'downloadable' | 'downloading' | 'unavailable' | 'checking'
  ollamaStatus: 'checking' | 'ready' | 'not-found' | 'error'
  availableModels: string[]
  selectedModel: string
  showModelDropdown: boolean
  modelDropdownPos: DropdownPos
  summaryMode: 'manual' | 'auto'
  minAutoWords: number
  allowlistInput: string
  denylistInput: string
  wordPopupEnabled: boolean
  preferredProvider: AIProviderPreference
  downloadedLanguages: string[]
  selectedLanguages: string[]
  downloadProgress: Record<string, DownloadProgressItem>
}

const props = defineProps<Props>()

const emit = defineEmits<{
  'toggle-dropdown': []
  'select-model': [model: string]
  'update:summaryMode': [value: 'manual' | 'auto']
  'update:minAutoWords': [value: number]
  'update:allowlistInput': [value: string]
  'update:denylistInput': [value: string]
  'toggle-word-popup': [value: boolean]
  'set-provider': [provider: AIProviderPreference]
  'toggle-language': [langCode: string]
  'delete-language': [langCode: string]
  'refresh-ollama': []
  'clear-cache': []
  'open-welcome': []
}>()

const themeStore = useThemeStore()
</script>

<template>
  <ScrollArea class="h-full">
    <div class="p-3 space-y-3">
      
      <AIStatusCards 
        :chrome-a-i-status="chromeAIStatus"
        :ollama-status="ollamaStatus"
        :available-models="availableModels"
        :selected-model="selectedModel"
        :show-model-dropdown="showModelDropdown"
        :model-dropdown-pos="modelDropdownPos"
        :preferred-provider="preferredProvider"
        @toggle-dropdown="emit('toggle-dropdown')"
        @select-model="(m) => emit('select-model', m)"
        @refresh-ollama="emit('refresh-ollama')"
        @set-provider="(p) => emit('set-provider', p)"
        @open-welcome="emit('open-welcome')"
      />

      <!-- summary preferences -->
      <div class="rounded-xl bg-card p-4 border border-border space-y-3">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2.5">
            <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/25">
              <FileText :size="12" class="text-primary" />
            </div>
            <span class="text-[12px] font-medium text-foreground tracking-wide">auto-summarise</span>
          </div>
          <Toggle 
            :model-value="summaryMode === 'auto'"
            @update:model-value="(v: boolean) => emit('update:summaryMode', v ? 'auto' : 'manual')"
          />
        </div>
        <p class="text-[10px] text-foreground/60">when enabled, pages matching the allowlist will be summarised automatically.</p>

        <div v-if="summaryMode === 'auto'" class="space-y-3 pt-2 border-t border-border">
          <label class="text-[11px] text-foreground/80 flex items-center gap-2">
            <span class="shrink-0 text-[10px] w-20">min words</span>
            <Input 
              type="number" 
              min="0" 
              class="w-full h-7 text-[11px]" 
              :model-value="minAutoWords"
              @update:model-value="(v) => emit('update:minAutoWords', Number(v))"
            />
          </label>

          <div class="grid grid-cols-2 gap-3">
            <div>
              <div class="flex items-center justify-between mb-1">
                <span class="text-[11px] text-foreground/80">allowlist</span>
                <span class="text-[10px] text-foreground/50">one per line</span>
              </div>
              <Textarea 
                class="w-full h-28 text-[11px] resize-none"
                :model-value="allowlistInput"
                @update:model-value="(v) => emit('update:allowlistInput', String(v))"
                placeholder="example.com"
              />
            </div>
            <div>
              <div class="flex items-center justify-between mb-1">
                <span class="text-[11px] text-foreground/80">denylist</span>
                <span class="text-[10px] text-foreground/50">one per line</span>
              </div>
              <Textarea 
                class="w-full h-28 text-[11px] resize-none"
                :model-value="denylistInput"
                @update:model-value="(v) => emit('update:denylistInput', String(v))"
                placeholder="dashboard"
              />
            </div>
          </div>
        </div>
      </div>

      <!-- word lookup toggle -->
      <div class="rounded-xl bg-card p-4 border border-border">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2.5 group relative cursor-help" title="show definitions on text selection">
            <div class="flex items-center justify-center w-6 h-6 rounded-md bg-secondary/25">
              <BookOpen :size="12" class="text-secondary" />
            </div>
            <span class="text-[12px] font-medium text-foreground tracking-wide">word lookup</span>
          </div>
          <Toggle 
            :model-value="wordPopupEnabled"
            @update:model-value="(v: boolean) => emit('toggle-word-popup', v)"
          />
        </div>
      </div>

      <!-- dictionaries -->
      <div class="rounded-xl bg-card p-4 border border-border">
        <div class="flex items-center gap-2.5 mb-3">
          <div class="flex items-center justify-center w-6 h-6 rounded-md bg-accent/25">
            <Database :size="12" class="text-accent" />
          </div>
          <span class="text-[12px] font-medium text-foreground tracking-wide">dictionaries</span>
        </div>
        <p class="text-[10px] text-foreground/50 mb-3">select languages for word lookup. undownloaded languages will download when enabled.</p>
        
        <ScrollArea class="h-40 w-full">
          <div class="space-y-1 pr-4">
            <div 
              v-for="lang in SUPPORTED_LANGUAGES" 
              :key="lang.code" 
              @click="emit('toggle-language', lang.code)"
              class="flex items-center gap-2.5 py-1.5 cursor-pointer hover:bg-muted/50 rounded-lg px-2 -mx-2 transition-colors"
            >
              <Checkbox
                :checked="selectedLanguages.includes(lang.code)"
                class="h-4 w-4 pointer-events-none"
              />
              <span class="text-[11px] text-foreground/80 flex-1">{{ lang.name }}</span>
              
              <!-- status badges -->
              <div v-if="downloadProgress[lang.code]" class="flex items-center gap-1">
                <Loader2 :size="10" class="animate-spin text-amber-400" />
                <span class="text-[9px] text-amber-400">
                  {{ Number(downloadProgress[lang.code].progress || 0).toFixed(0) }}%
                </span>
              </div>
              <span 
                v-else-if="downloadedLanguages.includes(lang.code)"
                class="text-[9px] px-1.5 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400/80"
              >
                ready
              </span>
              
              <!-- delete button -->
              <button
                v-if="downloadedLanguages.includes(lang.code)"
                @click.stop="emit('delete-language', lang.code)"
                class="p-1 rounded hover:bg-muted text-foreground/30 hover:text-destructive transition-colors"
                title="delete dictionary"
              >
                <X :size="10" />
              </button>
            </div>
          </div>
        </ScrollArea>
      </div>

      <!-- theme -->
      <div class="rounded-xl bg-card p-4 border border-border">
        <div class="flex items-center gap-2.5 mb-3">
          <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/20">
            <Palette :size="12" class="text-primary" />
          </div>
          <span class="text-[12px] font-medium text-foreground tracking-wide">theme</span>
        </div>
        <div class="grid grid-cols-3 gap-2">
          <button
            v-for="(themeData, key) in themeStore.themes"
            :key="key"
            @click="themeStore.setTheme(key)"
            class="flex flex-col items-center gap-1.5 p-2 rounded-lg transition-all"
            :class="[
              themeStore.currentTheme === key 
                ? 'bg-muted ring-1 ring-border' 
                : 'hover:bg-muted/50'
            ]"
          >
            <div class="flex gap-0.5">
              <div class="w-3 h-3 rounded-full" :style="{ background: themeData.primary }"></div>
              <div class="w-3 h-3 rounded-full" :style="{ background: themeData.secondary }"></div>
              <div class="w-3 h-3 rounded-full" :style="{ background: themeData.accent }"></div>
            </div>
            <span class="text-[10px] text-foreground/70">{{ themeData.name }}</span>
          </button>
        </div>
      </div>

      <div 
        class="rounded-xl bg-card p-4 border border-border group cursor-help"
        title="clear cached summaries"
      >
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2.5">
            <div class="flex items-center justify-center w-6 h-6 rounded-md bg-destructive/20">
              <Trash2 :size="12" class="text-destructive" />
            </div>
            <span class="text-[12px] font-medium text-foreground tracking-wide">cache</span>
          </div>
          <button @click="emit('clear-cache')" class="px-2 py-1 rounded text-[11px] text-destructive/70 hover:text-destructive hover:bg-destructive/10 transition-colors">
            clear
          </button>
        </div>
      </div>

      <!-- help -->
      <div class="rounded-xl bg-card p-4 border border-border">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2.5">
            <div class="flex items-center justify-center w-6 h-6 rounded-md bg-info/20">
              <HelpCircle :size="12" class="text-info" />
            </div>
            <span class="text-[12px] font-medium text-foreground tracking-wide">help</span>
          </div>
          <button @click="emit('open-welcome')" class="px-2 py-1 rounded text-[11px] text-foreground/70 hover:text-foreground hover:bg-muted transition-colors">
            open guide
          </button>
        </div>
      </div>
    </div>
  </ScrollArea>
</template>
