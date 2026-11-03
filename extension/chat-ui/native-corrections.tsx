import { useEffect, useRef, useState } from "react";
import type { UiBridge, UiCorrectionReceipt, UiNativeCorrectionSnapshot } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./native-corrections.css";

export type CorrectionReceipt = Pick<UiCorrectionReceipt, "run_id" | "request_id" | "text" | "truncated" | "status" | "label" | "detail">;
type View = Pick<UiNativeCorrectionSnapshot, "connected" | "stale" | "available" | "pending" | "uncertain" | "run_id" | "admitted_request_id"> & { receipts: CorrectionReceipt[] };

function copy(snapshot: UiNativeCorrectionSnapshot): View {
  try {
    const receipts = snapshot.receipts();
    try {
      return {
        connected: snapshot.connected, stale: snapshot.stale, available: snapshot.available,
        pending: snapshot.pending, uncertain: snapshot.uncertain, run_id: snapshot.run_id,
        admitted_request_id: snapshot.admitted_request_id,
        receipts: receipts.map(receipt => ({ run_id: receipt.run_id, request_id: receipt.request_id,
          text: receipt.text, truncated: receipt.truncated, status: receipt.status,
          label: receipt.label, detail: receipt.detail })),
      };
    } finally { receipts.forEach(receipt => receipt.free()); }
  } finally { snapshot.free(); }
}

export function readCorrectionReceipts(owner: Awaited<ReturnType<UiBridge["run_details"]>>): CorrectionReceipt[] {
  const receipts = owner.corrections();
  try {
    return receipts.map(receipt => ({ run_id: receipt.run_id, request_id: receipt.request_id,
      text: receipt.text, truncated: receipt.truncated, status: receipt.status,
      label: receipt.label, detail: receipt.detail }));
  } finally { receipts.forEach(receipt => receipt.free()); }
}

export function NativeCorrectionReceipts({ receipts }: { receipts: readonly CorrectionReceipt[] }) {
  if (!receipts.length) return null;
  return <section aria-label="Native corrections" data-native-corrections="">
    {receipts.map(receipt => <article key={`${receipt.run_id}:${receipt.request_id}`} data-native-correction-id={receipt.request_id} data-native-correction-run={receipt.run_id} data-correction-status={receipt.status}>
      <p role="status">{receipt.label}{receipt.truncated ? " (text truncated)" : ""}</p>
      <p className="native-correction-text">{receipt.text}</p>
    </article>)}
  </section>;
}

export function useNativeCorrections(bridge: UiBridge) {
  const ownerRef = useRef<ReturnType<UiBridge["native_corrections"]> | undefined>(undefined);
  const currentView = useRef<View | undefined>(undefined);
  const [view, setView] = useState<View>();
  const [error, setError] = useState("");
  useEffect(() => {
    let owner: ReturnType<UiBridge["native_corrections"]> | undefined;
    let subscription: ReturnType<ReturnType<UiBridge["native_corrections"]>["subscribe"]> | undefined;
    currentView.current = undefined;
    setView(undefined); setError("");
    try {
      owner = bridge.native_corrections();
      ownerRef.current = owner;
      const activeOwner = owner;
      subscription = owner.subscribe(snapshot => {
        const next = copy(snapshot);
        if (ownerRef.current === activeOwner) { currentView.current = next; setView(next); }
      });
    } catch (cause) { setError(String(cause)); }
    return () => {
      ownerRef.current = undefined;
      currentView.current = undefined;
      subscription?.close(); subscription?.free(); owner?.free();
    };
  }, [bridge]);
  return { view, error, owner: () => ownerRef.current, current: () => currentView.current };
}
