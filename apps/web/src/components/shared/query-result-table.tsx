import { useState } from "react";

export type QueryCell =
  | { type: "null" }
  | { type: "text"; value: string }
  | { type: "decimal"; value: string }
  | { type: "int8"; value: string };

export function QueryResultTable({
  columns,
  rows,
  queryId,
  truncated,
  warnings,
  receipt,
  state = "ready",
}: {
  columns: readonly string[];
  rows: readonly { id: string; cells: readonly QueryCell[] }[];
  queryId: string | null;
  truncated: boolean;
  warnings: readonly string[];
  receipt: string | null;
  state?: "ready" | "loading" | "error" | "forbidden" | "empty";
}) {
  const [copied, setCopied] = useState("");
  if (state === "forbidden") return <p>当前身份没有执行此操作的权限。</p>;
  if (state === "error") return <p>结果没有读到。</p>;
  if (state === "loading") return <p aria-busy="true">正在读取结果。</p>;
  if (state === "empty") return <p>没有可显示的记录。</p>;
  return (
    <div className="oo-stack">
      <p className="oo-code">{queryId ? `query_id ${queryId}` : "没有 query_id"}</p>
      {receipt ? <p className="oo-code">receipt {receipt}</p> : <p>没有 receipt，不能标成已完成。</p>}
      {truncated ? <p>这是部分结果。</p> : null}
      {warnings.map((warning) => (
        <p key={warning}>{warning}</p>
      ))}
      <div className="oo-table-wrap">
        <table className="oo-table">
          <thead>
            <tr>
              {columns.map((column) => (
                <th key={column}>{column}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.id}>
                {row.cells.map((cell, index) => (
                  <td key={`${row.id}-${index}`}>
                    <CellValue cell={cell} onCopy={setCopied} />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {copied ? <p className="oo-live" aria-live="polite">{`已复制 ${copied}`}</p> : null}
    </div>
  );
}

function CellValue({ cell, onCopy }: { cell: QueryCell; onCopy: (value: string) => void }) {
  if (cell.type === "null") {
    return <span className="oo-null">NULL</span>;
  }
  const label = cell.type === "text" && cell.value === "" ? "空字符串" : cell.value;
  return (
    <span className="oo-exact">
      {cell.type === "text" && cell.value === "" ? <span aria-label="空字符串" /> : cell.value}
      <button type="button" className="oo-button oo-button-ghost oo-button-sm" onClick={() => onCopy(cell.value)}>
        复制{label === "空字符串" ? "空字符串" : ""}
      </button>
    </span>
  );
}
