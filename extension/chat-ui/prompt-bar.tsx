/*!
 * Adapted from beautiful-ui PromptBar, copyright (c) 2026 Shane Levine.
 * Glimm: Copyright (c) 2026 Noman Ijaz - iamnoman.com
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import { createShader, playSweep, accentChain, ACCENTS } from "glimm";
import type { UiDestination } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./prompt-bar.css";

export type PromptChoice = Pick<UiDestination, "value" | "label" | "enabled" | "supports_plan">;
type CodexMode = "configured" | "plan";
const codexModes: readonly CodexMode[] = ["configured", "plan"];

type Props = {
  draft: string;
  onDraftChange: (draft: string) => void;
  choices: readonly PromptChoice[];
  selection: PromptChoice;
  onSelect: (choice: PromptChoice) => void;
  codexMode: CodexMode;
  onCodexMode: (mode: CodexMode) => void;
  onSend: () => void;
  onStop: () => void;
  onCapture: () => void;
  disabled: boolean;
  running: boolean;
  captureDisabled: boolean;
};

const RAINBOW = accentChain([
  ACCENTS.red, ACCENTS.orange, ACCENTS.yellow, ACCENTS.green,
  ACCENTS.cyan, ACCENTS.blue, ACCENTS.purple,
]);

function Icon({ children, size = 15, strokeWidth = 1.8 }: { children: ReactNode; size?: number; strokeWidth?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{children}</svg>;
}

function moveFocus(event: KeyboardEvent<HTMLDivElement>) {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  if (!rows.length) return;
  event.preventDefault();
  const index = rows.findIndex(row => row === document.activeElement);
  const next = event.key === "Home" ? 0 : event.key === "End" ? rows.length - 1
    : index < 0 ? event.key === "ArrowDown" ? 0 : rows.length - 1
    : (index + (event.key === "ArrowDown" ? 1 : rows.length - 1)) % rows.length;
  rows[next]?.focus();
}

function focusMenu(event: KeyboardEvent<HTMLButtonElement>, menu: HTMLDivElement | null) {
  if (!menu || !["ArrowDown", "ArrowUp"].includes(event.key)) return;
  event.preventDefault();
  menu.showPopover({ source: event.currentTarget });
  const rows = [...menu.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  (event.key === "ArrowDown" ? rows.at(0) : rows.at(-1))?.focus();
}

export default function PromptBar({ draft, onDraftChange, choices, selection, onSelect, codexMode, onCodexMode, onSend, onStop, onCapture, disabled, running, captureDisabled }: Props) {
  const sourceId = useId();
  const modelId = useId();
  const input = useRef<HTMLTextAreaElement>(null);
  const sourceButton = useRef<HTMLButtonElement>(null);
  const sourceMenu = useRef<HTMLDivElement>(null);
  const modelMenu = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const shader = useRef<ReturnType<typeof createShader> | null>(null);
  const sweepHandle = useRef<ReturnType<typeof playSweep> | null>(null);
  const sweeping = useRef(false);
  const [sourceOpen, setSourceOpen] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [engaged, setEngaged] = useState(false);
  const [modelHovered, setModelHovered] = useState<string | null>(null);
  const token = dismissed ? null : /(^|\s)([@/])([\w-]*)$/.exec(draft);
  const commandMenu = token?.[2] === "/";
  const query = token?.[3]?.toLowerCase() ?? "";
  const pageMatches = !commandMenu && "current page".includes(query);
  const model = choices.find(choice => choice.value === selection.value) ?? { ...selection, enabled: false };
  const modelLabel = model.supports_plan && codexMode === "plan" ? "Plan mode, " + model.label : model.label;
  const canSend = !disabled && !!draft.trim() && model.enabled;

  useLayoutEffect(() => {
    if (!token || !sourceMenu.current || !sourceButton.current) return;
    if (!sourceMenu.current.matches(":popover-open")) sourceMenu.current.showPopover({ source: sourceButton.current });
  }, [draft, dismissed]);

  useEffect(() => () => {
    sweepHandle.current?.cancel();
    sweepHandle.current = null;
    shader.current?.destroy();
    shader.current = null;
  }, []);

  function celebrate() {
    if (sweeping.current || window.matchMedia("(prefers-reduced-motion: reduce)").matches || !canvas.current) return;
    shader.current?.destroy();
    const random = Math.random;
    Math.random = () => 0;
    try {
      shader.current = createShader({ canvas: canvas.current, palette: RAINBOW, direction: "ltr", bandTight: 10, swellAmount: 0.85 });
    } finally {
      Math.random = random;
    }
    const active = shader.current;
    if (!active) return;
    sweeping.current = true;
    const sweep = playSweep(active, {
      palette: RAINBOW, direction: "ltr", sweepMs: 570, outroMs: 80,
      peakAlpha: 1.3, bandTight: 10, brightness: 1.4, swellAmount: 1,
      waveSpeed: 1.8, easing: "easeOutExpo",
    });
    sweepHandle.current = sweep;
    void sweep.done.finally(() => {
      if (shader.current === active) {
        active.destroy();
        shader.current = null;
      }
      if (sweepHandle.current === sweep) sweepHandle.current = null;
      sweeping.current = false;
    });
  }

  function closeMenus() {
    sourceMenu.current?.hidePopover();
    modelMenu.current?.hidePopover();
    setDismissed(true);
  }

  function capture() {
    if (captureDisabled) return;
    onCapture();
    if (token) onDraftChange(draft.slice(0, token.index + token[1].length));
    closeMenus();
    input.current?.focus();
  }

  function send() {
    if (!canSend) return;
    closeMenus();
    onSend();
  }

  return <div data-promptbar="" data-metldr-prompt="">
    <div className="composer-anchor">
      <div ref={sourceMenu} id={sourceId} popover="auto" className="composer-menu" onKeyDown={moveFocus}
        onToggle={event => { setSourceOpen(event.newState === "open"); setEngaged(false); }}>
        <span className="composer-menu-highlight" aria-hidden="true" style={{ opacity: engaged && pageMatches ? 1 : 0 }} />
        <div data-prompt-menu-rows="">
          {pageMatches && <button id="bridge-capture" type="button" data-active="true" disabled={captureDisabled}
            onMouseEnter={() => setEngaged(true)} onMouseLeave={() => setEngaged(false)} onFocus={() => setEngaged(true)} onClick={capture}>
            <Icon><g><circle cx="12" cy="12" r="10" /><path d="M2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" /></g></Icon>
            <span className="composer-row-name">Current page</span>
            <span className="composer-row-description">Use the page you are viewing</span>
          </button>}
          {!commandMenu && !query && <button type="button" disabled>
            <Icon><path d="m21.4 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" /></Icon>
            <span className="composer-row-name">Photos and files</span><span className="composer-row-description">Unavailable</span>
          </button>}
          {(commandMenu || !pageMatches) && <p className="composer-empty">{commandMenu ? "Commands are unavailable." : "No matching sources."}</p>}
        </div>
        <div data-prompt-menu-footer="">{commandMenu ? "Saved commands" : "Type to search sources and files"}</div>
      </div>

      <div ref={modelMenu} id={modelId} popover="auto" className="composer-model-menu" onKeyDown={moveFocus}
        onMouseLeave={() => setModelHovered(null)} onToggle={event => { setModelOpen(event.newState === "open"); setModelHovered(null); }}>
        <span className="composer-model-highlight" aria-hidden="true" style={{ opacity: modelHovered !== null ? 1 : 0 }} />
        <div data-prompt-model-rows="">{choices.map(choice => <button key={choice.value} type="button" disabled={!choice.enabled}
          data-sound="press"
          data-value={choice.value} data-active={modelHovered === choice.value} aria-pressed={selection.value === choice.value} title={choice.label}
          onMouseEnter={() => setModelHovered(choice.value)} onFocus={() => setModelHovered(choice.value)} onClick={() => {
            onSelect(choice);
            closeMenus();
            if (choice.value !== selection.value) celebrate();
            input.current?.focus();
          }}>
          <span className="composer-model-name">{choice.label}</span>
          {!choice.enabled && <span className="composer-model-tag">Unavailable</span>}
          <span className="composer-model-check" style={{ visibility: choice.value === selection.value ? "visible" : "hidden" }}>
            <Icon size={13} strokeWidth={2.5}><path d="M20 6L9 17l-5-5" /></Icon>
          </span>
        </button>)}</div>
        {model.supports_plan && <div data-prompt-model-rows="" role="group" aria-label="Codex mode">
          {codexModes.map(mode => <button key={mode} type="button" data-codex-mode={mode}
            data-sound="press" data-active={modelHovered === mode} aria-pressed={codexMode === mode} disabled={disabled}
            onMouseEnter={() => setModelHovered(mode)} onFocus={() => setModelHovered(mode)} onClick={() => {
              onCodexMode(mode);
              closeMenus();
              input.current?.focus();
            }}>
            <span className="composer-model-name">{mode === "plan" ? "Codex Plan mode" : "Codex configured mode"}</span>
            <span className="composer-model-check" style={{ visibility: codexMode === mode ? "visible" : "hidden" }}>
              <Icon size={13} strokeWidth={2.5}><path d="M20 6L9 17l-5-5" /></Icon>
            </span>
          </button>)}
        </div>}
      </div>

      <div data-prompt-frame="">
        <canvas ref={canvas} aria-hidden="true" className="composer-glimm" />
        <div data-prompt-controls="">
          <button id="bridge-add" ref={sourceButton} type="button" data-prompt-add="" aria-label="Add attachments and sources"
            popoverTarget={sourceId} aria-expanded={sourceOpen} onKeyDown={event => focusMenu(event, sourceMenu.current)}
            onClick={() => { setDismissed(true); modelMenu.current?.hidePopover(); }}>
            <Icon size={16} strokeWidth={2}><path d="M12 5v14M5 12h14" /></Icon>
          </button>
          <textarea ref={input} id="bridge-question" data-prompt-field="" rows={1} value={draft} aria-label="Prompt" placeholder="Write a message…"
            onChange={event => {
              onDraftChange(event.target.value);
              setDismissed(false);
              if (!/(^|\s)[@/][\w-]*$/.test(event.target.value)) sourceMenu.current?.hidePopover();
            }}
            onKeyDown={event => {
              if (event.nativeEvent.isComposing) return;
              const menuOpen = sourceMenu.current?.matches(":popover-open") === true;
              if (menuOpen && pageMatches && ["ArrowDown", "ArrowUp"].includes(event.key)) {
                event.preventDefault(); setEngaged(true); return;
              }
              if (menuOpen && pageMatches && !captureDisabled && ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab")) {
                event.preventDefault(); capture(); return;
              }
              if (event.key === "Escape") { closeMenus(); return; }
              if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); send(); }
            }} />
          <button id="bridge-model" type="button" data-prompt-model="" popoverTarget={modelId} aria-expanded={modelOpen}
            onKeyDown={event => focusMenu(event, modelMenu.current)}
            aria-label="Choose model" title={modelLabel} onClick={() => { setDismissed(true); sourceMenu.current?.hidePopover(); }}>
            <span data-prompt-model-label="">{modelLabel}</span>
            <Icon size={11} strokeWidth={2.4}><path d="M6 9l6 6 6-6" /></Icon>
          </button>
          <button type="button" data-prompt-dictate="" aria-label="Dictation unavailable" title="Dictation is unavailable" disabled>
            <Icon size={15} strokeWidth={2}><g><path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z" /><path d="M19 10v2a7 7 0 0 1-14 0v-2M12 19v3" /></g></Icon>
          </button>
          <button id="bridge-send" type="button" data-prompt-send="" aria-label="Send" disabled={!canSend} hidden={running} onClick={send}>
            <Icon size={16} strokeWidth={2.4}><path d="M12 19V5M5 12l7-7 7 7" /></Icon>
          </button>
          <button id="bridge-stop" type="button" data-prompt-stop="" aria-label="Stop" hidden={!running} onClick={onStop}>
            <Icon size={16}><rect x="6" y="6" width="12" height="12" rx="2" /></Icon>
          </button>
        </div>
      </div>
    </div>
  </div>;
}
