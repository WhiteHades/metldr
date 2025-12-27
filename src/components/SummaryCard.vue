<script setup lang="ts">
import { computed } from 'vue'
import { formatTime, stripThinking } from '@/utils/text'
import { marked } from 'marked'
import { 
  ChevronDown, ChevronUp, RefreshCw, Zap, Sparkles, Server, AlertCircle, FileText, Loader2 
} from 'lucide-vue-next'
import { Button } from '@/components/ui'
import type { AppPageSummary, SummaryPromptData } from '@/types'

interface Props {
  pageSummary: AppPageSummary | null
  summaryLoading: boolean
  summaryError: string | null
  summaryPrompt: SummaryPromptData | null
  summaryCollapsed: boolean
  isEmailClient: boolean
  isViewingEmailThread: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{
  'update:collapsed': [value: boolean]
  'refresh': []
  'manual-summary': []
  'accept-prompt': []
  'decline-prompt': []
}>()

function renderMarkdown(text: string): string {
  if (!text) return ''
  const cleaned = stripThinking(text)
  return marked.parseInline(cleaned) as string
}

const hasContent = computed(() => props.pageSummary || props.summaryLoading || props.summaryError)
</script>

<template>
  <div class="shrink-0 p-2 pb-0">
    <!-- Prompt toast -->
    <div v-if="summaryPrompt" class="absolute right-2 bottom-2 max-w-xs rounded-lg border border-warning/50 bg-warning/10 px-3 py-2 text-[10px] shadow-lg z-10">
      <div class="flex items-start gap-2">
        <div class="flex-1 min-w-0">
          <div class="font-medium text-foreground truncate">summarise this page?</div>
          <div class="text-foreground/70 truncate">{{ summaryPrompt.reason }}</div>
        </div>
        <div class="flex gap-1 shrink-0">
          <button class="px-2 py-1 rounded text-[10px] text-foreground/70 hover:bg-muted transition-colors" @click="emit('decline-prompt')">no</button>
          <button class="px-2 py-1 rounded text-[10px] bg-warning/30 border border-warning/50 text-warning-foreground hover:bg-warning/40 transition-colors" @click="emit('accept-prompt')">yes</button>
        </div>
      </div>
    </div>
    
    <!-- Summary card with collapse toggle -->
    <div v-if="pageSummary" class="rounded-lg bg-card border border-border">
      <!-- header (always visible) -->
      <div 
        class="flex items-center gap-2 p-2.5 cursor-pointer hover:bg-primary/5 rounded-t-lg transition-colors"
        @click="emit('update:collapsed', !summaryCollapsed)"
      >
        <component 
          :is="summaryCollapsed ? ChevronDown : ChevronUp" 
          :size="11" 
          class="text-primary/60 shrink-0 transition-transform"
        />
        <div class="flex-1 min-w-0">
          <h3 class="text-[12px] font-medium text-foreground truncate leading-tight">
            {{ pageSummary.title || 'untitled' }}
          </h3>
          <p v-if="summaryCollapsed" class="text-[10px] text-foreground/60 truncate">
            {{ pageSummary.bullets.length }} key points · {{ pageSummary.readTime || 'n/a' }} read
          </p>
        </div>
        <button 
          @click.stop="emit('refresh')" 
          class="flex items-center justify-center w-7 h-7 rounded-md hover:bg-primary/10 shrink-0 transition-colors"
          :disabled="summaryLoading"
          title="regenerate summary"
        >
          <RefreshCw :size="12" :class="[summaryLoading ? 'animate-spin' : '', 'text-primary']" />
        </button>
      </div>
      
      <!-- Collapsible content -->
      <Transition name="summary">
        <div v-if="!summaryCollapsed" class="summary-content overflow-hidden">
          <div class="px-3 pb-3 pt-1 border-t border-border">
            <!-- metadata -->
            <p v-if="pageSummary.publication || pageSummary.author" class="text-[10px] text-foreground/60 mb-2">
              {{ pageSummary.publication }}{{ pageSummary.author ? ` · ${pageSummary.author}` : '' }}
            </p>
            
            <!-- bullets -->
            <ul class="space-y-1.5">
              <li v-for="(bullet, i) in pageSummary.bullets" :key="i" class="flex gap-2 text-[11px] leading-relaxed">
                <span class="text-primary shrink-0 mt-0.5">•</span>
                <span class="text-foreground markdown-content" v-html="renderMarkdown(bullet)"></span>
              </li>
            </ul>
            
