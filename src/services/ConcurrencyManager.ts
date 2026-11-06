export type OperationType = 'chat' | 'indexing' | 'summary'

interface QueuedOperation {
  sourceUrl: string
  run: (signal: AbortSignal, settled: () => void) => void
  reject: (error: Error) => void
}

interface OperationGroup {
  limit: number
  active: Map<string, AbortController>
  queue: QueuedOperation[]
}

function group(limit: number): OperationGroup {
  return { limit, active: new Map(), queue: [] }
}

class ConcurrencyManager {
  private readonly groups: Record<OperationType, OperationGroup> = {
    chat: group(3),
    indexing: group(3),
    summary: group(2)
  }

  execute<T>(
    type: OperationType,
    sourceUrl: string,
    operation: (signal: AbortSignal) => Promise<T>
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      this.groups[type].queue.push({
        sourceUrl,
        run: (signal, settled) => {
          void Promise.resolve().then(() => operation(signal)).finally(settled).then(resolve, reject)
        },
        reject
      })
      this.processQueue(type)
    })
  }

  private processQueue(type: OperationType): void {
    const { active, queue, limit } = this.groups[type]
    for (let index = 0; index < queue.length && active.size < limit;) {
      const next = queue[index]
      if (!next) break
      if (active.has(next.sourceUrl)) {
        index += 1
        continue
      }

      queue.splice(index, 1)
      const controller = new AbortController()
      active.set(next.sourceUrl, controller)
      next.run(controller.signal, () => {
        active.delete(next.sourceUrl)
        this.processQueue(type)
      })
    }
  }

  getActiveCount(type: OperationType): number {
    return this.groups[type].active.size
  }

  getQueueLength(type: OperationType): number {
    return this.groups[type].queue.length
  }

  isRunning(type: OperationType, sourceUrl: string): boolean {
    return this.groups[type].active.has(sourceUrl)
  }

  abort(type: OperationType, sourceUrl: string): boolean {
    const { active, queue } = this.groups[type]
    const controller = active.get(sourceUrl)
    let cancelled = false
    for (let index = queue.length - 1; index >= 0; index -= 1) {
      const next = queue[index]
      if (next?.sourceUrl === sourceUrl) {
        queue.splice(index, 1)
        next.reject(new Error('Operation cancelled'))
        cancelled = true
      }
    }
    controller?.abort()
    return cancelled || controller !== undefined
  }

  abortAll(type: OperationType): void {
    const { active, queue } = this.groups[type]
    const controllers = Array.from(active.values())
    for (const next of queue.splice(0)) {
      next.reject(new Error('All operations cancelled'))
    }
    for (const controller of controllers) controller.abort()
  }

  abortForUrl(sourceUrl: string): void {
    this.abort('chat', sourceUrl)
    this.abort('indexing', sourceUrl)
    this.abort('summary', sourceUrl)
  }

  getStatus(): Record<OperationType, { active: number; queued: number }> {
    return {
      chat: { active: this.getActiveCount('chat'), queued: this.getQueueLength('chat') },
      indexing: { active: this.getActiveCount('indexing'), queued: this.getQueueLength('indexing') },
      summary: { active: this.getActiveCount('summary'), queued: this.getQueueLength('summary') }
    }
  }
}

export const concurrencyManager = new ConcurrencyManager()
