<script setup lang="ts">
import { ref, computed, nextTick, watch, onMounted } from 'vue'
import { Mail, Globe, FileText, Copy, ChevronDown, ChevronUp, RefreshCw, ExternalLink } from 'lucide-vue-next'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'
import ChatComposer from '@/components/ChatComposer.vue'
import { marked } from 'marked'
import { formatTime } from '@/utils/text'

interface ChatSource {
  index: number
  title: string
  url: string
  type: 'email' | 'page' | 'pdf'
  score: number
  snippet: string
}

interface ChatMessage {
  role: 'user' | 'assistant'
  content: string
  sources?: ChatSource[]
  timing?: { total: number; model?: string }
  error?: boolean
}

const chatInput = ref('')
const messages = ref<ChatMessage[]>([])
const loading = ref(false)
const viewportRef = ref<HTMLDivElement | null>(null)
const composerRef = ref<InstanceType<typeof ChatComposer> | null>(null)
const expandedSources = ref<Set<string>>(new Set())
const isInitialLoad = ref(true) // guard to prevent saving during initial load

const STORAGE_KEY = 'global_chat_state'

const isEmpty = computed(() => !chatInput.value.trim())
const isThreadEmpty = computed(() => messages.value.length === 0 && !loading.value)

// better example queries for indexed content
const exampleQueries = [
  'what orders did i place?',
  'find invoice details',
  'key points from recent articles'
]

marked.setOptions({ breaks: true, gfm: true })

// persist messages (skip during initial load to prevent race condition)
watch(messages, async (newMsgs) => {
  if (isInitialLoad.value) {
    console.log('[GlobalSearch] skipping save during initial load')
    return
  }
  
  const toSave = newMsgs.map(m => ({
    ...m,
    // deep clone sources to prevent reactivity issues
    sources: Array.isArray(m.sources) && m.sources.length > 0
      ? m.sources.map(s => ({
          index: s.index,
          title: s.title,
          url: s.url,
          type: s.type,
          score: s.score,
          snippet: s.snippet
        }))
      : []
  }))
  
  console.log('[GlobalSearch] saving messages:', toSave.length, 'sources per msg:', toSave.map(m => m.sources?.length || 0))
  
  try {
    await chrome.storage.local.set({ 
      [STORAGE_KEY]: { 
        messages: toSave,
        timestamp: Date.now()
      }
    })
  } catch (e) {
    console.error('[GlobalSearch] save failed:', e)
  }
}, { deep: true })

onMounted(async () => {
  try {
    const stored = await chrome.storage.local.get(STORAGE_KEY) as Record<string, { messages?: ChatMessage[]; timestamp?: number }>
    const state = stored[STORAGE_KEY]
    console.log('[GlobalSearch] loading state:', state ? 'found' : 'empty', 'msgs:', state?.messages?.length, 'sources per msg:', state?.messages?.map(m => m.sources?.length || 0))
    if (state?.messages?.length) {
      const age = Date.now() - (state.timestamp || 0)
      if (age < 60 * 60 * 1000) {
        messages.value = state.messages.map(m => ({
          ...m,
          sources: Array.isArray(m.sources) 
            ? m.sources.map((s, idx) => ({
                ...s,
                index: s.index ?? (idx + 1),
                title: s.title || 'Source',
                url: s.url || '',
                type: s.type || 'page',
                score: s.score || 0,
                snippet: s.snippet || ''
              }))
            : []
        }))
        console.log('[GlobalSearch] loaded messages:', messages.value.length, 'sources:', messages.value.map(m => m.sources?.length || 0))
      } else {
        console.log('[GlobalSearch] state too old, age:', age)
      }
    }
  } catch (e) {
    console.error('[GlobalSearch] load failed:', e)
  } finally {
    isInitialLoad.value = false
  }
})

function scrollToBottom() {
  nextTick(() => {
    if (viewportRef.value) {
      viewportRef.value.scrollTo({ top: viewportRef.value.scrollHeight, behavior: 'smooth' })
    }
  })
}

watch(() => messages.value.length, scrollToBottom)
watch(loading, scrollToBottom)

function renderContent(content: string, sources?: ChatSource[]): string {
  if (!content) return ''
  console.log('[GlobalSearch.renderContent] sources:', sources?.length, sources?.map(s => ({ index: s.index, title: s.title?.slice(0, 30) })))
  let processed = content.replace(/\[(\d+)\]/g, (match, num) => {
    const idx = parseInt(num)
    const source = sources?.find(s => s.index === idx)
    if (source) {
      return `<a href="#" class="citation" data-source-index="${idx}">[${idx}]</a>`
    }
    return `<span class="citation-dead">[${idx}]</span>`
  })
  return marked.parse(processed, { async: false }) as string
}