            <div class="flex items-center justify-between pt-2 mt-2 border-t border-border">
              <span class="text-[10px] text-foreground/60">{{ pageSummary.readTime || 'n/a' }} read</span>
              <div 
                v-if="pageSummary.timing"
                class="timing-badge group relative cursor-help"
              >
                <div class="flex items-center gap-1.5 text-[10px] text-foreground/60 hover:text-foreground transition-colors">
                  <!-- AI provider badge -->
                  <span 
                    v-if="pageSummary.timing.provider === 'chrome-ai'"
                    class="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full bg-linear-to-r from-blue-500/20 to-purple-500/20 border border-blue-500/30 text-blue-400"
                    title="Using Chrome Built-in AI"
                  >
                    <Sparkles :size="8" />
                    <span class="text-[8px] font-medium">Chrome AI</span>
                  </span>
                  <span 
                    v-else-if="pageSummary.timing.provider === 'ollama' || pageSummary.timing.model"
                    class="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full bg-green-500/15 border border-green-500/30 text-green-400"
                    title="Using Ollama Local AI"
                  >
                    <Server :size="8" />
                    <span class="text-[8px] font-medium">Ollama</span>
                  </span>
                  <span>{{ formatTime(pageSummary.timing.total) }}</span>
                  <span v-if="pageSummary.timing.cached" class="text-primary">· cached</span>
                </div>
                <!-- hover breakdown tooltip -->
                <div class="timing-tooltip absolute bottom-full right-0 mb-2 px-2.5 py-2 rounded-lg bg-popover border border-border shadow-lg opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-150 z-50 min-w-40">
                  <div class="text-[10px] font-medium text-foreground mb-1.5 uppercase tracking-wide">time breakdown</div>
                  <div class="space-y-1 text-[10px]">
                    <div class="flex justify-between gap-3">
                      <span class="text-foreground/70">extraction</span>
                      <span class="text-foreground font-mono">{{ formatTime(pageSummary.timing.extraction || 0) }}</span>
                    </div>
                    <div class="flex justify-between gap-3">
                      <span class="text-foreground/70">llm</span>
                      <span class="text-foreground font-mono">{{ formatTime(pageSummary.timing.llm || 0) }}</span>
                    </div>
                    <div class="flex justify-between gap-3 pt-1 border-t border-border">
                      <span class="text-foreground">total</span>
                      <span class="text-primary font-mono font-medium">{{ formatTime(pageSummary.timing.total) }}</span>
                    </div>
                    <div v-if="pageSummary.timing.provider" class="flex items-center justify-between pt-1 border-t border-border">
                      <span class="text-foreground/60">provider</span>
                      <span 
                        :class="[
                          'font-medium',
                          pageSummary.timing.provider === 'chrome-ai' ? 'text-blue-400' : 'text-green-400'
                        ]"
                      >
                        {{ pageSummary.timing.provider === 'chrome-ai' ? 'Chrome AI' : 'Ollama' }}
                      </span>
                    </div>
                    <div v-if="pageSummary.timing.model" class="pt-1 text-foreground/60 truncate">
                      {{ pageSummary.timing.model }}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </Transition>
    </div>

    <!-- loading state -->
    <div v-else-if="summaryLoading" class="flex items-center gap-2.5 p-3 rounded-lg bg-muted border border-border">
      <Loader2 class="w-4 h-4 animate-spin text-primary" />
      <span class="text-[11px] text-foreground">analysing...</span>
    </div>

    <!-- error state with retry button -->
    <div v-else-if="summaryError" class="space-y-2">
      <div class="flex items-center gap-2.5 p-3 rounded-lg bg-destructive/15 border border-destructive/30">
        <AlertCircle :size="13" class="text-destructive shrink-0" />
        <div class="flex-1 min-w-0">
          <p class="text-[11px] text-foreground">{{ summaryError }}</p>
        </div>
      </div>
      <Button 
        v-if="!isEmailClient"
        @click="emit('manual-summary')"
        :disabled="summaryLoading"
        class="w-full text-[11px] text-muted-foreground hover:text-foreground hover:bg-muted/60"
        variant="ghost"
      >
        <Zap :size="14" class="mr-2" />
        try summarising anyway
      </Button>
    </div>

    <!-- no page state with summarise button -->
    <div v-else class="space-y-2">
      <!-- Gmail inbox (not viewing thread) -->
      <div v-if="isEmailClient && !isViewingEmailThread" class="flex items-center gap-2.5 p-3 rounded-lg bg-muted border border-border">
        <FileText :size="13" class="text-foreground/50 shrink-0" />
        <p class="text-[11px] text-foreground">email summaries appear inline in gmail</p>
      </div>
      <!-- Regular page - show summarise button -->
      <Button 
        v-else-if="!isViewingEmailThread"
        @click="emit('manual-summary')"
        :disabled="summaryLoading"
        class="w-full"
        variant="outline"
      >
        <Zap :size="14" class="mr-2" />
        summarise this page
      </Button>
    </div>
  </div>
</template>

<style scoped>
.markdown-content :deep(strong),
.markdown-content :deep(b) {
  font-weight: 600;
  color: oklch(from var(--bc) calc(l + 0.1) c h);
}

.markdown-content :deep(em),
.markdown-content :deep(i) {
  font-style: italic;
}

.markdown-content :deep(code) {
  background: oklch(from var(--bc) l c h / 0.1);
  padding: 0.1em 0.3em;
  border-radius: 3px;
  font-family: ui-monospace, monospace;
  font-size: 0.9em;
}

.markdown-content :deep(a) {
  color: oklch(from var(--p) l c h);
  text-decoration: underline;
  text-underline-offset: 2px;
}

.timing-tooltip::after {
  content: '';
  position: absolute;
  top: 100%;
  right: 12px;
  border: 5px solid transparent;
  border-top-color: oklch(from var(--b2) l c h);
}

.summary-content {
  will-change: height, opacity;
}

.summary-enter-active,
.summary-leave-active {
  transition: all 250ms cubic-bezier(0.4, 0, 0.2, 1);
}

.summary-enter-from,
.summary-leave-to {
  opacity: 0;
  max-height: 0;
}

.summary-enter-to,
.summary-leave-from {
  opacity: 1;
  max-height: 500px;
}
</style>
