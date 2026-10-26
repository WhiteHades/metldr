"use client";

import { useEffect } from "react";
import { defineSound, dispose, ensureReady, setMasterVolume, type PlayOptions, type VoiceHandle } from "@web-kits/audio";

const INTERACTIVE_SELECTOR = [
  "button",
  "a[href]",
  "input:not([type='hidden'])",
  "select",
  "textarea",
  "summary",
  "[role='button']",
  "[role='checkbox']",
  "[role='menuitem']",
  "[role='menuitemcheckbox']",
  "[role='menuitemradio']",
  "[role='option']",
  "[role='radio']",
  "[role='switch']",
  "[role='tab']",
].join(",");

const DISMISS_WORDS = /close|dismiss|remove|delete|collapse|cancel|clear/i;
const PRIMARY_WORDS = /send|save|submit|calculate|create|add|upgrade|replay/i;

type Cue = "press" | "tick" | "release" | "page" | "pulse";

/* Five short interaction cues, synthesized with @web-kits/audio. Built lazily
 * on the first user gesture so no AudioContext is created before it's allowed. */
function buildCues(): Record<Cue, (opts?: PlayOptions) => VoiceHandle> {
  return {
    // most buttons — a crisp transient (D) blended with a bright pluck (C)
    press: defineSound({ layers: [
      { source: { type: "noise", color: "white" }, filter: { type: "highpass", frequency: 2400 }, envelope: { attack: 0.0003, decay: 0.009, sustain: 0 }, gain: 0.13 },
      { source: { type: "sine", frequency: 680 }, envelope: { attack: 0.0006, decay: 0.022, sustain: 0, release: 0.008 }, gain: 0.24 },
    ] }),
    // toggles / tabs / inputs — a light, crisp tick
    tick: defineSound({
      source: { type: "square", frequency: 2100 },
      filter: { type: "bandpass", frequency: 2600, resonance: 1.6 },
      envelope: { attack: 0.0004, decay: 0.028, sustain: 0 },
      gain: 0.24,
    }),
    // close / delete / cancel — a light clack
    release: defineSound({
      source: { type: "noise", color: "white" },
      filter: { type: "lowpass", frequency: 1600, resonance: 0.9 },
      envelope: { attack: 0.001, decay: 0.055, sustain: 0 },
      gain: 0.32,
    }),
    // links / navigation — a soft upward blip
    page: defineSound({
      source: { type: "sine", frequency: { start: 430, end: 640 } },
      envelope: { attack: 0.002, decay: 0.11, sustain: 0, release: 0.03 },
      gain: 0.4,
    }),
    // primary actions — a rounder pulse with a little body
    pulse: defineSound({
      source: { type: "sine", frequency: 330 },
      filter: { type: "lowpass", frequency: 2200 },
      envelope: { attack: 0.002, decay: 0.13, sustain: 0, release: 0.04 },
      gain: 0.5,
    }),
  };
}

function cueFor(element: Element): Cue {
  const override = element.getAttribute("data-sound");
  if (override === "press" || override === "tick" || override === "release" || override === "page" || override === "pulse") {
    return override;
  }

  const label = `${element.getAttribute("aria-label") ?? ""} ${element.textContent ?? ""}`.trim();
  if (DISMISS_WORDS.test(label)) return "release";
  if (element.matches("input[type='checkbox'], input[type='radio'], select, [role='checkbox'], [role='radio'], [role='switch'], [role='tab'], [aria-pressed]")) return "tick";
  if (element.matches("a[href]")) return "page";
  if (PRIMARY_WORDS.test(label)) return "pulse";
  if (element.matches("input, textarea")) return "tick";
  return "press";
}

/** One quiet, delegated audio layer for interactive clicks across every route. */
export function InteractionSounds({ enabled = true }: { enabled?: boolean }) {
  useEffect(() => {
    let cues: Record<Cue, (opts?: PlayOptions) => VoiceHandle> | null = null;

    const handleClick = (event: MouseEvent) => {
      if (!enabled) return;
      if (!(event.target instanceof Element)) return;
      const control = event.target.closest(INTERACTIVE_SELECTOR);
      if (!control || control.closest("[data-sound-silent]")) return;
      if (control.matches(":disabled, [aria-disabled='true']")) return;

      // first real gesture: unlock the context and build the cues
      if (!cues) {
        void ensureReady();
        setMasterVolume(0.32);
        cues = buildCues();
      }
      cues[cueFor(control)]();
    };

    document.addEventListener("click", handleClick, true);
    return () => { document.removeEventListener("click", handleClick, true); if (cues) dispose(); };
  }, [enabled]);

  return null;
}