function toggleSourceExpand(msgIndex: number, sourceIndex: number) {
  const key = `${msgIndex}-${sourceIndex}`
  if (expandedSources.value.has(key)) {
    expandedSources.value.delete(key)
  } else {
    expandedSources.value.add(key)
  }
  expandedSources.value = new Set(expandedSources.value)
}

function isSourceExpanded(msgIndex: number, sourceIndex: number): boolean {
  return expandedSources.value.has(`${msgIndex}-${sourceIndex}`)
}

function handleCitationClick(e: Event, sources: ChatSource[] | undefined, msgIndex: number) {
  const target = e.target as HTMLElement
  if (target.classList.contains('citation')) {
    e.preventDefault()
    const idx = parseInt(target.dataset.sourceIndex || '0')
    toggleSourceExpand(msgIndex, idx)
  }
}

async function openSource(source: ChatSource) {
  if (!source.url) return
  try {
    if (source.type === 'email') {
      const emailId = source.url.replace('email://', '').split('/').pop()
      await chrome.tabs.create({ url: `https://mail.google.com/mail/u/0/#inbox/${emailId}` })
    } else {
      await chrome.tabs.create({ url: source.url })
    }
  } catch {}
}

function getIcon(type: string) {
  switch (type) {
    case 'email': return Mail
    case 'pdf': return FileText
    default: return Globe
  }
}

function truncate(text: string | undefined | null, len = 100): string {
  if (!text) return ''
  return text.length <= len ? text : text.slice(0, len) + '...'
}

function formatScore(score: number): string {
  if (!score || typeof score !== 'number' || score <= 0) return ''
  // scores are now 0-100 directly
  const pct = Math.min(Math.round(score), 100)
  return pct > 0 ? `${pct}%` : ''
}

function useExample(query: string) {
  chatInput.value = query
}

// dedupe sources by URL
function dedupeSourcesByUrl(sources: ChatSource[]): ChatSource[] {
  const seen = new Map<string, ChatSource>()
  for (const s of sources) {
    const key = s.url || s.title
    if (!seen.has(key) || s.score > (seen.get(key)?.score || 0)) {
      seen.set(key, s)
    }
  }
  // re-index
  return Array.from(seen.values()).map((s, i) => ({ ...s, index: i + 1 }))
}

async function sendMessage() {
  if (isEmpty.value || loading.value) return
  
  const userContent = chatInput.value.trim()
  chatInput.value = ''
  messages.value.push({ role: 'user', content: userContent })
  
  loading.value = true
  
  try {
    const chatHistory = messages.value.map(m => ({ role: m.role, content: m.content }))
    const response = await chrome.runtime.sendMessage({
      type: 'GLOBAL_CHAT',
      messages: chatHistory
    }) as { ok: boolean; content?: string; sources?: ChatSource[]; timing?: { total: number; model?: string }; error?: string }
    
    if (!response?.ok) {
      messages.value.push({ role: 'assistant', content: response?.error || 'Something went wrong.', error: true })
      return
    }
    
    console.log('[GlobalSearch] raw response:', { ok: response.ok, hasContent: !!response.content, hasSources: !!response.sources, sourcesType: typeof response.sources, sourcesLen: response.sources?.length })
    const dedupedSources = dedupeSourcesByUrl(response.sources || [])
    console.log('[GlobalSearch] received sources:', response.sources?.length, 'deduped:', dedupedSources.length, dedupedSources)
    
    messages.value.push({
      role: 'assistant',
      content: response.content || '',
      sources: dedupedSources,
      timing: response.timing
    })
  } catch (err) {
    messages.value.push({ role: 'assistant', content: (err as Error).message || 'Connection error.', error: true })
  } finally {
    loading.value = false
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    sendMessage()
  }
}

function clearChat() {
  messages.value = []
  expandedSources.value.clear()
  chrome.storage.local.remove(STORAGE_KEY)
}

async function copyMessage(content: string) {
  try {
    await navigator.clipboard.writeText(content)
  } catch {
    const el = document.createElement('textarea')
    el.value = content
    document.body.appendChild(el)
    el.select()
    document.execCommand('copy')
    document.body.removeChild(el)
  }
}

function retryLastMessage() {
  const lastMsg = messages.value[messages.value.length - 1]
  if (lastMsg?.error) {
    messages.value.pop()
    const lastUserIdx = messages.value.length - 1
    if (lastUserIdx >= 0 && messages.value[lastUserIdx].role === 'user') {
      chatInput.value = messages.value[lastUserIdx].content
      messages.value.pop()
      sendMessage()
    }
  }
}
</script>

