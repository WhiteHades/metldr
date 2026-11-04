import { useSyncExternalStore } from "react";
import {
  Bar as RechartsBar, BarChart as RechartsBarChart, XAxis, YAxis, Tooltip,
  ResponsiveContainer, Pie as RechartsPie, PieChart as RechartsPieChart,
} from "recharts";
import type { ChartValue } from "./chart";

const colors = ["#3b82f6", "#8b5cf6", "#ec4899", "#f59e0b", "#10b981", "#06b6d4", "#f97316"];
const subscribeMotion = (notify: () => void) => {
  const query = matchMedia("(prefers-reduced-motion: reduce)");
  query.addEventListener("change", notify);
  return () => query.removeEventListener("change", notify);
};
const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
const serverMotion = () => false;

export function ChartPlot({ chart }: { chart: ChartValue }) {
  const animate = !useSyncExternalStore(subscribeMotion, reducedMotion, serverMotion);
  return <div className="metldr-chart-plot">
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
  </div>;
}
