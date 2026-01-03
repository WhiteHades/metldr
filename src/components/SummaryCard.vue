<script setup lang="ts">
import { computed } from 'vue'
import { formatTime, stripThinking } from '@/utils/text'
import { marked } from 'marked'
import { 
  ChevronDown, ChevronUp, RefreshCw, Zap, Sparkles, Server, AlertCircle, FileText, Loader2 
} from 'lucide-vue-next'
import { Button, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'
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
  'open-local-pdf': []
}>()

// configure marked for block parsing
marked.setOptions({
  breaks: true,
  gfm: true
})

function renderMarkdown(text: string): string {
  if (!text) return ''
  const cleaned = stripThinking(text)
  // use full parse for block-level elements (code blocks, lists, etc)
  return marked.parse(cleaned, { async: false }) as string
}

const hasContent = computed(() => props.pageSummary || props.summaryLoading || props.summaryError)
</script>

<template>
  <div class="shrink-0 p-2 pb-0">
    <!-- Prompt toast -->
    <div v-if="summaryPrompt" class="absolute right-2 bottom-2 max-w-xs rounded-lg border border-warning/50 bg-warning/10 px-3 py-2 text-(length:--font-text-small) shadow-lg z-10">
      <div class="flex items-start gap-2">
        <div class="flex-1 min-w-0">
          <div class="font-medium text-foreground truncate">summarise this page?</div>
          <div class="text-foreground/70 truncate">{{ summaryPrompt.reason }}</div>
        </div>
        <div class="flex gap-1 shrink-0">
          <button class="px-2 py-1 rounded text-(length:--font-text-small) text-foreground/70 hover:bg-muted transition-colors" @click="emit('decline-prompt')">no</button>
          <button class="px-2 py-1 rounded text-(length:--font-text-small) bg-warning/30 border border-warning/50 text-warning-foreground hover:bg-warning/40 transition-colors" @click="emit('accept-prompt')">yes</button>
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
          <h3 class="text-(length:--font-text-body) font-medium text-foreground truncate leading-tight">
            {{ pageSummary.title || 'untitled' }}
          </h3>
          <p v-if="summaryCollapsed" class="text-(length:--font-text-small) text-foreground/60 truncate">
            {{ pageSummary.bullets.length }} key points · {{ pageSummary.readTime || 'n/a' }} read
          </p>
        </div>
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger as-child>
              <button 
                @click.stop="emit('refresh')" 
                class="flex items-center justify-center w-7 h-7 rounded-md hover:bg-primary/10 shrink-0 transition-colors"
                :disabled="summaryLoading"
              >
                <RefreshCw :size="12" :class="[summaryLoading ? 'animate-spin' : '', 'text-primary']" />
              </button>
            </TooltipTrigger>
            <TooltipContent>regenerate summary</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      </div>
      
      <!-- Collapsible content -->
      <Transition name="summary">
        <div v-if="!summaryCollapsed" class="summary-content overflow-hidden">
          <div class="px-3 pb-3 pt-1 border-t border-border">
            <!-- metadata -->
            <p v-if="pageSummary.publication || pageSummary.author" class="text-(length:--font-text-small) text-foreground/60 mb-2">
              {{ pageSummary.publication }}{{ pageSummary.author ? ` · ${pageSummary.author}` : '' }}
            </p>
            
            <!-- bullets -->
            <ul class="space-y-1.5">
              <li v-for="(bullet, i) in pageSummary.bullets" :key="i" class="flex gap-2 text-(length:--font-text-body) leading-relaxed">
                <span class="text-primary shrink-0 mt-0.5">•</span>
                <span class="text-foreground markdown-content" v-html="renderMarkdown(bullet)"></span>
              </li>
            </ul>
            
            <div class="flex items-center justify-between pt-2 mt-2 border-t border-border">
              <span class="text-(length:--font-text-small) text-foreground/60">{{ pageSummary.readTime || 'n/a' }} read</span>
                <div v-if="pageSummary.timing">
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger as-child>
                        <div class="flex items-center gap-1.5 text-(length:--font-text-small) text-foreground/60 hover:text-foreground transition-colors cursor-help">
                          <span>{{ formatTime(pageSummary.timing.total) }}</span>
                          <span v-if="pageSummary.timing.model" class="opacity-60">· {{ pageSummary.timing.model }}</span>
                          <span v-if="pageSummary.timing.cached" class="text-primary">· cached</span>
                        </div>
                      </TooltipTrigger>
                      <TooltipContent class="min-w-40 p-2.5">
                        <div class="text-(length:--font-text-small) font-medium text-foreground mb-1.5 uppercase tracking-wide">time breakdown</div>
                        <div class="space-y-1 text-(length:--font-text-small)">
                          <div class="flex justify-between gap-3">
                            <span class="text-foreground/70">extraction</span>
                            <span class="text-foreground font-mono">{{ formatTime(pageSummary.timing.extraction || 0) }}</span>
                          </div>
                          <div class="flex justify-between gap-3">
                            <span class="text-foreground/70">llm</span>
                            <span class="text-foreground font-mono">{{ formatTime(pageSummary.timing.llm || 0) }}</span>
                          </div>
                          <div class="flex justify-between gap-3 pt-1 border-t border-zinc-800">
                            <span class="text-foreground">total</span>
                            <span class="text-primary font-mono font-medium">{{ formatTime(pageSummary.timing.total) }}</span>
                          </div>
                          <div v-if="pageSummary.timing.provider" class="flex items-center justify-between pt-1 border-t border-zinc-800">
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
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </div>
            </div>
          </div>
        </div>
      </Transition>
    </div>

    <!-- loading state -->
    <div v-else-if="summaryLoading" class="flex items-center gap-2.5 p-3 rounded-lg bg-muted border border-border">
      <Loader2 class="w-4 h-4 animate-spin text-primary" />
      <span class="text-(length:--font-text-small) text-foreground">analysing...</span>
    </div>

    <!-- error state with retry button -->
    <div v-else-if="summaryError" class="space-y-2">
      <!-- special state for local PDFs -->
      <div v-if="summaryError === 'LOCAL_PDF_CLICK_TO_OPEN'" class="space-y-2">
        <div class="flex items-center gap-2.5 p-3 rounded-lg bg-primary/10 border border-primary/30">
          <FileText :size="16" class="text-primary shrink-0" />
          <div class="flex-1 min-w-0">
            <p class="text-(length:--font-text-body) font-medium text-foreground">Local PDF detected</p>
            <p class="text-(length:--font-text-small) text-foreground/60">Click below to select and analyze</p>
          </div>
        </div>
        <Button 
          @click="emit('open-local-pdf')"
          class="w-full bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <FileText :size="14" class="mr-2" />
          Open PDF File
        </Button>
      </div>
      <!-- regular error -->
      <template v-else>
        <div class="flex items-center gap-2.5 p-3 rounded-lg bg-destructive/15 border border-destructive/30">
          <AlertCircle :size="13" class="text-destructive shrink-0" />
          <div class="flex-1 min-w-0">
            <p class="text-(length:--font-text-small) text-foreground">{{ summaryError }}</p>
          </div>
        </div>
        <Button 
          v-if="!isEmailClient"
          @click="emit('manual-summary')"
          :disabled="summaryLoading"
          class="w-full text-(length:--font-text-small) text-muted-foreground hover:text-foreground hover:bg-muted/60"
          variant="ghost"
        >
          <Zap :size="14" class="mr-2" />
          try summarising anyway
        </Button>
      </template>
    </div>

    <!-- no page state with summarise button -->
    <div v-else class="space-y-2">
      <!-- Gmail inbox (not viewing thread) -->
      <div v-if="isEmailClient && !isViewingEmailThread" class="flex items-center gap-2.5 p-3 rounded-lg bg-muted border border-border">
        <FileText :size="13" class="text-foreground/50 shrink-0" />
        <p class="text-(length:--font-text-small) text-foreground">email summaries appear inline in gmail</p>
      </div>
      <!-- Regular page - show summarise button -->
      <div 
        v-else-if="!isViewingEmailThread"
        @click="!summaryLoading && emit('manual-summary')"
        :class="[
          'flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-all duration-150',
          summaryLoading 
            ? 'bg-muted/50 border border-border pointer-events-none' 
            : 'bg-primary/15 border border-primary/40 hover:bg-primary/25 hover:border-primary/60'
        ]"
      >
        <span class="text-sm font-semibold text-primary">summarise this page</span>
      </div>
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