<template>
  <div class="search-panel">
    <!-- main content -->
    <div ref="viewportRef" class="viewport">
      <!-- empty state -->
      <div v-if="isThreadEmpty" class="empty-state">
        <p class="empty-title">ask your saved content</p>
        <p class="empty-desc">answers from emails, pages, PDFs you've summarized</p>
        
        <div class="examples">
          <button 
            v-for="(ex, i) in exampleQueries" 
            :key="i"
            @click="useExample(ex)"
            class="example-btn"
          >
            {{ ex }}
          </button>
        </div>
      </div>
      
      <!-- conversation -->
      <div v-else class="conversation">
        <template v-for="(msg, i) in messages" :key="i">
          <!-- user query -->
          <div v-if="msg.role === 'user'" class="msg-user message-appear">
            <span class="msg-text">{{ msg.content }}</span>
          </div>
          
          <!-- assistant answer -->
          <div v-else class="msg-assistant message-appear" :class="{ error: msg.error }">
            <div class="msg-header">
              <span v-if="msg.timing" class="msg-timing">{{ formatTime(msg.timing.total) }}</span>
              <div class="msg-actions">
                <TooltipProvider v-if="msg.error">
                  <Tooltip>
                    <TooltipTrigger as-child>
                      <button @click="retryLastMessage" class="action-icon">
                        <RefreshCw :size="12" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>retry</TooltipContent>
                  </Tooltip>
                </TooltipProvider>
                <TooltipProvider v-if="!msg.error">
                  <Tooltip>
                    <TooltipTrigger as-child>
                      <button @click="copyMessage(msg.content)" class="action-icon">
                        <Copy :size="12" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>copy</TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>
            </div>
            
            <div 
              class="msg-content" 
              v-html="renderContent(msg.content, msg.sources)"
              @click="(e) => handleCitationClick(e, msg.sources, i)"
            ></div>
            
            <!-- sources -->
            <div v-if="msg.sources?.length" class="sources">
              <div 
                v-for="source in msg.sources" 
                :key="source.index"
                class="source-item"
              >
                <div class="source-row" @click="toggleSourceExpand(i, source.index)">
                  <span class="source-num">[{{ source.index }}]</span>
                  <component :is="getIcon(source.type)" :size="12" class="source-icon" />
                  <span class="source-title">{{ truncate(source.title, 45) }}</span>
                  <span v-if="formatScore(source.score)" class="source-score">{{ formatScore(source.score) }}</span>
                  <component :is="isSourceExpanded(i, source.index) ? ChevronUp : ChevronDown" :size="12" class="source-chevron" />
                </div>
                
                <div v-if="isSourceExpanded(i, source.index)" class="source-detail">
                  <p class="source-snippet">{{ source.snippet }}</p>
                  <button v-if="source.url" @click.stop="openSource(source)" class="source-open">
                    <ExternalLink :size="10" />
                    <span>open</span>
                  </button>
                  <span v-else class="source-unavailable">source unavailable</span>
                </div>
              </div>
            </div>
            <div v-else-if="msg.role === 'assistant' && !msg.error && msg.content?.includes('[1]')" class="sources-missing">
              <span class="sources-missing-text">sources not available</span>
            </div>
          </div>
        </template>
        
        <!-- loading -->
        <div v-if="loading" class="msg-assistant loading message-appear">
          <div class="msg-header"></div>
          <div class="loading-dots"><span></span><span></span><span></span></div>
        </div>
      </div>
    </div>

    <div class="shrink-0 p-3 pt-2">
      <ChatComposer
        ref="composerRef"
        v-model="chatInput"
        placeholder="ask anything about this page..."
        :loading="loading"
        :show-clear="messages.length > 0"
        @send="sendMessage"
        @clear="clearChat"
      />
    </div>
  </div>
</template>

<style scoped>
.search-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--color-background);
}

.viewport {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px;
  scrollbar-width: thin;
  scrollbar-color: color-mix(in oklch, var(--color-foreground) 15%, transparent) transparent;
}

/* empty state */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  text-align: center;
  padding: 20px;
}

.empty-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--color-foreground);
  margin: 0 0 4px 0;
}

.empty-desc {
  font-size: 11px;
  color: var(--color-muted-foreground);
  margin: 0 0 20px 0;
}

.examples {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
  max-width: 240px;
}

.example-btn {
  padding: 10px 14px;
  background: color-mix(in oklch, var(--color-muted) 50%, transparent);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  font-size: 12px;
  color: var(--color-foreground);
  text-align: left;
  cursor: pointer;
  transition: all 150ms ease;
}

.example-btn:hover {
  border-color: var(--color-primary);
  background: color-mix(in oklch, var(--color-primary) 8%, transparent);
}

