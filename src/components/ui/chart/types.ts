import type { Component } from 'vue'

export interface ChartConfig {
  [key: string]: {
    label: string
    color?: string
    icon?: Component
    theme?: {
      light: string
      dark: string
    }
  }
}
