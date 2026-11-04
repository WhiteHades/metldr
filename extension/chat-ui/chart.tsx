import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  Bar as RechartsBar, BarChart as RechartsBarChart, XAxis, YAxis, Tooltip,
  ResponsiveContainer, Pie as RechartsPie, PieChart as RechartsPieChart,
} from "recharts";
import { z } from "zod";
import type { UiBridge } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./chart.css";

export const ChartReference = z.object({ runId: z.string().min(1).max(128), artifactId: z.string().min(1).max(128) }).strict();
const ChartData = z.object({ label: z.string(), value: z.number().finite() }).strict();
const Chart = z.object({
  kind: z.enum(["bar", "pie"]), title: z.string(), description: z.string(), data: z.array(ChartData),
}).strict().refine(chart => chart.kind !== "pie" || chart.data.every(({ value }) => value >= 0));
const ChartResult = z.object({ chart: Chart, source: z.string().min(1).max(80) }).strict();
const Export = z.object({ filename: z.string().min(1), html: z.string() }).strict();
type ChartValue = z.infer<typeof Chart>;
type Reference = z.infer<typeof ChartReference>;
type Reader = (reference: Reference) => Promise<string>;
type ExportValue = z.infer<typeof Export>;
type Exporter = (reference: Reference) => Promise<ExportValue>;
type SavedResult = { kind: "ready"; chart: ChartValue; source: string } | { kind: "failed" };

const colors = ["#3b82f6", "#8b5cf6", "#ec4899", "#f59e0b", "#10b981", "#06b6d4", "#f97316"];
const subscribeMotion = (notify: () => void) => {
  const query = matchMedia("(prefers-reduced-motion: reduce)");
  query.addEventListener("change", notify);
  return () => query.removeEventListener("change", notify);
};
const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
const serverMotion = () => false;

export function SavedChart({ reference, read, exportHtml }: { reference: Reference; read: Reader; exportHtml: Exporter }) {
  const [attempt, setAttempt] = useState(0);
  const [saved, setSaved] = useState<{ reference: Reference; read: Reader; attempt: number; result: SavedResult }>();
  const { runId, artifactId } = reference;
  useEffect(() => {
    let live = true;
    const current = { runId, artifactId };
    void Promise.resolve().then(() => read(current)).then(encoded => {
      if (!live) return;
      const parsed = ChartResult.parse(JSON.parse(encoded));
      if (live) setSaved({ reference: current, read, attempt, result: { kind: "ready", chart: parsed.chart, source: parsed.source } });
    }).catch(() => {
      if (live) setSaved({ reference: current, read, attempt, result: { kind: "failed" } });
    });
    return () => { live = false; };
  }, [runId, artifactId, read, attempt]);
  const current = saved?.reference.runId === runId && saved.reference.artifactId === artifactId && saved.read === read && saved.attempt === attempt ? saved.result : undefined;
  if (!current) return <p role="status">Loading chart</p>;
  if (current.kind === "failed") return <><p role="alert">The saved chart could not be loaded.</p><button className="metldr-chart-retry" type="button" onClick={() => setAttempt(value => value + 1)}>Retry loading chart</button></>;
  return <ChartView key={`${runId}:${artifactId}:${attempt}`} chart={current.chart} source={current.source} reference={reference} exportHtml={exportHtml} />;
}

function ChartView({ chart, source, reference, exportHtml }: { chart: ChartValue; source: string; reference: Reference; exportHtml: Exporter }) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState("");
  const menu = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const lifetime = useRef<object | null>(null);
  const animate = !useSyncExternalStore(subscribeMotion, reducedMotion, serverMotion);
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
      <div className="metldr-chart-plot">
        {chart.data.length === 0 ? <p className="metldr-chart-empty">No data available</p> : chart.kind === "bar" ? <ResponsiveContainer width="100%" height={300}>
          <RechartsBarChart data={chart.data.map((entry, index) => ({ ...entry, fill: colors[index % colors.length] }))} margin={{ top: 5, right: 20, bottom: 5, left: 0 }}>
            <XAxis dataKey="label" tick={{ fontSize: 12 }} stroke="var(--ink-2)" /><YAxis tick={{ fontSize: 12 }} stroke="var(--ink-2)" />
            <Tooltip itemStyle={{ color: "var(--ink)" }} labelStyle={{ color: "var(--ink)" }} contentStyle={{ backgroundColor: "var(--surface)", border: "1px solid var(--line)", borderRadius: 8, paddingBlock: 8, paddingInline: 12, color: "var(--ink)" }} />
            <RechartsBar dataKey="value" radius={[4, 4, 0, 0]} isAnimationActive={animate} animationDuration={800} animationEasing="ease-out" />
          </RechartsBarChart>
        </ResponsiveContainer> : <>
          <ResponsiveContainer width="100%" height={300}><RechartsPieChart>
            <RechartsPie data={chart.data.map((entry, index) => ({ ...entry, fill: colors[index % colors.length] }))} dataKey="value" nameKey="label" cx="50%" cy="50%" outerRadius={100} isAnimationActive={animate} animationDuration={800} animationEasing="ease-out" animationBegin={200} />
            <Tooltip itemStyle={{ color: "var(--ink)" }} labelStyle={{ color: "var(--ink)" }} contentStyle={{ backgroundColor: "var(--surface)", border: "1px solid var(--line)", borderRadius: 8, paddingBlock: 8, paddingInline: 12, color: "var(--ink)" }} />
          </RechartsPieChart></ResponsiveContainer>
          <div className="metldr-chart-legend">{chart.data.map((item, index) => <div key={index}><span style={{ backgroundColor: colors[index % colors.length] }} /><span>{item.label}</span></div>)}</div>
        </>}
      </div>
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
