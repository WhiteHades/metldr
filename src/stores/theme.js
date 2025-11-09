import { defineStore } from 'pinia';
import { ref, computed } from 'vue';

export const useThemeStore = defineStore('theme', () => {
  const themes = {
    default: {
      name: 'default',
      primary: '#60a5fa',      // lighter blue for dark bg
      secondary: '#a78bfa',    // lighter purple
      accent: '#34d399',       // lighter green
      bg: '#0f172a',           // dark slate
      bgSecondary: '#1e293b',  // slightly lighter slate
      text: '#e2e8f0',         // light gray text
      textMuted: '#94a3b8',    // muted gray
      border: 'rgba(148, 163, 184, 0.2)',
      glow: 'rgba(96, 165, 250, 0.15)',
      shadow: 'rgba(0, 0, 0, 0.3)',
    },
    cyberpunk: {
      name: 'cyberpunk 2077',
      primary: '#00f0ff',      
      secondary: '#ff0080',
      accent: '#fcee09',
      bg: '#000000',
      bgSecondary: '#0a0a0a',
      text: '#e4e4e7',
      textMuted: '#71717a',
      border: 'rgba(0, 240, 255, 0.3)',
      glow: 'rgba(0, 240, 255, 0.4)',
      shadow: 'rgba(0, 240, 255, 0.3)',
    },
    catppuccin: {
      name: 'catppuccin mocha',
      primary: '#f5e0dc', 
      secondary: '#cba6f7',
      accent: '#fab387',
      bg: '#1e1e2e',
      bgSecondary: '#181825',
      text: '#cdd6f4', 
      textMuted: '#6c7086',
      border: 'rgba(245, 224, 220, 0.2)',
      glow: 'rgba(245, 224, 220, 0.3)',
      shadow: 'rgba(245, 224, 220, 0.15)',
    },
    gruvbox: {
      name: 'gruvbox dark',
      primary: '#fe8019',
      secondary: '#8ec07c',
      accent: '#fabd2f', 
      bg: '#282828',
      bgSecondary: '#1d2021',
      text: '#ebdbb2',
      textMuted: '#928374',
      border: 'rgba(254, 128, 25, 0.3)',
      glow: 'rgba(254, 128, 25, 0.4)',
      shadow: 'rgba(254, 128, 25, 0.2)',
    },
  };

  const currentTheme = ref('default');

  const colors = computed(() => themes[currentTheme.value]);

  const setTheme = (themeName) => {
    if (themes[themeName]) {
      currentTheme.value = themeName;
      // save to chrome.storage for persistence
      chrome.storage.local.set({ theme: themeName });
    }
  };

  const loadSavedTheme = async () => {
    try {
      const result = await chrome.storage.local.get('theme');
      if (result.theme && themes[result.theme]) {
        currentTheme.value = result.theme;
      }
    } catch (error) {
      console.error('failed to load theme:', error);
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
