<script setup lang="ts">
import { ref } from 'vue'
import { useThemeStore } from '@/stores/theme'
import { SUPPORTED_LANGUAGES } from '@/utils/storage'
import { 
  FileText, BookOpen, Database, Trash2, HelpCircle, X, Cpu, Zap, Palette, Loader2, TestTube2
} from 'lucide-vue-next'
import { Toggle, ScrollArea, Checkbox, Textarea, Input, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'
import AIStatusCards from './AIStatusCards.vue'
import DonationCard from './DonationCard.vue'
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
  fontSize: 'small' | 'medium' | 'large'
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
  'update:fontSize': [size: 'small' | 'medium' | 'large']
}>()

const themeStore = useThemeStore()

const seeding = ref(false)
const seedResult = ref<string | null>(null)
const isDev = import.meta.env.DEV

async function seedTestData() {
  seeding.value = true
  seedResult.value = null
  try {
    const response = await chrome.runtime.sendMessage({ type: 'DEV_SEED_DATA', days: 60 })
    if (response.success) {
      seedResult.value = `seeded: ${response.emailSessions} emails, ${response.tabSessions} pages, ${response.dailyStats} days`
    } else {
      seedResult.value = `error: ${response.error}`
    }
  } catch (err) {
    seedResult.value = `error: ${(err as Error).message}`
  } finally {
    seeding.value = false
  }
}

