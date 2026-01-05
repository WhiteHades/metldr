<template>
  <div 
    class="relative h-full flex flex-col"
    @drop.prevent="onDrop"
    @dragover.prevent="onDragOver"
    @dragleave.prevent="onDragLeave"
    @dragenter.prevent="onDragEnter"
  >
    <!-- drop overlay -->
    <Transition name="fade">
      <div 
        v-if="isActive"
        class="absolute inset-0 z-50 bg-primary/10 border-2 border-dashed border-primary rounded-xl flex items-center justify-center backdrop-blur-sm"
      >
        <div class="text-center">
          <FileText class="w-12 h-12 text-primary mx-auto mb-2" />
          <p class="text-sm font-medium text-primary">Drop PDF here</p>
          <p class="text-xs text-primary/60 mt-1">to summarize and index</p>
        </div>
      </div>
    </Transition>
    
    <slot />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { FileText } from 'lucide-vue-next'

const emit = defineEmits<{
  'pdf-dropped': [file: File]
  'error': [message: string]
}>()

const isActive = ref(false)
let dragCounter = 0

function onDragEnter(e: DragEvent) {
  dragCounter++
  if (hasPdfFile(e)) {
    isActive.value = true
  }
}

function onDragOver(e: DragEvent) {
  if (hasPdfFile(e)) {
    isActive.value = true
  }
}

function onDragLeave() {
  dragCounter--
  if (dragCounter === 0) {
    isActive.value = false
  }
}

function onDrop(e: DragEvent) {
  dragCounter = 0
  isActive.value = false
  
  const files = [...(e.dataTransfer?.files || [])]
  const pdfFile = files.find(f => 
    f.type === 'application/pdf' || f.name.toLowerCase().endsWith('.pdf')
  )
  
  if (!pdfFile) {
    emit('error', 'Please drop a PDF file')
    return
  }
  
  emit('pdf-dropped', pdfFile)
}

function hasPdfFile(e: DragEvent): boolean {
  const items = e.dataTransfer?.items
  if (!items) return false
  
  for (const item of items) {
    if (item.type === 'application/pdf') return true
    if (item.kind === 'file') return true // can't check extension until drop
  }
  return false
}
</script>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
