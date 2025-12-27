/**
 * Unified background messaging utility
 * 
 * Single source of truth for all content/side-panel to background communication
 */

export async function sendToBackground<T = unknown>(
  message: Record<string, unknown>,
  retries = 2
): Promise<T> {
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const response = await chrome.runtime.sendMessage(message)
      return response as T
    } catch (error) {
      const errorMessage = (error as Error).message
      if (errorMessage?.includes('Receiving end does not exist') && attempt < retries) {
        await new Promise(resolve => setTimeout(resolve, 100 * (attempt + 1)))
        continue
      }
      throw error
    }
  }
  throw new Error('sendToBackground: max retries exceeded')
}

/**
 * Measure execution time of an async function
 */
export async function withTiming<T>(
  fn: () => Promise<T>
): Promise<{ result: T; timing: number }> {
  const start = performance.now()
  const result = await fn()
  return { result, timing: Math.round(performance.now() - start) }
}

/**
 * Format timing for display
 */
export { formatTime } from '@/utils/text'
