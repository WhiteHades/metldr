export function useTransitions() {
  function onBeforeEnter(el: Element): void {
    const htmlEl = el as HTMLElement
    htmlEl.style.height = '0'
    htmlEl.style.opacity = '0'
  }

  function onEnter(el: Element, done: () => void): void {
    const htmlEl = el as HTMLElement
    htmlEl.offsetHeight // force reflow
    htmlEl.style.transition = 'height 250ms cubic-bezier(0.4, 0, 0.2, 1), opacity 200ms ease-out'
    htmlEl.style.height = htmlEl.scrollHeight + 'px'
    htmlEl.style.opacity = '1'
    
    htmlEl.addEventListener('transitionend', function handler(e: TransitionEvent) {
      if (e.propertyName === 'height') {
        htmlEl.removeEventListener('transitionend', handler)
        done()
      }
    })
  }

  function onAfterEnter(el: Element): void {
    const htmlEl = el as HTMLElement
    htmlEl.style.height = 'auto'
    htmlEl.style.transition = ''
  }

  function onBeforeLeave(el: Element): void {
    const htmlEl = el as HTMLElement
    htmlEl.style.height = htmlEl.scrollHeight + 'px'
    htmlEl.style.opacity = '1'
  }

  function onLeave(el: Element, done: () => void): void {
    const htmlEl = el as HTMLElement
    htmlEl.offsetHeight // force reflow
    htmlEl.style.transition = 'height 200ms cubic-bezier(0.4, 0, 0.2, 1), opacity 150ms ease-in'
    htmlEl.style.height = '0'
    htmlEl.style.opacity = '0'
    
    htmlEl.addEventListener('transitionend', function handler(e: TransitionEvent) {
      if (e.propertyName === 'height') {
        htmlEl.removeEventListener('transitionend', handler)
        done()
      }
    })
  }

  function onAfterLeave(el: Element): void {
    const htmlEl = el as HTMLElement
    htmlEl.style.height = ''
    htmlEl.style.transition = ''
  }

  return {
    onBeforeEnter,
    onEnter,
    onAfterEnter,
    onBeforeLeave,
    onLeave,
    onAfterLeave
  }
}
