export type PermissionObject = {
  id: string;
  label: string;
};

/** Displays the server preview as given. It does not decide who can see an object. */
export function PermissionPreview({
  principalId,
  role,
  projectId,
  visible,
  denied,
  state = "ready",
}: {
  principalId: string;
  role: string;
  projectId: string;
  visible: readonly PermissionObject[];
  denied: readonly PermissionObject[];
  state?: "ready" | "loading" | "error" | "forbidden" | "readOnly";
}) {
  if (state === "forbidden") return <p>当前身份没有执行此操作的权限。</p>;
  if (state === "error") return <p>预览没有读到。</p>;
  if (state === "loading") return <p aria-busy="true">正在读取预览。</p>;
  return (
    <section className="oo-stack" aria-readonly="true">
      <p className="oo-code">
        {principalId} · {role} · {projectId}
      </p>
      {state === "readOnly" ? <p>这是只读预览，浏览器不能据此授权。</p> : <p>这是服务端预览，浏览器不能据此授权。</p>}
      <h2 className="oo-section-title">可见</h2>
      <ul>
        {visible.map((item) => (
          <li key={item.id}>{item.label}</li>
        ))}
      </ul>
      <h2 className="oo-section-title">拒绝</h2>
      <ul>
        {denied.map((item) => (
          <li key={item.id}>{item.label}</li>
        ))}
      </ul>
    </section>
  );
}
