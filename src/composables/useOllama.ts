import { ref } from 'vue'
import { sendToBackground } from './useMessaging'
import { storageService } from '@/services/StorageService'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('useOllama')

type OllamaStatus = 'checking' | 'ready' | 'not-found' | 'error'

const ollamaStatus = ref<OllamaStatus>('checking')
const availableModels = ref<string[]>([])
const selectedModel = ref<string>('')

interface OllamaHealthResponse {
  success?: boolean
  connected?: boolean
  models?: string[]
}

export function useOllama() {
  async function checkOllama(showChecking = true): Promise<boolean> {
    if (showChecking && ollamaStatus.value !== 'ready') {
      ollamaStatus.value = 'checking'
    }
    
    try {
      let response: OllamaHealthResponse | null = null
      for (let attempt = 0; attempt < 3; attempt++) {
        try {
          response = await sendToBackground({ type: 'CHECK_OLLAMA_HEALTH' }) as OllamaHealthResponse
          if (response && response.success !== undefined) break
          await new Promise(r => setTimeout(r, 150 * (attempt + 1)))
        } catch {
          await new Promise(r => setTimeout(r, 150 * (attempt + 1)))
        }
      }
      
      if (!response || response.success === undefined) {
        // always update to not-found when we can't reach ollama
        ollamaStatus.value = 'not-found'
        return false
      }
      
      const { connected, models } = response
      
      if (connected && models && models.length > 0) {
        ollamaStatus.value = 'ready'
        availableModels.value = models
        
        try {
          const storedModel = await storageService.get<string>('selectedModel', '')
          if (storedModel && models.includes(storedModel)) {
            selectedModel.value = storedModel
          } else if (!selectedModel.value || !models.includes(selectedModel.value)) {
            selectedModel.value = models[0]
          }
        } catch {
          if (!selectedModel.value) selectedModel.value = models[0]
        }
        return true
      }
      
      // ollama not connected - always update status
      ollamaStatus.value = 'not-found'
      return false
    } catch {
      ollamaStatus.value = 'not-found'
      return false
    }
  }

  async function selectModel(model: string): Promise<void> {
    selectedModel.value = model
    try {
      await storageService.set('selectedModel', model)
    } catch (e) {
      log.error('failed to save model', e)
    }
  }

  return {
    ollamaStatus,
    availableModels,
    selectedModel,
    checkOllama,
    selectModel
  }
}
