/*
The MIT License

Copyright (c) Atai Barkai

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/

import { z } from "zod";

export const TableProps = z.object({
  title: z.string().max(300),
  columns: z.array(z.string().max(200)).min(1).max(12),
  rows: z.array(z.array(z.string().max(2000)).max(12)).max(200),
  source: z
    .string()
    .max(1000)
    .describe(
      "Data provenance: user-provided, calculated, sourced, or explicitly illustrative.",
    ),
});

export function DataTable({ props }: { props: z.infer<typeof TableProps> }) {
  const parsed = TableProps.safeParse(props);
  if (
    !parsed.success ||
    parsed.data.rows.some((row) => row.length !== parsed.data.columns.length)
  ) {
    return (
      <p role="alert">
        This table has invalid rows or columns. Ask the assistant to regenerate
        it.
      </p>
    );
  }
  const { title, columns, rows, source } = parsed.data;
  return (
    <section className="answer-table" aria-label={title}>
      <div
        className="answer-table-scroll"
        role="region"
        aria-label={`${title} table`}
        tabIndex={0}
      >
        <table>
          <caption>{title}</caption>
          <thead>
            <tr>
              {columns.map((column, index) => (
                <th key={index} scope="col">
                  {column}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, index) => (
              <tr key={index}>
                {row.map((cell, column) => (
                  <td key={column}>{cell}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {rows.length === 0 && <p>No rows to display.</p>}
      </div>
      <p className="answer-table-source">{source}</p>
    </section>
  );
}
