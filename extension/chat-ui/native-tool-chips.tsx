import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { UiBridge, UiNativeTool, UiToolPreview } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import ToolChips, { type ToolStep } from "./tool-chips";

type Read = (reference: string) => ReturnType<UiBridge["native_tools"]>;
type Display = { steps: ToolStep[]; reports: number; run: string; group: string };

function preview(value: UiToolPreview | undefined): string | undefined {
  if (!value) return;
  try { return value.text + (value.truncated ? "\nPreview incomplete." : ""); }
  finally { value.free(); }
}

function step(tool: UiNativeTool, status: string): ToolStep {
  const name = tool.name;
  const title = preview(tool.title);
  const input = preview(tool.input);
  const output = preview(tool.output);
  const phase = tool.outcome ?? tool.phase;
  const detail = [{ text: name === undefined ? "Details unavailable." : `Tool: ${name}` }];
  if (phase) detail.push({text: `State: ${phase}`});
  if (status !== "running" && status !== "stop_requested") detail.push({text: `Run: ${status}`});
  if (input !== undefined) detail.push({text: `Input\n${input}`});
  if (output !== undefined) detail.push({text: `Output\n${output}`});
  const history = tool.history();
  try {
    for (const [index, report] of history.entries()) {
      const outcome = report.outcome;
      detail.push({text: `Report ${index + 1}: ${report.phase}${outcome ? `, ${outcome}` : ""}`});
    }
  } finally { history.forEach(report => report.free()); }
  detail.push({text: `${tool.reports} retained reports`});
  return {
    id: tool.id,
    icon: name === "read" ? "read" : name === "write" || name === "edit" ? "write" : "run",
    label: name ?? "Tool",
    chip: title || phase || "Details unavailable",
    mono: false,
    detailMono: true,
    detail,
  };
}

export function NativeToolChips({ reference, read }: { reference: unknown; read: Read }) {
  const encoded = JSON.stringify(reference) ?? "null";
  const [display, setDisplay] = useState<Display>({steps: [], reports: 0, run: "", group: ""});
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(true);
  const [retry, setRetry] = useState(0);
  const state = useRef({alive: true, reading: false, serial: 0, wanted: {encoded, read, serial: 0}});
  useLayoutEffect(() => {
    state.current.alive = true;
    return () => { state.current.alive = false; };
  }, []);
  useEffect(() => {
    const stateValue = state.current;
    stateValue.wanted = {encoded, read, serial: ++stateValue.serial};
    if (stateValue.reading) return;
    stateValue.reading = true;
    void (async () => {
      try {
        while (stateValue.alive) {
          const wanted = stateValue.wanted;
          setBusy(true);
          setError("");
          let snapshot: Awaited<ReturnType<Read>> | undefined;
          try {
            snapshot = await wanted.read(wanted.encoded);
            const runStatus = snapshot.run_status;
            const steps: ToolStep[] = [];
            let offset: number | undefined = 0;
            while (offset !== undefined) {
              const page = snapshot.page(offset);
              try {
                const tools = page.tools();
                try { steps.push(...tools.map(tool => step(tool, runStatus))); }
                finally { tools.forEach(tool => tool.free()); }
                offset = page.next_offset;
              } finally { page.free(); }
            }
            if (stateValue.alive && stateValue.wanted.serial === wanted.serial) {
              setDisplay({steps, reports: snapshot.report_count, run: snapshot.run_id, group: snapshot.group_id});
              setBusy(false);
            }
          } catch (error) {
            if (stateValue.alive && stateValue.wanted.serial === wanted.serial) {
              setError(String(error));
              setBusy(false);
            }
          } finally { snapshot?.free(); }
          if (stateValue.wanted.serial === wanted.serial) break;
        }
      } finally { stateValue.reading = false; }
    })();
  }, [encoded, read, retry]);
  return <section data-native-tool-group={display.group} data-native-tool-run={display.run} aria-label="Tool activity" aria-busy={busy}>
    {display.steps.length > 0 && <ToolChips steps={display.steps} labels={{header: `${display.steps.length} tool ${display.steps.length === 1 ? "call" : "calls"}, ${display.reports} retained reports`}} />}
    {busy && display.steps.length === 0 && <p role="status">Loading tool activity</p>}
    {error && <div className="chat-toolbar"><p role="alert">{error}</p><button type="button" onClick={() => setRetry(value => value + 1)}>Try again</button></div>}
  </section>;
}
