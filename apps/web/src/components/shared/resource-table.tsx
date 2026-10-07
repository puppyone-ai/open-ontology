import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { StatePanel } from "./state-panel";

export type ResourceColumn<T> = {
  id: string;
  header: string;
  cell: (row: T) => ReactNode;
};

export type ResourceTableProps<T extends { id: string }> = {
  rows: readonly T[];
  columns: readonly ResourceColumn<T>[];
  page: number;
  pageCount: number;
  onPageChange: (page: number) => void;
  sort: string | null;
  onSortChange: (columnId: string) => void;
  filter: string;
  onFilterChange: (value: string) => void;
  count: { exact: number } | { exact: null; reason: string };
  state: "ready" | "loading" | "empty" | "noMatch" | "error" | "forbidden";
};

export function ResourceTable<T extends { id: string }>({
  rows,
  columns,
  page,
  pageCount,
  onPageChange,
  sort,
  onSortChange,
  filter,
  onFilterChange,
  count,
  state,
}: ResourceTableProps<T>) {
  const filterId = useId();
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [announcement, setAnnouncement] = useState("");
  const signature = `${page}\0${filter}\0${sort ?? ""}`;
  const seen = useRef(signature);

  useEffect(() => {
    if (seen.current === signature) {
      return;
    }
    seen.current = signature;
    setSelected(new Set());
    setAnnouncement("选择已清除");
  }, [signature]);

  const hidden = state === "forbidden" || state === "error" || state === "empty" || state === "noMatch";
  const visibleRows = hidden ? [] : rows;

  function toggle(id: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      setAnnouncement(`本页已选${next.size}项`);
      return next;
    });
  }

  function togglePage() {
    setSelected((current) => {
      const all = visibleRows.every((row) => current.has(row.id));
      const next = all ? new Set<string>() : new Set(visibleRows.map((row) => row.id));
      setAnnouncement(`本页已选${next.size}项`);
      return next;
    });
  }

  return (
    <div className="oo-stack">
      <label htmlFor={filterId}>筛选</label>
      <input
        id={filterId}
        className="oo-input"
        value={filter}
        onChange={(event) => onFilterChange(event.target.value)}
      />
      <p>{count.exact === null ? count.reason : `共 ${count.exact} 条`}</p>
      <p>本页已选{selected.size}项</p>
      <p className="oo-live" aria-live="polite">
        {announcement}
      </p>
      {state === "forbidden" ? <StatePanel kind="forbidden" /> : null}
      {state === "error" ? <StatePanel kind="error" /> : null}
      {state === "empty" ? <StatePanel kind="empty" /> : null}
      {state === "noMatch" ? <StatePanel kind="noMatch" /> : null}
      {state === "loading" ? <p>正在读取。</p> : null}
      <div className="oo-table-wrap">
        <table className="oo-table">
          <thead>
            <tr>
              <th>
                <input
                  type="checkbox"
                  aria-label="选择本页"
                  checked={visibleRows.length > 0 && visibleRows.every((row) => selected.has(row.id))}
                  onChange={togglePage}
                />
              </th>
              {columns.map((column) => (
                <th key={column.id}>
                  <button type="button" className="oo-button oo-button-ghost oo-button-sm" onClick={() => onSortChange(column.id)}>
                    {column.header}
                    {sort === column.id ? "，当前排序" : ""}
                  </button>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {visibleRows.map((row) => (
              <tr key={row.id}>
                <td>
                  <input
                    type="checkbox"
                    aria-label={`选择 ${row.id}`}
                    checked={selected.has(row.id)}
                    onChange={() => toggle(row.id)}
                  />
                </td>
                {columns.map((column) => (
                  <td key={column.id}>{column.cell(row)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="oo-actions">
        <button
          type="button"
          className="oo-button oo-button-secondary"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
        >
          上一页
        </button>
        <span>
          {page} / {pageCount}
        </span>
        <button
          type="button"
          className="oo-button oo-button-secondary"
          disabled={page >= pageCount}
          onClick={() => onPageChange(page + 1)}
        >
          下一页
        </button>
      </div>
    </div>
  );
}
