<template>
  <Dialog :open="open" @update:open="$emit('update:open', $event)">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle class="flex items-center gap-2">
          <Lock class="w-5 h-5 text-primary" />
          Password Protected PDF
        </DialogTitle>
        <DialogDescription>
          This PDF is encrypted. Enter the password to unlock it.
        </DialogDescription>
      </DialogHeader>
      
      <form @submit.prevent="handleSubmit" class="space-y-4">
        <div class="relative">
          <input
            v-model="password"
            :type="showPassword ? 'text' : 'password'"
            placeholder="Enter password"
            class="w-full px-4 py-2.5 pr-10 rounded-lg border border-input bg-background text-foreground focus:ring-2 focus:ring-primary/50 focus:border-primary transition-all"
            :disabled="loading"
            autofocus
          />
          <button
            type="button"
            @click="showPassword = !showPassword"
            class="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
          >
            <Eye v-if="!showPassword" class="w-4 h-4" />
            <EyeOff v-else class="w-4 h-4" />
          </button>
        </div>
        
        <p v-if="error" class="text-sm text-destructive flex items-center gap-1.5">
          <AlertCircle class="w-4 h-4" />
          {{ error }}
        </p>
        
        <div class="flex justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            @click="$emit('update:open', false)"
            :disabled="loading"
          >
            Cancel
          </Button>
          <Button type="submit" :disabled="!password || loading">
            <Loader2 v-if="loading" class="w-4 h-4 mr-2 animate-spin" />
            {{ loading ? 'Unlocking...' : 'Unlock' }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { Lock, Eye, EyeOff, AlertCircle, Loader2 } from 'lucide-vue-next'
import { 
  Dialog, 
  DialogContent, 
  DialogHeader, 
  DialogTitle, 
  DialogDescription 
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  'update:open': [value: boolean]
  'submit': [password: string]
}>()

const password = ref('')
const showPassword = ref(false)
const loading = ref(false)
const error = ref('')

function handleSubmit() {
  if (!password.value) return
  
  error.value = ''
  loading.value = true
  
  emit('submit', password.value)
}

// exposed for parent to call on error
function setError(message: string) {
  error.value = message
  loading.value = false
}

function reset() {
  password.value = ''
  error.value = ''
  loading.value = false
  showPassword.value = false
}

defineExpose({ setError, reset })
</script>
