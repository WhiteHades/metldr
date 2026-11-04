import { useEffect, useRef, useState } from "react";
import { z } from "zod";
import type { ChartPlot } from "./chart-plot";
import type { UiBridge } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./chart.css";

export const ChartReference = z.object({ runId: z.string().min(1).max(128), artifactId: z.string().min(1).max(128) }).strict();
const ChartData = z.object({ label: z.string(), value: z.number().finite() }).strict();
const Chart = z.object({
  kind: z.enum(["bar", "pie"]), title: z.string(), description: z.string(), data: z.array(ChartData),
}).strict().refine(chart => chart.kind !== "pie" || chart.data.every(({ value }) => value >= 0));
const ChartResult = z.object({ chart: Chart, source: z.string().min(1).max(80) }).strict();
const Export = z.object({ filename: z.string().min(1), html: z.string() }).strict();
export type ChartValue = z.infer<typeof Chart>;
type Reference = z.infer<typeof ChartReference>;
type Reader = (reference: Reference) => Promise<string>;
type ExportValue = z.infer<typeof Export>;
type Exporter = (reference: Reference) => Promise<ExportValue>;
type SavedResult = { kind: "ready"; chart: ChartValue; source: string; Plot: typeof ChartPlot } | { kind: "failed" };

export function SavedChart({ reference, read, exportHtml }: { reference: Reference; read: Reader; exportHtml: Exporter }) {
  const [attempt, setAttempt] = useState(0);
  const [saved, setSaved] = useState<{ reference: Reference; read: Reader; attempt: number; result: SavedResult }>();
  const { runId, artifactId } = reference;
  useEffect(() => {
    let live = true;
    const current = { runId, artifactId };
    void Promise.all([Promise.resolve().then(() => read(current)), import("./chart-plot")]).then(([encoded, { ChartPlot }]) => {
      if (!live) return;
      const parsed = ChartResult.parse(JSON.parse(encoded));
      if (live) setSaved({ reference: current, read, attempt, result: { kind: "ready", chart: parsed.chart, source: parsed.source, Plot: ChartPlot } });
    }).catch(() => {
      if (live) setSaved({ reference: current, read, attempt, result: { kind: "failed" } });
    });
    return () => { live = false; };
  }, [runId, artifactId, read, attempt]);
  const current = saved?.reference.runId === runId && saved.reference.artifactId === artifactId && saved.read === read && saved.attempt === attempt ? saved.result : undefined;
  if (!current) return <p role="status">Loading chart</p>;
  if (current.kind === "failed") return <><p role="alert">The saved chart could not be loaded.</p><button className="metldr-chart-retry" type="button" onClick={() => setAttempt(value => value + 1)}>Retry loading chart</button></>;
  return <ChartView key={`${runId}:${artifactId}:${attempt}`} chart={current.chart} source={current.source} reference={reference} exportHtml={exportHtml} Plot={current.Plot} />;
}

function ChartView({ chart, source, reference, exportHtml, Plot }: { chart: ChartValue; source: string; reference: Reference; exportHtml: Exporter; Plot: typeof ChartPlot }) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState("");
  const menu = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const lifetime = useRef<object | null>(null);
  useEffect(() => {
    lifetime.current = {};
    return () => { lifetime.current = null; };
  }, []);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Node) || !menu.current?.contains(target)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);
  async function copy() {
    const current = lifetime.current;
    if (!current) return;
    setError("");
    try {
      const result = await exportHtml(reference);
      if (lifetime.current !== current) return;
      await navigator.clipboard.writeText(result.html);
      if (lifetime.current !== current) return;
      if (menu.current?.contains(document.activeElement)) trigger.current?.focus();
      setCopied(true); setOpen(false); window.setTimeout(() => { if (lifetime.current === current) setCopied(false); }, 1800);
    } catch { if (lifetime.current === current) setError("The chart could not be copied."); }
  }
  async function download() {
    const current = lifetime.current;
    if (!current) return;
    setError("");
    try {
      const result = await exportHtml(reference);
      if (lifetime.current !== current) return;
      const url = URL.createObjectURL(new Blob([result.html], { type: "text/html" }));
      const link = document.createElement("a");
      link.href = url; link.download = result.filename; document.body.append(link); link.click(); link.remove();
      if (menu.current?.contains(document.activeElement)) trigger.current?.focus();
      URL.revokeObjectURL(url); setOpen(false);
    } catch { if (lifetime.current === current) setError("The chart could not be downloaded."); }
  }
  return <div className="metldr-chart-wrap" data-kind={chart.kind}>
    <section className="metldr-chart" aria-labelledby={`chart-${reference.artifactId}`}>
      <div className="metldr-chart-head"><h3 id={`chart-${reference.artifactId}`}>{chart.title}</h3><p>{chart.description}</p></div>
      <Plot chart={chart} />
      {chart.data.length > 0 && <details className="metldr-chart-data">
        <summary>View chart data</summary>
        <table><thead><tr><th scope="col">Label</th><th scope="col">Value</th></tr></thead><tbody>{chart.data.map((item, index) => <tr key={index}><th scope="row">{item.label}</th><td>{item.value}</td></tr>)}</tbody></table>
      </details>}
      <p className="metldr-chart-source">Source: {source}</p>
      {chart.data.length > 0 && <div className="metldr-chart-options" data-open={open} ref={menu} onKeyDown={event => { if (event.key === "Escape" && open) { setOpen(false); trigger.current?.focus(); } }}>
        <button ref={trigger} type="button" aria-label="Chart options" aria-expanded={open} onClick={() => setOpen(value => !value)}><svg viewBox="0 0 24 24" aria-hidden="true" fill="currentColor"><circle cx="5" cy="12" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="19" cy="12" r="1.5"/></svg></button>
        {open && <div className="metldr-chart-menu"><button type="button" onClick={() => void copy()}>{copied ? "Copied!" : "Copy to clipboard"}</button><button type="button" onClick={() => void download()}>Download file</button></div>}
      </div>}
      {error && <p className="metldr-chart-error" role="alert">{error}</p>}
    </section>
  </div>;
}

export function chartReader(bridge: UiBridge, enqueue: <T>(operation: () => Promise<T>) => Promise<T>): Reader {
  return reference => enqueue(() => bridge.chart(reference.runId, reference.artifactId));
}
export function chartExporter(bridge: UiBridge, enqueue: <T>(operation: () => Promise<T>) => Promise<T>): Exporter {
  return async reference => Export.parse(JSON.parse(await enqueue(() => bridge.chart_export(reference.runId, reference.artifactId))));
}
