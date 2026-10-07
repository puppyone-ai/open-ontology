import { useRef, useState, type KeyboardEvent } from "react";

export type GraphNode = { id: string; label: string };
export type GraphEdge = { id: string; from: string; to: string; label: string };

/** List of nodes the server already authorized. Search does not grant access. */
export function ObjectGraph({
  nodes,
  edges,
  state = "ready",
}: {
  nodes: readonly GraphNode[];
  edges: readonly GraphEdge[];
  state?: "ready" | "loading" | "error" | "forbidden" | "empty";
}) {
  const [query, setQuery] = useState("");
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);
  if (state === "forbidden") return <p>当前身份没有执行此操作的权限。</p>;
  if (state === "error") return <p>关系没有读到。</p>;
  if (state === "loading") return <p aria-busy="true">正在读取关系。</p>;
  const visible = nodes.filter((node) => node.label.includes(query) || node.id.includes(query));
  if (state === "empty" || nodes.length === 0) return <p>没有可显示的对象。</p>;

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") {
      return;
    }
    event.preventDefault();
    const next = event.key === "ArrowDown" ? index + 1 : index - 1;
    buttons.current[next]?.focus();
  }

  return (
    <div className="oo-stack">
      <label>
        在已授权对象中查找
        <input className="oo-input" value={query} onChange={(event) => setQuery(event.target.value)} />
      </label>
      <ul>
        {visible.map((node, index) => (
          <li key={node.id}>
            <button
              type="button"
              className="oo-button oo-button-ghost"
              ref={(element) => {
                buttons.current[index] = element;
              }}
              onKeyDown={(event) => onKeyDown(event, index)}
            >
              {node.label}
            </button>
          </li>
        ))}
      </ul>
      <h2 className="oo-section-title">关系</h2>
      <ul>
        {edges.map((edge) => (
          <li key={edge.id}>
            {edge.from} → {edge.to} · {edge.label}
          </li>
        ))}
      </ul>
    </div>
  );
}
