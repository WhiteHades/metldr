import { ref, computed } from 'vue'

export interface PdfProgress {
  stage: 'idle' | 'loading' | 'extracting' | 'summarizing' | 'indexing' | 'complete' | 'error'
  currentPage: number
  totalPages: number
  startTime: number
  error?: string
}

// singleton state for global PDF progress tracking
const progress = ref<PdfProgress>({
  stage: 'idle',
  currentPage: 0,
  totalPages: 0,
  startTime: 0
})

export function usePdfProgress() {
  const percentComplete = computed(() => {
    if (progress.value.totalPages === 0) return 0
    
    // weight stages differently
    const pageProgress = progress.value.currentPage / progress.value.totalPages
    
    switch (progress.value.stage) {
      case 'loading': return 5
      case 'extracting': return 5 + (pageProgress * 60) // 5-65%
      case 'summarizing': return 70
      case 'indexing': return 85
      case 'complete': return 100
      default: return 0
    }
  })

  const stageText = computed(() => {
    switch (progress.value.stage) {
      case 'loading': 
        return 'Loading PDF...'
      case 'extracting': 
        return `Extracting page ${progress.value.currentPage} of ${progress.value.totalPages}...`
      case 'summarizing': 
        return 'Generating summary...'
      case 'indexing': 
        return 'Indexing for search...'
      case 'complete': 
        return 'Done!'
      case 'error':
        return progress.value.error || 'Error occurred'
      default: 
        return ''
    }
  })

  const estimatedTimeRemaining = computed(() => {
    if (progress.value.stage !== 'extracting' || progress.value.currentPage === 0) {
      return null
    }
    
    const elapsed = Date.now() - progress.value.startTime
    const msPerPage = elapsed / progress.value.currentPage
    const remaining = (progress.value.totalPages - progress.value.currentPage) * msPerPage
    
    // add overhead for summarizing stage
    const summarizeEstimate = 3000 // ~3s for summary
    
    return Math.ceil((remaining + summarizeEstimate) / 1000)
  })

  const isActive = computed(() => {
    return progress.value.stage !== 'idle' && progress.value.stage !== 'complete' && progress.value.stage !== 'error'
  })

  function startProgress(totalPages: number) {
    progress.value = { 
      stage: 'loading', 
      currentPage: 0, 
      totalPages, 
      startTime: Date.now() 
    }
  }

  function updatePage(page: number) {
    progress.value.stage = 'extracting'
    progress.value.currentPage = page
  }

  function setStage(stage: PdfProgress['stage']) {
    progress.value.stage = stage
  }

  function setError(error: string) {
    progress.value.stage = 'error'
    progress.value.error = error
  }

  function reset() {
    progress.value = { 
      stage: 'idle', 
      currentPage: 0, 
      totalPages: 0, 
      startTime: 0 
    }
  }

  return {
    progress,
    percentComplete,
    stageText,
    estimatedTimeRemaining,
    isActive,
    startProgress,
    updatePage,
    setStage,
    setError,
    reset
  }
}
