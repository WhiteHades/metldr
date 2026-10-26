import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { UiBridge, UiRunReport } from "../../crates/metldr-panel/pkg/metldr_panel.js";

type Read = (reference: string) => ReturnType<UiBridge["run_details"]>;
type Snapshot = Awaited<ReturnType<Read>>;
type Report = Pick<UiRunReport, "tool" | "phase" | "outcome" | "call_id">;
type Display = { owner: Read; run: string; route: string; status: string; reportCount: number; reports: Report[] };

function read(snapshot: Snapshot, owner: Read): Display {
  const run = snapshot.run_id;
  const route = snapshot.route;
  const status = snapshot.status;
  const reportCount = snapshot.report_count;
  const reports: Report[] = [];
  let offset: number | undefined = 0;
  while (offset !== undefined) {
    const page = snapshot.page(offset);
    try {
      const entries = page.reports();
      try {
        for (const entry of entries) {
          const outcome = entry.outcome;
          const callId = entry.call_id;
          reports.push({
            tool: entry.tool,
            phase: entry.phase,
            outcome,
            call_id: callId,
          });
        }
      } finally { entries.forEach(entry => entry.free()); }
      offset = page.next_offset;
    } finally { page.free(); }
  }
  return { owner, run, route, status, reportCount, reports };
}

export function RunDetails({ reference, read: load }: { reference: unknown; read: Read }) {
  const encoded = JSON.stringify(reference) ?? "null";
  const [display, setDisplay] = useState<Display>();
  const [error, setError] = useState<{ scope: string; message: string }>();
  const [busy, setBusy] = useState(true);
  const [retry, setRetry] = useState(0);
  const state = useRef({ alive: true, reading: false, serial: 0, wanted: { encoded, load, serial: 0 } });
  useLayoutEffect(() => {
    state.current.alive = true;
    return () => { state.current.alive = false; };
  }, []);
  useEffect(() => {
    const current = state.current;
    current.wanted = { encoded, load, serial: ++current.serial };
    if (current.reading) return;
    current.reading = true;
    void (async () => {
      try {
        while (current.alive) {
          const wanted = current.wanted;
          setBusy(true);
          setError(undefined);
          let snapshot: Snapshot | undefined;
          try {
            snapshot = await wanted.load(wanted.encoded);
            const next = read(snapshot, wanted.load);
            if (current.alive && current.wanted.serial === wanted.serial) {
              setDisplay(next);
              setBusy(false);
            }
          } catch (error) {
            if (current.alive && current.wanted.serial === wanted.serial) {
              setError({ scope: wanted.encoded, message: String(error) });
              setBusy(false);
            }
          } finally { snapshot?.free(); }
          if (current.wanted.serial === wanted.serial) break;
        }
      } finally { current.reading = false; }
    })();
  }, [encoded, load, retry]);

  const visible = display?.owner === load ? display : undefined;
  return <section data-run-details={visible?.run ?? ""} aria-label="Run details" aria-busy={busy}>
    {visible && <>
      <p data-part="route">{visible.route}</p>
      <p data-part="state">{visible.status}</p>
    </>}
    <details data-run-reports>
      <summary>{visible ? `${visible.reportCount} retained ${visible.reportCount === 1 ? "report" : "reports"}` : busy ? "Loading retained reports" : "Reports unavailable"}</summary>
      <div>
        {visible?.reports.map((report, index) => <div data-run-report="" data-call-id={report.call_id} key={`${index}:${report.call_id ?? ""}`}>
          <span data-part="tool">{report.tool}</span>
          <span data-part="phase">{report.phase}</span>
          {report.outcome !== undefined && <span data-part="outcome">{report.outcome}</span>}
        </div>)}
      </div>
    </details>
    {busy && !visible && <p role="status">Loading run details</p>}
    {error?.scope === encoded && <div className="chat-toolbar"><p role="alert">Run details could not be loaded: {error.message}</p><button type="button" onClick={() => setRetry(value => value + 1)}>Try again</button></div>}
  </section>;
}
