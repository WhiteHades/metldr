import { useEffect, useRef, useState } from "react";
import { UiSourceStatus, type UiBridge, type UiSavedSource, type UiSourceSnapshot } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./saved-source.css";

type Metadata = Pick<UiSourceSnapshot, "source_id" | "title" | "url" | "status">;
type Display = Metadata & { owner: UiBridge };
type Text = { owner: UiBridge; id: string; text: string };

function statusLabel(status: UiSourceStatus) {
  switch (status) {
    case UiSourceStatus.Empty: return "No saved page";
    case UiSourceStatus.Checking: return "Checking page";
    case UiSourceStatus.Current: return "Matches current page";
    case UiSourceStatus.Changed: return "Page has changed";
    case UiSourceStatus.Saved: return "Saved capture";
    case UiSourceStatus.Unavailable: return "Page check unavailable";
    default: { const unknown: never = status; throw new Error(`Unknown source status: ${unknown}`); }
  }
}

export function SavedSource({ bridge }: { bridge: UiBridge }) {
  const disclosure = useRef<HTMLDetailsElement>(null);
  const source = useRef<UiSavedSource | undefined>(undefined);
  const cache = useRef<Text | undefined>(undefined);
  const [display, setDisplay] = useState<Display>();
  const [detail, setDetail] = useState<Text>();
  const [error, setError] = useState("");
  const [refreshing, setRefreshing] = useState(false);

  function accept(snapshot: UiSourceSnapshot) {
    try {
      const next: Display = { owner: bridge, source_id: snapshot.source_id, title: snapshot.title,
        url: snapshot.url, status: snapshot.status };
      setDisplay(next);
      if (cache.current?.owner !== bridge || cache.current.id !== next.source_id) {
        cache.current = undefined;
        setDetail(undefined);
      }
      if (disclosure.current?.open && next.source_id && !cache.current) {
        const text = snapshot.text();
        if (text !== undefined) {
          cache.current = { owner: bridge, id: next.source_id, text };
          setDetail(cache.current);
        }
      }
      setError("");
    } catch (error) { setError(String(error)); }
    finally { snapshot.free(); }
  }

  useEffect(() => {
    let owner: UiSavedSource | undefined;
    let subscription: ReturnType<UiSavedSource["subscribe"]> | undefined;
    cache.current = undefined;
    setDetail(undefined);
    setError("");
    setRefreshing(false);
    try {
      owner = bridge.saved_source();
      source.current = owner;
      subscription = owner.subscribe(accept);
    } catch (error) { setError(String(error)); }
    return () => {
      source.current = undefined;
      subscription?.close();
      subscription?.free();
      owner?.free();
    };
  }, [bridge]);

  function open() {
    if (!disclosure.current?.open) {
      cache.current = undefined;
      setDetail(undefined);
      return;
    }
    if (!source.current) return;
    try { accept(source.current.snapshot()); }
    catch (error) { setError(String(error)); }
  }

  function refresh() {
    const owner = source.current;
    if (!owner || refreshing) return;
    setRefreshing(true);
    setError("");
    try {
      void owner.refresh().then(() => {
        if (source.current === owner) accept(owner.snapshot());
      }).catch(error => {
        if (source.current === owner) setError(String(error));
      }).finally(() => {
        if (source.current === owner) setRefreshing(false);
      });
    } catch (error) { setError(String(error)); setRefreshing(false); }
  }

  const visible = display?.owner === bridge ? display : undefined;
  const text = detail?.owner === bridge && detail.id === visible?.source_id ? detail.text : undefined;
  return <details className="saved-source" data-saved-source={visible?.source_id ?? ""} ref={disclosure} onToggle={open}>
    <summary>
      <span>Saved page</span>
      <span data-source-status="" role="status">{visible ? statusLabel(visible.status) : "Loading saved page"}</span>
      <svg aria-hidden="true" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6" /></svg>
    </summary>
    <div className="saved-source__body">
      {visible?.source_id ? <>
        <h2 data-source-title="">{visible.title || "Untitled page"}</h2>
        <p data-source-url="">{visible.url}</p>
        <p>Captured text</p>
        <p className="saved-source__text" data-source-text="">{text ?? "Loading captured text"}</p>
      </> : <p>Capture a page to use it in this conversation.</p>}
      {error && <p role="alert">The saved page could not be read. {error}</p>}
      <button type="button" data-source-refresh="" onClick={refresh} disabled={refreshing || !source.current}>
        {refreshing ? "Checking page" : "Check page"}
      </button>
    </div>
  </details>;
}
