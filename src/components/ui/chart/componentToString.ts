import type { ChartConfig } from './types'

// utility to convert Vue component to HTML string for tooltips
export function componentToString(
  config: ChartConfig,
  _component: unknown,
  options?: {
    labelFormatter?: (d: number) => string
    labelKey?: string
    nameKey?: string
    indicator?: 'dot' | 'line' | 'dashed'
    hideLabel?: boolean
    hideIndicator?: boolean
  }
): string {
  // build tooltip HTML dynamically
  const indicator = options?.indicator ?? 'dot'
  const hideIndicator = options?.hideIndicator ?? false
  
  return `
    <div class="bg-popover text-popover-foreground border border-border rounded-lg shadow-lg p-2 min-w-[120px]">
      {{#if (d.label)}}
        <div class="text-xs text-muted-foreground mb-1.5 font-medium">
          ${options?.labelFormatter ? '{{labelFormatter d.x}}' : '{{d.label}}'}
        </div>
      {{/if}}
      <div class="space-y-1">
        {{#each d.data}}
          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-1.5">
              ${!hideIndicator ? `<span class="w-2 h-2 rounded-full shrink-0 ${indicator === 'line' ? 'w-0.5 h-3' : ''}" style="background-color: {{this.color}}"></span>` : ''}
              <span class="text-xs text-muted-foreground">{{this.key}}</span>
            </div>
            <span class="text-xs font-medium tabular-nums">{{this.value}}</span>
          </div>
        {{/each}}
      </div>
    </div>
  `
}
