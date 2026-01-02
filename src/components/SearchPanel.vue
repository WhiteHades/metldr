<script setup lang="ts">
import { ref } from 'vue'
import { Search, FileText, Mail, File, X, Loader2 } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface SearchResult {
  id: string
  type: 'article' | 'email' | 'pdf'
  title: string
  snippet: string
  score: number
  sourceUrl?: string
}

const searchQuery = ref('')
const results = ref<SearchResult[]>([])
const isSearching = ref(false)
const hasSearched = ref(false)

const typeIcons = {
  article: FileText,
  email: Mail,
  pdf: File
}

const typeLabels = {
  article: 'Article',
  email: 'Email',
  pdf: 'PDF'
}

async function performSearch() {
  if (!searchQuery.value.trim()) return
  
  isSearching.value = true
  hasSearched.value = true
  
  try {
    const response = await chrome.runtime.sendMessage({
      type: 'RAG_SEARCH',
      query: searchQuery.value,
      limit: 10
    })
    
    if (response?.success && response.results) {
      results.value = response.results.map((r: any) => ({
        id: r.entry.id,
        type: r.entry.type,
        title: r.entry.metadata?.title || r.entry.id.split(':').pop() || 'Untitled',
        snippet: r.entry.content.slice(0, 200) + '...',
        score: r.score,
        sourceUrl: r.entry.metadata?.sourceUrl
      }))
    } else {
      results.value = []
    }
  } catch (err) {
    console.error('[SearchPanel] Search failed:', err)
    results.value = []
  } finally {
    isSearching.value = false
  }
}

function clearSearch() {
  searchQuery.value = ''
  results.value = []
  hasSearched.value = false
}

function openResult(result: SearchResult) {
  if (result.sourceUrl && !result.sourceUrl.startsWith('email://')) {
    chrome.tabs.create({ url: result.sourceUrl })
  }
}
</script>

<template>
  <div class="search-panel p-3">
    <div class="search-input-container flex gap-2 mb-4">
      <div class="relative flex-1">
        <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input 
          v-model="searchQuery"
          placeholder="Search across all content..."
          class="pl-10 h-9 text-[13px]"
          @keydown.enter="performSearch"
        />
        <button 
          v-if="searchQuery" 
          @click="clearSearch"
          class="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>
      <Button 
        size="sm" 
        @click="performSearch" 
        :disabled="isSearching || !searchQuery.trim()"
        class="h-9 px-3 text-[12px]"
      >
        <Loader2 v-if="isSearching" class="h-3.5 w-3.5 animate-spin" />
        <span v-else>Search</span>
      </Button>
    </div>
    
    <div v-if="isSearching" class="flex items-center justify-center py-8">
      <Loader2 class="h-5 w-5 animate-spin text-muted-foreground" />
      <span class="ml-2 text-[12px] text-muted-foreground">Searching...</span>
    </div>
    
    <div v-else-if="hasSearched && results.length === 0" class="text-center py-8">
      <Search class="h-8 w-8 mx-auto mb-2 opacity-30" />
      <p class="text-[12px] text-muted-foreground">No results for "{{ searchQuery }}"</p>
    </div>
    
    <div v-else-if="results.length > 0" class="results-list space-y-2">
      <div
        v-for="result in results"
        :key="result.id"
        class="result-item p-2.5 rounded-lg border border-border/50 bg-card/50 hover:bg-accent/50 cursor-pointer transition-all duration-150"
        @click="openResult(result)"
      >
        <div class="flex items-start gap-2.5">
          <component 
            :is="typeIcons[result.type]" 
            class="h-4 w-4 mt-0.5 text-muted-foreground shrink-0"
          />
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-1.5 mb-1">
              <span class="text-[10px] px-1.5 py-0.5 rounded-full bg-muted/80 text-muted-foreground font-medium">
                {{ typeLabels[result.type] }}
              </span>
              <span class="text-[10px] text-muted-foreground/60">
                {{ Math.round(result.score * 100) }}%
              </span>
            </div>
            <h4 class="font-medium text-[12px] leading-snug truncate text-foreground/90">{{ result.title }}</h4>
            <p class="text-[11px] text-muted-foreground/70 mt-1 result-snippet">{{ result.snippet }}</p>
          </div>
        </div>
      </div>
    </div>
    
    <div v-else class="text-center py-10">
      <div class="w-12 h-12 mx-auto mb-3 rounded-full bg-muted/30 flex items-center justify-center">
        <Search class="h-5 w-5 text-muted-foreground/50" />
      </div>
      <p class="text-[12px] text-foreground/70 mb-1">Cross-document search</p>
      <p class="text-[11px] text-muted-foreground">Find anything across your indexed content</p>
    </div>
  </div>
</template>

<style scoped>
.search-panel {
  min-height: 200px;
}

.result-item:hover {
  border-color: hsl(var(--border));
}

.result-snippet {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  line-clamp: 2;
  overflow: hidden;
}
</style>