async function clearTestData() {
  seeding.value = true
  seedResult.value = null
  try {
    await chrome.runtime.sendMessage({ type: 'DEV_CLEAR_DATA' })
    seedResult.value = 'all data cleared'
  } catch (err) {
    seedResult.value = `error: ${(err as Error).message}`
  } finally {
    seeding.value = false
  }
}
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

      <!-- word lookup toggle -->
      <div class="rounded-xl bg-card p-4 border border-border">
        <div class="flex items-center justify-between">
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger as-child>
                <div class="flex items-center gap-2.5 cursor-help">
                  <div class="flex items-center justify-center w-6 h-6 rounded-md bg-secondary/25">
                    <BookOpen :size="12" class="text-secondary" />
                  </div>
                  <span class="text-(length:--font-text-secondary) font-medium text-foreground tracking-wide">word lookup</span>
                </div>
              </TooltipTrigger>
              <TooltipContent>show definitions on text selection</TooltipContent>
            </Tooltip>
          </TooltipProvider>
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
          <span class="text-(length:--font-text-secondary) font-medium text-foreground tracking-wide">dictionaries</span>
        </div>
        <p class="text-(length:--font-text-secondary) text-foreground/50 mb-3">select languages for word lookup. undownloaded languages will download when enabled.</p>
        
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
              <span class="text-(length:--font-text-secondary) text-foreground/80 flex-1">{{ lang.name }}</span>
              
              <!-- status badges -->
              <div v-if="downloadProgress[lang.code]" class="flex items-center gap-1">
                <Loader2 :size="10" class="animate-spin text-amber-400" />
                <span class="text-(length:--font-text-secondary) text-amber-400">
                  {{ Number(downloadProgress[lang.code].progress || 0).toFixed(0) }}%
                </span>
              </div>
              <span 
                v-else-if="downloadedLanguages.includes(lang.code)"
                class="text-(length:--font-text-secondary) px-1.5 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400/80"
              >
                ready
              </span>
              
              <!-- delete button -->
              <TooltipProvider v-if="downloadedLanguages.includes(lang.code)">
                <Tooltip>
                  <TooltipTrigger as-child>
                    <button
                      @click.stop="emit('delete-language', lang.code)"
                      class="p-1 rounded hover:bg-muted text-foreground/30 hover:text-destructive transition-colors"
                    >
                      <X :size="10" />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent>delete dictionary</TooltipContent>
                </Tooltip>
              </TooltipProvider>
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
          <span class="text-(length:--font-text-secondary) font-medium text-foreground tracking-wide">theme</span>
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
            <span class="text-(length:--font-text-secondary) text-foreground/70">{{ themeData.name }}</span>
          </button>
        </div>
      </div>

      <!-- font size -->
      <div class="rounded-xl bg-card p-4 border border-border">
        <div class="flex items-center gap-2.5 mb-3">
          <div class="flex items-center justify-center w-6 h-6 rounded-md bg-secondary/20">
            <span class="text-secondary text-[10px] font-bold">Aa</span>
          </div>
          <span class="text-(length:--font-text-secondary) font-medium text-foreground tracking-wide">text size</span>
        </div>
        <div class="grid grid-cols-3 gap-2">
          <button
            v-for="size in ['small', 'medium', 'large'] as const"
            :key="size"
            @click="emit('update:fontSize', size)"
            class="flex items-center justify-center py-2 rounded-lg transition-all capitalize"
            :class="[
              fontSize === size 
                ? 'bg-muted ring-1 ring-border text-foreground' 
                : 'hover:bg-muted/50 text-foreground/70'
            ]"
          >
            <span :class="[
              size === 'small' ? 'text-[11px]' : size === 'medium' ? 'text-[13px]' : 'text-[15px]'
            ]">{{ size }}</span>
          </button>
        </div>
      </div>

      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger as-child>
            <div class="rounded-xl bg-card p-4 border border-border cursor-help">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2.5">
                  <div class="flex items-center justify-center w-6 h-6 rounded-md bg-destructive/20">
                    <Trash2 :size="12" class="text-destructive" />
                  </div>
                  <span class="text-(length:--font-text-secondary) font-medium text-foreground tracking-wide">cache</span>
                </div>
                <button @click.stop="emit('clear-cache')" class="px-2 py-1 rounded text-(length:--font-text-secondary) text-destructive/70 hover:text-destructive hover:bg-destructive/10 transition-colors">
                  clear
                </button>
              </div>
            </div>
          </TooltipTrigger>
          <TooltipContent>clear cached summaries</TooltipContent>
        </Tooltip>
      </TooltipProvider>

      <!-- help -->
      <div class="rounded-xl bg-card p-4 border border-border">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2.5">
            <div class="flex items-center justify-center w-6 h-6 rounded-md bg-info/20">
              <HelpCircle :size="12" class="text-info" />
            </div>
            <span class="text-(length:--font-text-secondary) font-medium text-foreground tracking-wide">help</span>
          </div>
          <button @click="emit('open-welcome')" class="px-2 py-1 rounded text-(length:--font-text-secondary) text-foreground/70 hover:text-foreground hover:bg-muted transition-colors">
            open guide
          </button>
        </div>
      </div>

      <!-- auto-summarise (WIP) -->
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger as-child>
            <div class="rounded-xl bg-card/50 p-4 border border-border/50 opacity-50 cursor-not-allowed select-none">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2.5">
                  <div class="flex items-center justify-center w-6 h-6 rounded-md bg-primary/15">
                    <FileText :size="12" class="text-foreground/40" />
                  </div>
                  <div class="flex items-center gap-2">
                    <span class="text-(length:--font-text-secondary) font-medium text-foreground/50 tracking-wide">auto-summarise</span>
                    <span class="text-[9px] px-1.5 py-0.5 rounded-full bg-amber-500/15 text-amber-500/80 font-medium uppercase tracking-wider">wip</span>
                  </div>
                </div>
                <Toggle 
                  :model-value="false"
                  disabled
                  class="pointer-events-none opacity-40"
                />
              </div>
              <p class="text-(length:--font-text-secondary) text-foreground/40 mt-2">automatically summarise pages matching your allowlist when visited. configure domains and minimum word count thresholds.</p>
            </div>
          </TooltipTrigger>
          <TooltipContent class="max-w-[240px]">
            <p class="font-medium mb-1">coming soon</p>
            <p class="text-muted-foreground">this feature will auto-summarise articles, emails, and PDFs when you visit pages matching your custom allowlist patterns.</p>
          </TooltipContent>
        </Tooltip>
      </TooltipProvider>

      <!-- support -->
      <DonationCard />

      <!-- dev tools (for testing stats visualization) -->
      <div v-if="isDev" class="rounded-xl bg-card p-4 border border-dashed border-border/50">
        <div class="flex items-center gap-2.5 mb-3">
          <div class="flex items-center justify-center w-6 h-6 rounded-md bg-amber-500/20">
            <TestTube2 :size="12" class="text-amber-500" />
          </div>
          <span class="text-(length:--font-text-secondary) font-medium text-foreground/70 tracking-wide">dev tools</span>
          <span class="text-[9px] px-1.5 py-0.5 rounded-full bg-amber-500/15 text-amber-500/80 font-medium uppercase tracking-wider">test</span>
        </div>
        <p class="text-(length:--font-text-secondary) text-foreground/50 mb-3">populate database with realistic test data to preview long-term usage stats.</p>
        <div class="flex gap-2">
          <button 
            @click="seedTestData" 
            :disabled="seeding"
            class="flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-lg text-(length:--font-text-secondary) font-medium bg-amber-500/15 text-amber-500 hover:bg-amber-500/25 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Loader2 v-if="seeding" :size="12" class="animate-spin" />
            <span>seed 60 days</span>
          </button>
          <button 
            @click="clearTestData"
            :disabled="seeding"
            class="px-3 py-2 rounded-lg text-(length:--font-text-secondary) font-medium bg-destructive/15 text-destructive hover:bg-destructive/25 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            clear all
          </button>
        </div>
        <p v-if="seedResult" class="text-(length:--font-text-secondary) text-foreground/60 mt-2 italic">{{ seedResult }}</p>
      </div>
    </div>
  </ScrollArea>
</template>
