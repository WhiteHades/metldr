import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  UiPreferenceStatus,
  type UiPreferenceSnapshot,
  type UiPreferences,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";

export type PreferenceView = Pick<
  UiPreferenceSnapshot,
  "theme" | "font_size" | "appearance_status" | "sounds_enabled" | "sound_status"
>;

function readSnapshot(snapshot: UiPreferenceSnapshot): PreferenceView {
  try {
    return {
      theme: snapshot.theme,
      font_size: snapshot.font_size,
      appearance_status: snapshot.appearance_status,
      sounds_enabled: snapshot.sounds_enabled,
      sound_status: snapshot.sound_status,
    };
  } finally {
    snapshot.free();
  }
}

export function usePreferences(preferences: UiPreferences): PreferenceView {
  const [view, setView] = useState(() => readSnapshot(preferences.snapshot()));

  useEffect(() => {
    const subscription = preferences.subscribe((snapshot) => setView(readSnapshot(snapshot)));
    return () => {
      subscription.close();
      subscription.free();
    };
  }, [preferences]);

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = view.theme;
    if (view.font_size === "medium") delete document.documentElement.dataset.fontSize;
    else document.documentElement.dataset.fontSize = view.font_size;
  }, [view.theme, view.font_size]);

  return view;
}

const themeLabels: Record<string, string> = {
  catppuccin: "Catppuccin",
  gruvbox: "Gruvbox",
  cyberpunk: "Cyberpunk",
  light: "Light",
};

const sizeLabels: Record<string, string> = {
  small: "Small",
  medium: "Medium",
  large: "Large",
};

function isBusy(status: UiPreferenceStatus) {
  return status === UiPreferenceStatus.Loading || status === UiPreferenceStatus.Saving;
}

function isFailed(status: UiPreferenceStatus) {
  return status === UiPreferenceStatus.LoadFailed || status === UiPreferenceStatus.SaveFailed;
}

function statusMessage(domain: "appearance" | "sound", status: UiPreferenceStatus) {
  switch (status) {
    case UiPreferenceStatus.Loading:
      return `Loading ${domain} preferences.`;
    case UiPreferenceStatus.Saving:
      return `Saving ${domain} preferences.`;
    case UiPreferenceStatus.Saved:
      return `${domain === "appearance" ? "Appearance" : "Sound"} preferences saved.`;
    case UiPreferenceStatus.LoadFailed:
      return domain === "appearance"
        ? "Appearance preferences could not be loaded. Choose a setting or retry."
        : "Sound preferences could not be loaded. Choose a setting or retry.";
    case UiPreferenceStatus.SaveFailed:
      return domain === "appearance"
        ? "Appearance could not be saved. Your preview remains active. Retry to save it."
        : "Sound preferences could not be saved. The current setting remains active. Retry to save it.";
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
}

export function PreferenceSettings({
  preferences,
  view,
}: {
  preferences: UiPreferences;
  view: PreferenceView;
}) {
  const themeRef = useRef<HTMLSelectElement | null>(null);
  const sizeRef = useRef<HTMLSelectElement | null>(null);
  const soundRef = useRef<HTMLInputElement | null>(null);
  const retryAppearanceRef = useRef<HTMLButtonElement | null>(null);
  const retrySoundRef = useRef<HTMLButtonElement | null>(null);
  const appearanceStatusRef = useRef<HTMLParagraphElement | null>(null);
  const soundStatusRef = useRef<HTMLParagraphElement | null>(null);
  const pendingAppearanceFocus = useRef<(() => HTMLElement | null) | null>(null);
  const lastAppearanceControl = useRef<"theme" | "size">("theme");

  function parkFocus(target: () => HTMLElement | null) {
    pendingAppearanceFocus.current = target;
    appearanceStatusRef.current?.focus({ preventScroll: true });
  }

  async function updateAppearance(operation: () => Promise<undefined>, target: () => HTMLElement | null) {
    parkFocus(target);
    try { await operation(); }
    catch { /* The Rust snapshot carries loading, save failures and the preview. */ }
  }

  useLayoutEffect(() => {
    if (!isBusy(view.appearance_status)) {
      const target = pendingAppearanceFocus.current?.();
      pendingAppearanceFocus.current = null;
      if (document.activeElement === appearanceStatusRef.current && target && !target.matches(":disabled")) target.focus();
    }
  }, [view]);

  return (
    <details data-settings-editor data-preferences-settings>
      <summary>Appearance and sound<svg aria-hidden="true" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6" /></svg></summary>
      <div data-settings-body>
        <fieldset disabled={isBusy(view.appearance_status)}>
          <legend>Appearance</legend>
          <label>
            Theme
            <select
              ref={themeRef}
              value={view.theme}
              onChange={(event) => {
                lastAppearanceControl.current = "theme";
                const value = event.currentTarget.value;
                void updateAppearance(() => preferences.set_theme(value), () => themeRef.current);
              }}
            >
              {preferences.themes().map((theme) => (
                <option key={theme} value={theme}>{themeLabels[theme] ?? theme}</option>
              ))}
            </select>
          </label>
          <label>
            Text size
            <select
              ref={sizeRef}
              value={view.font_size}
              onChange={(event) => {
                lastAppearanceControl.current = "size";
                const value = event.currentTarget.value;
                void updateAppearance(() => preferences.set_font_size(value), () => sizeRef.current);
              }}
            >
              {preferences.font_sizes().map((size) => (
                <option key={size} value={size}>{sizeLabels[size] ?? size}</option>
              ))}
            </select>
          </label>
        </fieldset>
        <p
          ref={appearanceStatusRef}
          aria-live="polite"
          role={isFailed(view.appearance_status) ? "alert" : "status"}
          tabIndex={-1}
        >
          {statusMessage("appearance", view.appearance_status)}
        </p>
        <button
          ref={retryAppearanceRef}
          type="button"
          hidden={!isFailed(view.appearance_status)}
          disabled={isBusy(view.appearance_status)}
          onClick={() => {
            void updateAppearance(() => preferences.retry_appearance(), () => lastAppearanceControl.current === "theme" ? themeRef.current : sizeRef.current);
          }}
        >
          Retry appearance settings
        </button>
        <fieldset disabled={isBusy(view.sound_status)}>
          <legend>Sound</legend>
          <label data-settings-toggle>
            <input
              ref={soundRef}
              type="checkbox"
              checked={view.sounds_enabled}
              onChange={(event) => {
                try {
                  preferences.set_sounds_enabled(event.currentTarget.checked);
                } catch { /* The Rust snapshot carries the failed save and preview. */ }
              }}
            />
            Interaction sounds
          </label>
        </fieldset>
        <p ref={soundStatusRef} aria-live="polite" role={isFailed(view.sound_status) ? "alert" : "status"} tabIndex={-1}>
          {statusMessage("sound", view.sound_status)}
        </p>
        <button
          ref={retrySoundRef}
          type="button"
          hidden={!isFailed(view.sound_status)}
          disabled={isBusy(view.sound_status)}
          onClick={() => {
            const restore = document.activeElement === retrySoundRef.current;
            try {
              preferences.retry_sound();
            } catch { /* The Rust snapshot carries the failed save and preview. */ }
            if (restore) soundRef.current?.focus({ preventScroll: true });
          }}
        >
          Retry sound settings
        </button>
      </div>
    </details>
  );
}