.markdown-content :deep(p) {
  margin: 0 0 0.5em 0;
}

.markdown-content :deep(p:last-child) {
  margin-bottom: 0;
}

.markdown-content :deep(ul),
.markdown-content :deep(ol) {
  margin: 0.4em 0;
  padding-left: 1.2em;
}

.markdown-content :deep(li) {
  margin: 0.2em 0;
}

.markdown-content :deep(h1),
.markdown-content :deep(h2),
.markdown-content :deep(h3) {
  font-weight: 600;
  margin: 0.6em 0 0.3em 0;
  line-height: 1.3;
}

.markdown-content :deep(h1) { font-size: 1.15em; }
.markdown-content :deep(h2) { font-size: 1.05em; }
.markdown-content :deep(h3) { font-size: 1em; }

.markdown-content :deep(pre) {
  background: color-mix(in oklch, var(--color-background) 60%, var(--color-muted));
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 8px 10px;
  overflow-x: auto;
  margin: 0.5em 0;
  font-size: 0.85em;
}

.markdown-content :deep(:not(pre) > code) {
  background: color-mix(in oklch, var(--color-muted) 50%, transparent);
  padding: 0.15em 0.35em;
  border-radius: 4px;
}

.markdown-content :deep(blockquote) {
  border-left: 3px solid var(--color-primary);
  margin: 0.5em 0;
  padding-left: 0.8em;
  color: var(--color-muted-foreground);
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
