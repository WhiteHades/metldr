import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { storageService } from '@/services/StorageService'
import { logger } from '@/services/LoggerService'

const log = logger.createScoped('ThemeStore')

interface ThemeColors {
  name: string
  primary: string
  secondary: string
  accent: string
  warning: string
  error: string
  success: string
  background: string
  card: string
  foreground: string
  muted: string
  border: string
  shadow: string
}

// note: color values must stay in sync with src/lib/ThemeManager.ts for content script
export const useThemeStore = defineStore('theme', () => {
  const themes: Record<string, ThemeColors> = {
    catppuccin: {
      name: 'catppuccin',
      primary: 'oklch(0.87 0.04 30)',
      secondary: 'oklch(0.72 0.13 290)',
      accent: 'oklch(0.77 0.12 35)',
      warning: 'oklch(0.86 0.09 75)',
      error: 'oklch(0.67 0.18 15)',
      success: 'oklch(0.73 0.13 140)',
      background: 'oklch(0.19 0.02 265)',
      card: 'oklch(0.23 0.02 265)',
      foreground: 'oklch(0.87 0.03 250)',
      muted: 'oklch(0.54 0.03 250)',
      border: 'oklch(0.87 0.04 30 / 0.2)',
      shadow: 'oklch(0.87 0.04 30 / 0.15)',
    },
    gruvbox: {
      name: 'gruvbox',
      primary: 'oklch(0.66 0.15 45)',
      secondary: 'oklch(0.68 0.12 150)',
      accent: 'oklch(0.75 0.14 80)',
      warning: 'oklch(0.72 0.13 75)',
      error: 'oklch(0.60 0.18 25)',
      success: 'oklch(0.70 0.13 140)',
      background: 'oklch(0.22 0.01 70)',
      card: 'oklch(0.26 0.01 70)',
      foreground: 'oklch(0.86 0.04 70)',
      muted: 'oklch(0.58 0.02 70)',
      border: 'oklch(0.66 0.15 45 / 0.3)',
      shadow: 'oklch(0.66 0.15 45 / 0.2)',
    },
    cyberpunk: {
      name: 'cyberpunk',
      primary: 'oklch(0.75 0.22 200)',
      secondary: 'oklch(0.60 0.24 340)',
      accent: 'oklch(0.84 0.16 100)',
      warning: 'oklch(0.80 0.18 90)',
      error: 'oklch(0.55 0.20 25)',
      success: 'oklch(0.66 0.16 150)',
      background: 'oklch(0.15 0.01 265)',
      card: 'oklch(0.19 0.01 265)',
      foreground: 'oklch(0.88 0.02 265)',
      muted: 'oklch(0.50 0.02 265)',
      border: 'oklch(0.75 0.22 200 / 0.2)',
      shadow: 'oklch(0.75 0.22 200 / 0.10)',
    },
    light: {
      name: 'light',
      primary: 'oklch(0.55 0.20 230)',
      secondary: 'oklch(0.50 0.18 285)',
      accent: 'oklch(0.56 0.17 165)',
      warning: 'oklch(0.68 0.18 85)',
      error: 'oklch(0.55 0.24 28)',
      success: 'oklch(0.62 0.17 145)',
      background: 'oklch(0.98 0.01 265)',
      card: 'oklch(0.96 0.01 265)',
      foreground: 'oklch(0.20 0.02 265)',
      muted: 'oklch(0.45 0.02 265)',
      border: 'oklch(0.90 0.01 265)',
      shadow: 'oklch(0 0 0 / 0.08)',
    },
  }

  const currentTheme = ref<string>('catppuccin')
  const colors = computed(() => themes[currentTheme.value])

  const setTheme = (themeName: string): void => {
    if (themes[themeName]) {
      currentTheme.value = themeName
      applyThemeToDOM(themes[themeName])
      storageService.set('theme', themeName)
    }
  }

  const applyThemeToDOM = (theme: ThemeColors): void => {
    const root = document.documentElement

    // Toggle light class for CSS .light {} selector
    if (theme.name === 'light') {
      root.classList.add('light')
    } else {
      root.classList.remove('light')
    }

    // Apply CSS variables (shadcn-vue compatible naming)
    root.style.setProperty('--color-primary', theme.primary)
    root.style.setProperty('--color-primary-foreground', theme.foreground)
    root.style.setProperty('--color-secondary', theme.secondary)
    root.style.setProperty('--color-secondary-foreground', theme.foreground)
    root.style.setProperty('--color-accent', theme.accent)
    root.style.setProperty('--color-accent-foreground', theme.foreground)
    root.style.setProperty('--color-destructive', theme.error)
    root.style.setProperty('--color-warning', theme.warning)
    root.style.setProperty('--color-success', theme.success)
    root.style.setProperty('--color-background', theme.background)
    root.style.setProperty('--color-foreground', theme.foreground)
    root.style.setProperty('--color-card', theme.card)
    root.style.setProperty('--color-card-foreground', theme.foreground)
    root.style.setProperty('--color-muted', theme.card)
    root.style.setProperty('--color-muted-foreground', theme.muted)
    root.style.setProperty('--color-border', theme.border)
    root.style.setProperty('--color-input', theme.border)
    root.style.setProperty('--color-ring', theme.primary)
  }

  const loadSavedTheme = async (): Promise<void> => {
    try {
      const themeName = await storageService.get('theme', 'catppuccin')
      if (themeName && themes[themeName]) {
        currentTheme.value = themeName
        applyThemeToDOM(themes[themeName])
      } else {
        applyThemeToDOM(themes.catppuccin)
      }
    } catch (error) {
      log.error('failed to load theme:', error)
      applyThemeToDOM(themes.catppuccin)
    }
  }

  return {
    themes,
    currentTheme,
    colors,
    setTheme,
    loadSavedTheme,
  }
})
