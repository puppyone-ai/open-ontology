export type ChartPoint = {
  id: string;
  label: string;
  exact: string;
};

function barWidths(values: readonly string[]): number[] | null {
  const numbers: number[] = [];
  for (const value of values) {
    if (!/^-?\d+(\.\d+)?$/.test(value)) {
      return null;
    }
    const whole = value.replace("-", "").split(".")[0] ?? "";
    if (whole.length > 15) {
      return null;
    }
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) {
      return null;
    }
    numbers.push(Math.abs(parsed));
  }
  const max = Math.max(...numbers, 0);
  if (max === 0) {
    return numbers.map(() => 0);
  }
  return numbers.map((value) => value / max);
}

export function NumericChart({
  points,
  unit,
  state = "ready",
}: {
  points: readonly ChartPoint[];
  unit: string;
  state?: "ready" | "loading" | "error" | "forbidden" | "empty";
}) {
  if (state === "forbidden") return <p>当前身份没有执行此操作的权限。</p>;
  if (state === "error") return <p>图表没有读到。</p>;
  if (state === "loading") return <p aria-busy="true">正在读取图表。</p>;
  if (state === "empty" || points.length === 0) return <p>没有可显示的序列。</p>;
  const widths = barWidths(points.map((point) => point.exact));
  return (
    <div className="oo-stack">
      <p>{`单位 ${unit}。条形长度是近似。表格里的数字是原文字。`}</p>
      {widths
        ? points.map((point, index) => (
            <div key={point.id}>
              <span>{point.label}</span>
              <div className="oo-bar-track" title={point.exact}>
                <div className="oo-bar" style={{ width: `${(widths[index] ?? 0) * 100}%` }} />
              </div>
            </div>
          ))
        : null}
      <table className="oo-table">
        <thead>
          <tr>
            <th>标签</th>
            <th>精确值</th>
          </tr>
        </thead>
        <tbody>
          {points.map((point) => (
            <tr key={point.id}>
              <td>{point.label}</td>
              <td className="oo-exact">{point.exact}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
