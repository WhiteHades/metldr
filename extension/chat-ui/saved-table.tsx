import { useEffect, useState } from "react";
import { z } from "zod";
import { DataTable, TableProps } from "./data-table";

export const TableReference = z.object({
  runId: z.string().min(1).max(128),
  artifactId: z.string().min(1).max(128),
}).strict();

type Reference = z.infer<typeof TableReference>;
type Reader = (reference: Reference) => Promise<string>;
type Result =
  | { kind: "ready"; table: z.infer<typeof TableProps> }
  | { kind: "failed"; message: string };

export function SavedTable({ reference, read }: { reference: Reference; read: Reader }) {
  const [attempt, setAttempt] = useState(0);
  const [saved, setSaved] = useState<{
    reference: Reference;
    read: Reader;
    attempt: number;
    result: Result;
  }>();
  const { runId, artifactId } = reference;

  useEffect(() => {
    let active = true;
    const reference = { runId, artifactId };
    void Promise.resolve().then(() => read(reference)).then(encoded => {
      if (!active) return;
      const data: unknown = JSON.parse(encoded);
      const table = TableProps.parse(data);
      if (active) setSaved({ reference, read, attempt, result: { kind: "ready", table } });
    }).catch(() => {
      if (active) setSaved({
        reference, read, attempt,
        result: { kind: "failed", message: "The saved table could not be loaded." },
      });
    });
    return () => { active = false; };
  }, [runId, artifactId, read, attempt]);

  const current = saved?.reference.runId === runId && saved.reference.artifactId === artifactId
    && saved.read === read && saved.attempt === attempt ? saved.result : undefined;
  if (!current) return <p role="status">Loading table</p>;
  switch (current.kind) {
    case "ready": return <DataTable props={current.table} />;
    case "failed": return <div>
      <p role="alert">{current.message}</p>
      <button type="button" onClick={() => setAttempt(value => value + 1)}>Retry loading table</button>
    </div>;
  }
}
