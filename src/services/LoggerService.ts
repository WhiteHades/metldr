type LogLevel = 'debug' | 'info' | 'warn' | 'error'

interface LogEntry {
  timestamp: number
  level: LogLevel
  context: string
  message: string
  data?: unknown
}

const LOG_LEVELS: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3
}

class LoggerServiceClass {
  private level: LogLevel = 'info'
  private history: LogEntry[] = []
  private readonly MAX_HISTORY = 100
  private readonly PREFIX = 'metldr'

  setLevel(level: LogLevel): void {
    this.level = level
  }

  getLevel(): LogLevel {
    return this.level
  }

  private shouldLog(level: LogLevel): boolean {
    return LOG_LEVELS[level] >= LOG_LEVELS[this.level]
  }

  private formatMessage(context: string, message: string): string {
    return `${this.PREFIX}: [${context}] ${message}`
  }

  private addToHistory(entry: LogEntry): void {
    this.history.push(entry)
    if (this.history.length > this.MAX_HISTORY) {
      this.history.shift()
    }
  }

  debug(context: string, message: string, data?: unknown): void {
    if (!this.shouldLog('debug')) return

    const entry: LogEntry = { timestamp: Date.now(), level: 'debug', context, message, data }
    this.addToHistory(entry)

    if (data !== undefined) {
      console.debug(this.formatMessage(context, message), data)
    } else {
      console.debug(this.formatMessage(context, message))
    }
  }

  log(context: string, message: string, data?: unknown): void {
    if (!this.shouldLog('info')) return

    const entry: LogEntry = { timestamp: Date.now(), level: 'info', context, message, data }
    this.addToHistory(entry)

    if (data !== undefined) {
      console.log(this.formatMessage(context, message), data)
    } else {
      console.log(this.formatMessage(context, message))
    }
  }

  info(context: string, message: string, data?: unknown): void {
    this.log(context, message, data)
  }

  warn(context: string, message: string, data?: unknown): void {
    if (!this.shouldLog('warn')) return

    const entry: LogEntry = { timestamp: Date.now(), level: 'warn', context, message, data }
    this.addToHistory(entry)

    if (data !== undefined) {
      console.warn(this.formatMessage(context, message), data)
    } else {
      console.warn(this.formatMessage(context, message))
    }
  }

  error(context: string, message: string, data?: unknown): void {
    if (!this.shouldLog('error')) return

    const entry: LogEntry = { timestamp: Date.now(), level: 'error', context, message, data }
    this.addToHistory(entry)

    if (data !== undefined) {
      console.error(this.formatMessage(context, message), data)
    } else {
      console.error(this.formatMessage(context, message))
    }
  }

  getHistory(): LogEntry[] {
    return [...this.history]
  }

  getHistoryByLevel(level: LogLevel): LogEntry[] {
    return this.history.filter(e => e.level === level)
  }

  getHistoryByContext(context: string): LogEntry[] {
    return this.history.filter(e => e.context === context)
  }

  clearHistory(): void {
    this.history = []
  }

  createScoped(context: string): ScopedLogger {
    return new ScopedLogger(this, context)
  }
}

class ScopedLogger {
  constructor(
    private parent: LoggerServiceClass,
    private context: string
  ) {}

  debug(message: string, data?: unknown): void {
    this.parent.debug(this.context, message, data)
  }

  log(message: string, data?: unknown): void {
    this.parent.log(this.context, message, data)
  }

  info(message: string, data?: unknown): void {
    this.parent.info(this.context, message, data)
  }

  warn(message: string, data?: unknown): void {
    this.parent.warn(this.context, message, data)
  }

  error(message: string, data?: unknown): void {
    this.parent.error(this.context, message, data)
  }
}

export const logger = new LoggerServiceClass()
export type { LogLevel, LogEntry, ScopedLogger }
