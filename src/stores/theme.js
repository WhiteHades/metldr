import { defineStore } from 'pinia';
import { ref, computed } from 'vue';

export const useThemeStore = defineStore('theme', () => {
  const themes = {
    default: {
      name: 'default',
      primary: 'oklch(0.55 0.12 240)',
      secondary: 'oklch(0.50 0.10 280)',
      accent: 'oklch(0.53 0.11 190)',
      warning: 'oklch(0.65 0.11 85)',
      error: 'oklch(0.55 0.16 20)',
      success: 'oklch(0.62 0.11 140)',
      bg: 'oklch(0.14 0.01 265)',
      bgSecondary: 'oklch(0.18 0.01 265)',
      text: 'oklch(0.75 0.01 265)',
      textMuted: 'oklch(0.50 0.01 265)',
      border: 'oklch(0.28 0.01 265)',
      borderSubtle: 'oklch(0.22 0.01 265)',
      shadow: 'oklch(0 0 0 / 0.15)',
    },
    light: {
      name: 'light',
      primary: 'oklch(0.38 0.20 240)',
      secondary: 'oklch(0.42 0.16 280)',
      accent: 'oklch(0.45 0.17 190)',
      warning: 'oklch(0.55 0.16 85)',
      error: 'oklch(0.48 0.22 20)',
      success: 'oklch(0.50 0.16 140)',
      bg: 'oklch(1.00 0.00 0)',
      bgSecondary: 'oklch(0.975 0.00 265)',
      text: 'oklch(0.18 0.02 265)',
      textMuted: 'oklch(0.58 0.01 265)',
      border: 'oklch(0.80 0.01 265)',
      borderSubtle: 'oklch(0.88 0.01 265)',
      shadow: 'oklch(0 0 0 / 0.08)',
    },
    cyberpunk: {
      name: 'cyberpunk',
      primary: 'oklch(0.75 0.22 200)',
      secondary: 'oklch(0.60 0.24 340)',
      accent: 'oklch(0.84 0.16 100)',
      warning: 'oklch(0.80 0.18 90)',
      error: 'oklch(0.55 0.20 25)',
      success: 'oklch(0.66 0.16 150)',
      bg: 'oklch(0.15 0.01 265)',
      bgSecondary: 'oklch(0.19 0.01 265)',
      text: 'oklch(0.88 0.02 265)',
      textMuted: 'oklch(0.50 0.02 265)',
      border: 'oklch(0.75 0.22 200 / 0.2)',
      borderSubtle: 'oklch(0.75 0.22 200 / 0.1)',
      shadow: 'oklch(0.75 0.22 200 / 0.10)',
    },
    catppuccin: {
      name: 'catppuccin',
      primary: 'oklch(0.87 0.04 30)',
      secondary: 'oklch(0.72 0.13 290)',
      accent: 'oklch(0.77 0.12 35)',
      warning: 'oklch(0.86 0.09 75)',
      error: 'oklch(0.67 0.18 15)',
      success: 'oklch(0.73 0.13 140)',
      bg: 'oklch(0.19 0.02 265)',
      bgSecondary: 'oklch(0.23 0.02 265)',
      text: 'oklch(0.87 0.03 250)',
      textMuted: 'oklch(0.54 0.03 250)',
      border: 'oklch(0.87 0.04 30 / 0.2)',
      borderSubtle: 'oklch(0.87 0.04 30 / 0.1)',
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
      bg: 'oklch(0.22 0.01 70)',
      bgSecondary: 'oklch(0.26 0.01 70)',
      text: 'oklch(0.86 0.04 70)',
      textMuted: 'oklch(0.58 0.02 70)',
      border: 'oklch(0.66 0.15 45 / 0.3)',
      borderSubtle: 'oklch(0.66 0.15 45 / 0.15)',
      shadow: 'oklch(0.66 0.15 45 / 0.2)',
    },
  };

  const currentTheme = ref('default');
  const colors = computed(() => themes[currentTheme.value]);

  const setTheme = (themeName) => {
    if (themes[themeName]) {
      currentTheme.value = themeName;
      applyThemeToDOM(themes[themeName]);
      chrome.storage.local.set({ theme: themeName });
    }
  };

  const applyThemeToDOM = (theme) => {
    const root = document.documentElement;
    root.style.setProperty('--color-primary', theme.primary);
    root.style.setProperty('--color-secondary', theme.secondary);
    root.style.setProperty('--color-accent', theme.accent);
    root.style.setProperty('--color-warning', theme.warning);
    root.style.setProperty('--color-error', theme.error);
    root.style.setProperty('--color-success', theme.success);
    root.style.setProperty('--color-base-100', theme.bg);
    root.style.setProperty('--color-base-200', theme.bgSecondary);
    root.style.setProperty('--color-base-300', theme.bgSecondary);
    root.style.setProperty('--color-base-content', theme.text);
    root.style.setProperty('--color-neutral-content', theme.textMuted);
    root.style.setProperty('--color-border', theme.border);
    root.style.setProperty('--tw-shadow-color', theme.shadow);
    root.style.setProperty('--p', theme.primary);
    root.style.setProperty('--pc', theme.text);
    root.style.setProperty('--s', theme.secondary);
    root.style.setProperty('--sc', theme.text);
    root.style.setProperty('--a', theme.accent);
    root.style.setProperty('--ac', theme.text);
    root.style.setProperty('--b1', theme.bg);
    root.style.setProperty('--b2', theme.bgSecondary);
    root.style.setProperty('--b3', theme.bgSecondary);
    root.style.setProperty('--bc', theme.text);
    root.style.setProperty('--su', theme.success);
    root.style.setProperty('--wa', theme.warning);
    root.style.setProperty('--er', theme.error);
  };

  const loadSavedTheme = async () => {
    try {
      const result = await chrome.storage.local.get('theme');
      if (result.theme && themes[result.theme]) {
        currentTheme.value = result.theme;
        applyThemeToDOM(themes[result.theme]);
      } else {
        applyThemeToDOM(themes.default);
      }
    } catch (error) {
      console.error('failed to load theme:', error);
      applyThemeToDOM(themes.default);
    }
  };

  return {
    themes,
    currentTheme,
    colors,
    setTheme,
    loadSavedTheme,
  };
});
