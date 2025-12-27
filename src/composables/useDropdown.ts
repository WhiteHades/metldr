import { ref, type Ref } from 'vue'
import type { DropdownPos } from '@/types'

export function useDropdown() {
  const showModelDropdown = ref<boolean>(false)
  const modelDropdownPos = ref<DropdownPos>({ top: 0, left: 0, width: 0 })

  function updateDropdownPosition(buttonElement: Element | null, posRef: Ref<DropdownPos>): void {
    if (!buttonElement) return
    const rect = buttonElement.getBoundingClientRect()
    posRef.value = {
      top: rect.bottom + 8,
      left: rect.left,
      width: rect.width,
    }
  }

  function toggleModelDropdown(): void {
    if (!showModelDropdown.value) {
      const btn = document.querySelector('.model-selector-btn')
      updateDropdownPosition(btn, modelDropdownPos)
    }
    showModelDropdown.value = !showModelDropdown.value
  }

  function closeDropdown(): void {
    showModelDropdown.value = false
  }

  function setupDropdownClickHandler(): () => void {
    const handler = (e: MouseEvent): void => {
      const target = e.target as HTMLElement
      if (showModelDropdown.value && !target.closest('.model-selector-btn') && !target.closest('.model-dropdown')) {
        showModelDropdown.value = false
      }
    }

    document.addEventListener('click', handler)
    return () => document.removeEventListener('click', handler)
  }

  return {
    showModelDropdown,
    modelDropdownPos,
    updateDropdownPosition,
    toggleModelDropdown,
    closeDropdown,
    setupDropdownClickHandler
  }
}
