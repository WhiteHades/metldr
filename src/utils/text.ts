const thinkPattern = /<think>[\s\S]*?<\/think>/gi
const thinkingPattern = /<thinking>[\s\S]*?<\/thinking>/gi
const reasonPattern = /<reason>[\s\S]*?<\/reason>/gi
const reasoningPattern = /<reasoning>[\s\S]*?<\/reasoning>/gi

export function stripThinking(text: string | null | undefined): string {
  if (!text) return ''
  return text
    .replace(thinkPattern, '')
    .replace(thinkingPattern, '')
    .replace(reasonPattern, '')
    .replace(reasoningPattern, '')
    .trim()
}

export function formatTime(ms: number | null | undefined): string {
  if (ms === undefined || ms === null) return ''
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

