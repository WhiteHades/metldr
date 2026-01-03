import { stripThinking } from '../utils/text'
import type {
  OllamaModel,
  OllamaTagsResponse,
  ChatMessage,
  CompleteResult,
  TaskType
} from '../types'

interface CompleteOptions {
  temperature?: number
  top_p?: number
  format?: unknown
  longContext?: boolean
}

export class OllamaService {
  static BASE_URL = 'http://127.0.0.1:11434'
  static TIMEOUT_HEALTH = 3000
  static TIMEOUT_CHAT = 30000
  static TIMEOUT_CHAT_LONG = 120000

  static PRIORITY: Record<TaskType, string[]> = {
    word_lookup: ['llama3.2:1b', 'qwen2.5:1.5b', 'llama3.2:3b'],
    page_summary: ['llama3.2:3b', 'llama3.2:1b', 'qwen2.5:1.5b'],
    email_summary: ['llama3.2:3b', 'llama3.2:1b', 'qwen2.5:3b']
  }

  static async checkAvailable(): Promise<{ available: boolean; models: string[] }> {
    try {
      const res = await fetch(`${this.BASE_URL}/api/tags`, {
        signal: AbortSignal.timeout(this.TIMEOUT_HEALTH)
      })

      if (!res.ok) return { available: false, models: [] }

      const data: OllamaTagsResponse = await res.json()
      const models = data.models?.map(m => m.name) || []

      return { available: true, models }
    } catch {
      return { available: false, models: [] }
    }
  }

  static async complete(model: string, messages: ChatMessage[], options: CompleteOptions = {}): Promise<CompleteResult> {
    try {
      const body: Record<string, unknown> = {
        model,
        messages,
        stream: false,
        options: {
          temperature: options.temperature ?? 0,
          top_p: options.top_p ?? 0.9
        }
      }

      if (options.format) body.format = options.format

      const timeout = options.longContext ? this.TIMEOUT_CHAT_LONG : this.TIMEOUT_CHAT

      const res = await fetch(`${this.BASE_URL}/api/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(timeout)
      })

      if (!res.ok) {
        return {
          ok: false,
          error: `ollama returned ${res.status}`
        }
      }

      const data = await res.json()
      const rawContent = data?.message?.content || data?.response
      const content = stripThinking(rawContent)

      return { ok: true, content }
    } catch (err) {
      const error = err as Error
      if (error.name === 'AbortError') {
        return { ok: false, error: 'timeout' }
      }
      console.error('[OllamaService.complete]', error.message)
      return { ok: false, error: error.message }
    }
  }

  // streaming version - yields content chunks via async generator
  static async *completeStream(
    model: string,
    messages: ChatMessage[],
    options: CompleteOptions = {}
  ): AsyncGenerator<string, void, unknown> {
    const body: Record<string, unknown> = {
      model,
      messages,
      stream: true,
      options: {
        temperature: options.temperature ?? 0,
        top_p: options.top_p ?? 0.9
      }
    }

    if (options.format) body.format = options.format

    const timeout = options.longContext ? this.TIMEOUT_CHAT_LONG : this.TIMEOUT_CHAT

    const res = await fetch(`${this.BASE_URL}/api/chat`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(timeout)
    })

    if (!res.ok || !res.body) {
      throw new Error(`ollama returned ${res.status}`)
    }

    // parse NDJSON stream
    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''

    while (true) {
      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        if (!line.trim()) continue
        try {
          const chunk = JSON.parse(line)
          if (chunk.message?.content) {
            yield stripThinking(chunk.message.content)
          }
          if (chunk.done) return
        } catch {
          // skip malformed json
        }
      }
    }
  }

  static async getUserSelected(): Promise<string | null> {
    try {
      const result = await chrome.storage.local.get('selectedModel')
      return (result.selectedModel as string) || null
    } catch (err) {
      console.error('[OllamaService.getUserSelected]', (err as Error).message)
      return null
    }
  }

  static async listAvailable(): Promise<string[]> {
    const { available, models } = await this.checkAvailable()
    return available ? models : []
  }

  static async selectBest(taskType: TaskType = 'email_summary'): Promise<string | null> {
    const userModel = await this.getUserSelected()
    if (userModel) return userModel

    const available = await this.listAvailable()
    if (!available.length) return null

    const priority = this.PRIORITY[taskType] || []
    for (const hint of priority) {
      const found = available.find(m => m.includes(hint))
      if (found) return found
    }

    return available[0]
  }

  static async tryWithFallback<T>(taskFn: (model: string) => Promise<T>, taskType: TaskType = 'email_summary'): Promise<T> {
    const userModel = await this.getUserSelected()
    const priority = userModel ? [userModel] : (this.PRIORITY[taskType] || [])

    const available = await this.listAvailable()
    if (!available.length) throw new Error('no models available')

    const modelList: string[] = []
    const seen = new Set<string>()

    for (const hint of priority) {
      const found = available.find(m => m.includes(hint))
      if (found && !seen.has(found)) {
        modelList.push(found)
        seen.add(found)
      }
    }

    for (const model of available) {
      if (!seen.has(model)) {
        modelList.push(model)
        seen.add(model)
      }
    }

    for (const model of modelList) {
      try {
        return await taskFn(model)
      } catch (err) {
        console.log(`[OllamaService] model ${model} failed:`, (err as Error).message)
        continue
      }
    }

    throw new Error(`all ${modelList.length} models failed`)
  }
}
