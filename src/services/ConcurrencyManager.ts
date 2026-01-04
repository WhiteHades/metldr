export type OperationType = 'chat' | 'indexing' | 'summary'

interface QueuedOperation<T> {
  id: string
  sourceUrl: string
  execute: (signal: AbortSignal) => Promise<T>
  resolve: (value: T) => void
  reject: (error: Error) => void
  abortController: AbortController
  timestamp: number
}

interface ActiveOperation {
  promise: Promise<unknown>
  abortController: AbortController
}

interface OperationLimits {
  chat: number
  indexing: number
  summary: number
}

class ConcurrencyManager {
  private readonly limits: OperationLimits = {
    chat: 3,
    indexing: 3,
    summary: 2
  }

  private activeOperations = new Map<OperationType, Map<string, ActiveOperation>>()
  private queues = new Map<OperationType, QueuedOperation<unknown>[]>()

  constructor() {
    for (const type of ['chat', 'indexing', 'summary'] as OperationType[]) {
      this.activeOperations.set(type, new Map())
      this.queues.set(type, [])
    }
  }

  async execute<T>(
    type: OperationType,
    sourceUrl: string,
    operation: (signal: AbortSignal) => Promise<T>
  ): Promise<T> {
    const active = this.activeOperations.get(type)!
    const limit = this.limits[type]

    const existingForUrl = active.get(sourceUrl)
    if (existingForUrl) {
      console.log(`[ConcurrencyManager] ${type} for ${sourceUrl.slice(0, 50)} already running, waiting...`)
      try {
        await existingForUrl.promise
      } catch {}
      return this.execute(type, sourceUrl, operation)
    }

    if (active.size < limit) {
      return this.runOperation(type, sourceUrl, operation)
    }

    console.log(`[ConcurrencyManager] ${type} limit (${limit}) reached, queueing...`)
    return new Promise((resolve, reject) => {
      const abortController = new AbortController()
      const queue = this.queues.get(type)!
      queue.push({
        id: `${type}:${sourceUrl}:${Date.now()}`,
        sourceUrl,
        execute: operation as (signal: AbortSignal) => Promise<unknown>,
        resolve: resolve as (value: unknown) => void,
        reject,
        abortController,
        timestamp: Date.now()
      })
    })
  }

  private async runOperation<T>(
    type: OperationType,
    sourceUrl: string,
    operation: (signal: AbortSignal) => Promise<T>
  ): Promise<T> {
    const active = this.activeOperations.get(type)!
    const abortController = new AbortController()
    
    const promise = operation(abortController.signal)
    active.set(sourceUrl, { promise, abortController })
    
    try {
      const result = await promise
      return result
    } finally {
      active.delete(sourceUrl)
      this.processQueue(type)
    }
  }

  private processQueue(type: OperationType): void {
    const queue = this.queues.get(type)!
    const active = this.activeOperations.get(type)!
    const limit = this.limits[type]

    while (queue.length > 0 && active.size < limit) {
      const next = queue.shift()!
      
      if (next.abortController.signal.aborted) {
        next.reject(new Error('Operation cancelled before start'))
        continue
      }
      
      if (active.has(next.sourceUrl)) {
        queue.push(next)
        continue
      }

      const abortController = next.abortController
      const promise = next.execute(abortController.signal)
      active.set(next.sourceUrl, { promise, abortController })
      
      promise
        .then(next.resolve)
        .catch(next.reject)
        .finally(() => {
          active.delete(next.sourceUrl)
          this.processQueue(type)
        })
    }
  }

  getActiveCount(type: OperationType): number {
    return this.activeOperations.get(type)?.size || 0
  }

  getQueueLength(type: OperationType): number {
    return this.queues.get(type)?.length || 0
  }

  isRunning(type: OperationType, sourceUrl: string): boolean {
    return this.activeOperations.get(type)?.has(sourceUrl) || false
  }

  abort(type: OperationType, sourceUrl: string): boolean {
    const active = this.activeOperations.get(type)
    const op = active?.get(sourceUrl)
    if (op) {
      op.abortController.abort()
      active?.delete(sourceUrl)
      console.log(`[ConcurrencyManager] Aborted active ${type} for ${sourceUrl.slice(0, 50)}`)
      return true
    }
    
    const queue = this.queues.get(type)!
    const idx = queue.findIndex(q => q.sourceUrl === sourceUrl)
    if (idx !== -1) {
      const [removed] = queue.splice(idx, 1)
      removed.abortController.abort()
      removed.reject(new Error('Operation cancelled'))
      console.log(`[ConcurrencyManager] Cancelled queued ${type} for ${sourceUrl.slice(0, 50)}`)
      return true
    }
    
    return false
  }

  abortAll(type: OperationType): void {
    const active = this.activeOperations.get(type)!
    for (const [url, op] of active) {
      op.abortController.abort()
      console.log(`[ConcurrencyManager] Aborted ${type} for ${url.slice(0, 50)}`)
    }
    active.clear()
    
    const queue = this.queues.get(type)!
    for (const op of queue) {
      op.abortController.abort()
      op.reject(new Error('All operations cancelled'))
    }
    queue.length = 0
  }

  abortForUrl(sourceUrl: string): void {
    for (const type of ['chat', 'indexing', 'summary'] as OperationType[]) {
      this.abort(type, sourceUrl)
    }
  }

  cancelQueued(type: OperationType, sourceUrl: string): void {
    this.abort(type, sourceUrl)
  }

  getStatus(): Record<OperationType, { active: number; queued: number }> {
    const status: Record<string, { active: number; queued: number }> = {}
    for (const type of ['chat', 'indexing', 'summary'] as OperationType[]) {
      status[type] = {
        active: this.getActiveCount(type),
        queued: this.getQueueLength(type)
      }
    }
    return status as Record<OperationType, { active: number; queued: number }>
  }
}

export const concurrencyManager = new ConcurrencyManager()
