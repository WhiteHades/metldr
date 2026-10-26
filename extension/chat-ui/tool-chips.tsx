/*
 * Adapted from beautiful-ui ToolChips, copyright (c) 2026 Shane Levine.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

import { useState, type ReactNode } from "react";
import "./tool-chips.css";

const icons = {
  think: <path d="M12 2l2.4 7.2L22 12l-7.6 2.8L12 22l-2.4-7.2L2 12l7.6-2.8z" />,
  write: <g fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M17 3a2.8 2.8 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5z" /></g>,
  run: <g fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M4 17l6-5-6-5M12 19h8" /></g>,
  read: <g fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></g>,
} satisfies Record<string, ReactNode>;

export type ToolDetailLine = { text: string; tone?: "add" };

export type ToolStep = {
  id: string;
  icon: keyof typeof icons;
  label: string;
  chip: string;
  mono: boolean;
  detailMono: boolean;
  detail: readonly ToolDetailLine[];
};

type ToolChipsProps = {
  steps: readonly ToolStep[];
  labels?: { header?: string };
  className?: string;
  onToggleRow?: (id: string, open: boolean) => void;
};

export default function ToolChips({ steps, labels, className, onToggleRow }: ToolChipsProps) {
  const [open, setOpen] = useState(true);
  const [openRows, setOpenRows] = useState<ReadonlySet<string>>(() => new Set());

  const toggleRow = (id: string) => {
    const isOpen = !openRows.has(id);
    setOpenRows(current => {
      const next = new Set(current);
      if (isOpen) next.add(id);
      else next.delete(id);
      return next;
    });
    onToggleRow?.(id, isOpen);
  };

  return (
    <div className={`tool-chips${className ? ` ${className}` : ""}`}>
      <button type="button" aria-expanded={open} onClick={() => setOpen(value => !value)} className="tool-chips__header">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="tool-chips__header-chevron" data-open={open}>
          <path d="M6 9l6 6 6-6" />
        </svg>
        <span className="tool-chips__count tabular-nums">{labels?.header ?? `${steps.length} tool ${steps.length === 1 ? "call" : "calls"}`}</span>
      </button>

      <div className="tool-chips__rows" data-open={open} inert={!open}>
        <div className="tool-chips__clip">
          <div className="tool-chips__list">
            {steps.map(row => {
              const rowOpen = openRows.has(row.id);
              return (
                  <div key={row.id} className="tool-chips__row-entry" data-native-tool={row.id}>
                  <button type="button" aria-expanded={rowOpen} onClick={() => toggleRow(row.id)} className="tool-chips__row" data-expanded={rowOpen}>
                    <span className="tool-chips__icon">
                      <svg width="13" height="13" viewBox="0 0 24 24" fill={row.icon === "think" ? "currentColor" : "none"} stroke="currentColor" aria-hidden="true" className="tool-chips__row-icon" data-expanded={rowOpen}>
                        {icons[row.icon]}
                      </svg>
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="tool-chips__row-chevron" data-expanded={rowOpen}>
                        <path d="M6 9l6 6 6-6" />
                      </svg>
                    </span>
                    <span className="tool-chips__label" title={row.label}>{row.label}</span>
                    <span className="tool-chips__chip" data-mono={row.mono}><span title={row.chip}>{row.chip}</span></span>
                  </button>

                  <div className="tool-chips__details" data-open={rowOpen} aria-hidden={!rowOpen}>
                    <div className="tool-chips__detail-clip">
                      <div className="tool-chips__detail-list">
                        {row.detail.map((line, index) => (
                          <span key={`${row.id}-${index}`} className="tool-chips__detail" data-mono={row.detailMono} data-tone={line.tone ?? "default"}>
                            {line.text}
                          </span>
                        ))}
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