/* conversation */
.conversation {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.msg-user {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  background: color-mix(in oklch, var(--color-primary) 8%, transparent);
  border-radius: 10px;
}

.msg-badge {
  font-size: 9px;
  font-weight: 700;
  color: var(--color-primary);
  background: color-mix(in oklch, var(--color-primary) 15%, transparent);
  padding: 2px 5px;
  border-radius: 4px;
  flex-shrink: 0;
}

.msg-badge.answer {
  color: var(--color-muted-foreground);
  background: var(--color-muted);
}

.msg-text {
  font-size: 13px;
  color: var(--color-foreground);
  line-height: 1.5;
}

.msg-assistant {
  padding: 12px;
  background: color-mix(in oklch, var(--color-card) 70%, transparent);
  border: 1px solid var(--color-border);
  border-radius: 10px;
}

.msg-assistant.error {
  background: color-mix(in oklch, var(--color-destructive) 5%, transparent);
  border-color: color-mix(in oklch, var(--color-destructive) 20%, transparent);
}

.msg-assistant.loading {
  opacity: 0.7;
}

.msg-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.msg-timing {
  font-size: 10px;
  color: var(--color-muted-foreground);
  font-family: ui-monospace, monospace;
}

.msg-actions {
  display: flex;
  gap: 4px;
  margin-left: auto;
}

.action-icon {
  padding: 4px;
  color: var(--color-muted-foreground);
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: all 150ms ease;
}

.action-icon:hover {
  color: var(--color-foreground);
  background: var(--color-muted);
}

.msg-content {
  font-size: 13px;
  line-height: 1.6;
  color: var(--color-foreground);
}

.msg-content :deep(p) { margin: 0 0 0.5em 0; }
.msg-content :deep(p:last-child) { margin-bottom: 0; }

.msg-content :deep(.citation) {
  color: var(--color-primary);
  font-weight: 600;
  cursor: pointer;
  text-decoration: none;
}

.msg-content :deep(.citation:hover) {
  text-decoration: underline;
}

.msg-content :deep(.citation-dead) {
  color: var(--color-muted-foreground);
}

/* sources */
.sources {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px dashed var(--color-border);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.source-item {
  background: var(--color-background);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  overflow: hidden;
}

.source-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  cursor: pointer;
  transition: background 150ms ease;
}

.source-row:hover {
  background: color-mix(in oklch, var(--color-primary) 5%, transparent);
}

.source-num {
  font-size: 10px;
  font-weight: 700;
  color: var(--color-primary);
}

.source-icon {
  color: var(--color-muted-foreground);
  flex-shrink: 0;
}

.source-title {
  flex: 1;
  font-size: 11px;
  color: var(--color-foreground);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.source-score {
  font-size: 10px;
  color: var(--color-muted-foreground);
  font-family: ui-monospace, monospace;
}

.source-chevron {
  color: var(--color-muted-foreground);
}

.source-detail {
  padding: 10px;
  background: color-mix(in oklch, var(--color-muted) 30%, transparent);
  border-top: 1px dashed var(--color-border);
}

.source-snippet {
  font-size: 11px;
  line-height: 1.5;
  color: var(--color-muted-foreground);
  margin: 0 0 8px 0;
  padding: 8px;
  background: var(--color-background);
  border-left: 2px solid var(--color-primary);
  border-radius: 0 4px 4px 0;
}

.source-open {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  font-size: 10px;
  color: var(--color-primary);
  background: var(--color-background);
  border: 1px solid color-mix(in oklch, var(--color-primary) 30%, transparent);
  border-radius: 4px;
  cursor: pointer;
  transition: all 150ms ease;
}

.source-open:hover {
  background: color-mix(in oklch, var(--color-primary) 10%, transparent);
}

.source-unavailable {
  font-size: 10px;
  color: var(--color-muted-foreground);
  font-style: italic;
}

.sources-missing {
  margin-top: 8px;
  padding: 8px 12px;
  background: color-mix(in oklch, var(--color-muted) 30%, transparent);
  border-radius: 6px;
}

.sources-missing-text {
  font-size: 11px;
  color: var(--color-muted-foreground);
  font-style: italic;
}

/* loading */
.loading-dots {
  display: flex;
  gap: 4px;
}

.loading-dots span {
  width: 6px;
  height: 6px;
  background: var(--color-primary);
  border-radius: 50%;
  animation: bounce 1.4s infinite ease-in-out both;
}

.loading-dots span:nth-child(1) { animation-delay: 0s; }
.loading-dots span:nth-child(2) { animation-delay: 0.16s; }
.loading-dots span:nth-child(3) { animation-delay: 0.32s; }

@keyframes bounce {
  0%, 80%, 100% { transform: scale(0.6); opacity: 0.4; }
  40% { transform: scale(1); opacity: 1; }
}

/* message animations */
.message-appear {
  animation: messageSlideIn 0.25s ease-out forwards;
}

@keyframes messageSlideIn {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
