import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { ActionDialog } from "../shared/action-dialog";
import { AppShell } from "../shared/app-shell";
import { ChatComposer } from "../shared/chat-composer";
import { DiffViewer } from "../shared/diff-viewer";
import { EvidencePanel } from "../shared/evidence-panel";
import { GateList } from "../shared/gate-list";
import { JobTimeline } from "../shared/job-timeline";
import { NumericChart } from "../shared/numeric-chart";
import { ObjectGraph } from "../shared/object-graph";
import { PageHeader } from "../shared/page-header";
import { PermissionPreview } from "../shared/permission-preview";
import { QueryResultTable } from "../shared/query-result-table";
import { ResourceTable } from "../shared/resource-table";
import { ReviewDecisionForm } from "../shared/review-decision-form";
import { SafeMarkdown } from "../shared/safe-markdown";
import { SecretInput } from "../shared/secret-input";
import { StatePanel } from "../shared/state-panel";
import { StatusBadge } from "../shared/status-badge";
import { VersionStrip } from "../shared/version-strip";
import { Wizard } from "../shared/wizard";

function Demo({ children }: { children: ReactNode }) {
  return (
    <div className="oo-page">
      <p className="oo-demo">DEMO。这不是生产数据，也不是已接入的后端结果。</p>
      {children}
    </div>
  );
}

const longZh = "这是一段很长的中文标题，用来检查换行、字重，以及关键文字在放大后仍然完整可见。";
const longEn = "This is a long English title used to check wrapping and that the important words stay visible.";

export default { title: "public/shared" };

export const C01Default = () => (
  <MemoryRouter>
    <Demo>
      <AppShell entry="chat" identity={null} nav={[{ id: "login", label: "登录", to: "/login" }]} modules={[]}>
        <p>当前没有身份。</p>
      </AppShell>
    </Demo>
  </MemoryRouter>
);

export const C01Long = () => (
  <MemoryRouter>
    <Demo>
      <AppShell
        entry="chat"
        identity={{ label: longZh, projectId: "example-project", role: "example-role" }}
        nav={[{ id: "en", label: longEn, to: "/login" }]}
        modules={[]}
      >
        <p>{longEn}</p>
      </AppShell>
    </Demo>
  </MemoryRouter>
);

export const C02Default = () => (
  <Demo>
    <PageHeader title="页面标题" description="说明" actions={[{ id: "stay", label: "不可执行", emphasis: "primary", disabled: true, disabledReason: "此能力尚未接入。" }]} />
  </Demo>
);

export const C02Long = () => (
  <Demo>
    <PageHeader title={longZh} description={longEn} />
  </Demo>
);

export const C03Default = () => (
  <Demo>
    <ResourceTable
      rows={[{ id: "demo-1", label: "DEMO 对象" }]}
      columns={[{ id: "label", header: "名称", cell: (row: { id: string; label: string }) => row.label }]}
      page={1}
      pageCount={1}
      onPageChange={() => undefined}
      sort={null}
      onSortChange={() => undefined}
      filter=""
      onFilterChange={() => undefined}
      count={{ exact: 1 }}
      state="ready"
    />
  </Demo>
);

export const C03Empty = () => (
  <Demo>
    <ResourceTable
      rows={[]}
      columns={[{ id: "label", header: "名称", cell: () => null }]}
      page={1}
      pageCount={1}
      onPageChange={() => undefined}
      sort={null}
      onSortChange={() => undefined}
      filter=""
      onFilterChange={() => undefined}
      count={{ exact: 0 }}
      state="empty"
    />
  </Demo>
);

export const C03Error = () => (
  <Demo>
    <ResourceTable rows={[]} columns={[]} page={1} pageCount={1} onPageChange={() => undefined} sort={null} onSortChange={() => undefined} filter="没有" onFilterChange={() => undefined} count={{ exact: null, reason: "数量未知" }} state="error" />
  </Demo>
);

export const C03Forbidden = () => (
  <Demo>
    <ResourceTable rows={[{ id: "hidden", label: "隐藏对象" }]} columns={[{ id: "label", header: "名称", cell: (row: { label: string }) => row.label }]} page={1} pageCount={1} onPageChange={() => undefined} sort={null} onSortChange={() => undefined} filter="" onFilterChange={() => undefined} count={{ exact: null, reason: "没有数量" }} state="forbidden" />
  </Demo>
);

export const C03Loading = () => (
  <Demo>
    <ResourceTable rows={[{ id: "demo-1", label: "DEMO 对象" }]} columns={[{ id: "label", header: "名称", cell: (row: { label: string }) => row.label }]} page={1} pageCount={1} onPageChange={() => undefined} sort={null} onSortChange={() => undefined} filter="" onFilterChange={() => undefined} count={{ exact: 1 }} state="loading" />
  </Demo>
);

export const C04States = () => (
  <Demo>
    <div className="oo-actions">
      <StatusBadge kind="pass" />
      <StatusBadge kind="not_run" />
      <StatusBadge kind="pending" />
      <StatusBadge kind="unknown" />
      <StatusBadge kind="forbidden" />
      <StatusBadge kind="loading" />
      <StatusBadge kind="error" />
      <StatusBadge kind="disabled" />
      <StatusBadge kind="read_only" />
      <StatusBadge kind="conflict" />
    </div>
  </Demo>
);

export const C05Default = () => (
  <Demo>
    <EvidencePanel items={[{ id: "demo-evidence", hash: "example-hash", collectedAt: "2026-10-08T00:00:00Z", summary: longZh, technical: "<b>不是 HTML</b>" }]} />
  </Demo>
);

export const C05States = () => (
  <Demo>
    <EvidencePanel items={[]} state="loading" />
    <EvidencePanel items={[]} state="empty" />
    <EvidencePanel items={[]} state="error" />
    <EvidencePanel items={[]} state="forbidden" />
  </Demo>
);

export const C06Default = () => (
  <Demo>
    <ReviewDecisionForm candidateId="demo-candidate" revision="1" onSubmit={() => undefined} />
  </Demo>
);

export const C06Conflict = () => (
  <Demo>
    <ReviewDecisionForm candidateId="demo-candidate" revision="2" stale conflict={{ previous: longZh, next: longEn }} onSubmit={() => undefined} />
  </Demo>
);

export const C06ReadOnly = () => (
  <Demo>
    <ReviewDecisionForm candidateId="demo-candidate" revision="1" readOnly onSubmit={() => undefined} />
  </Demo>
);

export const C07Default = () => (
  <Demo>
    <DiffViewer rows={[{ id: "literal", label: longZh, before: "1.0", after: "1.00" }]} />
  </Demo>
);

export const C07States = () => (
  <Demo>
    <DiffViewer rows={[]} state="loading" />
    <DiffViewer rows={[]} state="empty" />
    <DiffViewer rows={[]} state="error" />
    <DiffViewer rows={[]} state="forbidden" />
  </Demo>
);

export const C08Default = () => (
  <Demo>
    <GateList
      items={[
        { id: "G1", state: "pass", summary: "示例通过", reportHash: "example" },
        { id: "G2", state: "not_run", summary: "未运行", reportHash: null },
        { id: "G3", state: "block", summary: "阻断", reportHash: "example" },
        { id: "G4", state: "warn", summary: "警告", reportHash: "example" },
      ]}
    />
  </Demo>
);

export const C09Default = () => (
  <Demo>
    <JobTimeline
      events={[
        { id: "1", sequence: 1, label: "运行", state: "running" },
        { id: "2", sequence: 2, label: "结束标记", state: "eof" },
      ]}
      progress={{ kind: "estimated", completed: 1, total: 4, denominator: "4 个检查点" }}
    />
  </Demo>
);

export const C09States = () => (
  <Demo>
    <JobTimeline events={[]} progress={null} state="loading" />
    <JobTimeline events={[]} progress={null} state="error" />
    <JobTimeline events={[]} progress={null} state="forbidden" />
    <JobTimeline events={[]} progress={null} state="disabled" />
  </Demo>
);

export const C10Default = () => (
  <Demo>
    <PermissionPreview principalId="example-principal" role="example-role" projectId="example-project" visible={[{ id: "v", label: longZh }]} denied={[{ id: "d", label: "DEMO 拒绝" }]} />
  </Demo>
);

export const C10States = () => (
  <Demo>
    <PermissionPreview principalId="example" role="example" projectId="example" visible={[]} denied={[]} state="loading" />
    <PermissionPreview principalId="example" role="example" projectId="example" visible={[]} denied={[]} state="error" />
    <PermissionPreview principalId="example" role="example" projectId="example" visible={[]} denied={[]} state="forbidden" />
    <PermissionPreview principalId="example" role="example" projectId="example" visible={[]} denied={[]} state="readOnly" />
  </Demo>
);

export const C11Default = () => (
  <Demo>
    <QueryResultTable
      columns={["数值", "文本"]}
      rows={[{ id: "r", cells: [{ type: "decimal", value: "1.2300" }, { type: "null" }, { type: "text", value: "" }] }]}
      queryId={null}
      truncated
      warnings={[longZh]}
      receipt={null}
    />
  </Demo>
);

export const C11States = () => (
  <Demo>
    <QueryResultTable columns={[]} rows={[]} queryId={null} truncated={false} warnings={[]} receipt={null} state="loading" />
    <QueryResultTable columns={[]} rows={[]} queryId={null} truncated={false} warnings={[]} receipt={null} state="empty" />
    <QueryResultTable columns={[]} rows={[]} queryId={null} truncated={false} warnings={[]} receipt={null} state="error" />
    <QueryResultTable columns={[]} rows={[]} queryId={null} truncated={false} warnings={[]} receipt={null} state="forbidden" />
  </Demo>
);

export const C12Default = () => (
  <Demo>
    <SafeMarkdown source={`${longZh}\n\n\`code\`\n\n<script>alert(1)</script>\n\n[^missing]`} />
  </Demo>
);

export const C12English = () => (
  <Demo>
    <SafeMarkdown source={longEn} />
  </Demo>
);

export const C13Default = () => (
  <Demo>
    <ChatComposer state="disabled" disabledReason="此能力尚未接入。依赖 B01、B08。" onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
  </Demo>
);

export const C13States = () => (
  <Demo>
    <ChatComposer state="ready" onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
    <ChatComposer state="sending" onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
    <ChatComposer state="error" disabledReason="没有发出。" onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
    <ChatComposer state="forbidden" onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
    <ChatComposer state="readOnly" onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
  </Demo>
);

export const C14Default = () => (
  <Demo>
    <SecretInput configured={false} refLabel={null} canWrite={false} onReplace={() => undefined} onClear={() => undefined} />
  </Demo>
);

export const C14ReadOnly = () => (
  <Demo>
    <SecretInput configured refLabel="secret-ref" canWrite={false} onReplace={() => undefined} onClear={() => undefined} />
  </Demo>
);

export const C14Unbound = () => (
  <Demo>
    <SecretInput
      configured={false}
      refLabel={null}
      canWrite
      connection="unbound"
      onReplace={() => undefined}
      onClear={() => undefined}
    />
  </Demo>
);

export const C15States = () => (
  <Demo>
    <StatePanel kind="notConnected" dependency="B01" />
    <StatePanel kind="empty" />
    <StatePanel kind="noMatch" />
    <StatePanel kind="loading" />
    <StatePanel kind="error" traceId="example-trace" />
    <StatePanel kind="forbidden" />
    <StatePanel kind="unsupported" />
    <StatePanel kind="conflict" />
    <StatePanel kind="readOnly" />
    <StatePanel kind="notFound" />
  </Demo>
);

export const C16Default = () => (
  <Demo>
    <VersionStrip value={{ project: null, environment: null, catalog: null, policy: "example-policy", knowledge: null, fresh: false }} />
  </Demo>
);

export const C16States = () => (
  <Demo>
    <VersionStrip value={{ project: null, environment: null, catalog: null, policy: null, knowledge: null, fresh: true }} state="loading" />
    <VersionStrip value={{ project: null, environment: null, catalog: null, policy: null, knowledge: null, fresh: true }} state="error" />
    <VersionStrip value={{ project: null, environment: null, catalog: null, policy: null, knowledge: null, fresh: true }} state="forbidden" />
  </Demo>
);

export const C17Default = () => (
  <Demo>
    <ObjectGraph nodes={[{ id: "a", label: longZh }, { id: "b", label: longEn }]} edges={[{ id: "e", from: "a", to: "b", label: "DEMO 关系" }]} />
  </Demo>
);

export const C17States = () => (
  <Demo>
    <ObjectGraph nodes={[]} edges={[]} state="empty" />
    <ObjectGraph nodes={[]} edges={[]} state="loading" />
    <ObjectGraph nodes={[]} edges={[]} state="error" />
    <ObjectGraph nodes={[]} edges={[]} state="forbidden" />
  </Demo>
);

export const C18Default = () => (
  <Demo>
    <NumericChart points={[{ id: "a", label: "甲", exact: "10.00" }, { id: "b", label: "乙", exact: "2.50" }]} unit="1" />
  </Demo>
);

export const C18Exact = () => (
  <Demo>
    <NumericChart points={[{ id: "wide", label: longEn, exact: "12345678901234567890.50" }]} unit="1" />
  </Demo>
);

export const C18States = () => (
  <Demo>
    <NumericChart points={[]} unit="1" state="empty" />
    <NumericChart points={[]} unit="1" state="loading" />
    <NumericChart points={[]} unit="1" state="error" />
    <NumericChart points={[]} unit="1" state="forbidden" />
  </Demo>
);

export const C19Default = () => (
  <Demo>
    <Wizard
      steps={[{ id: "one", title: longZh, fields: [{ id: "name", label: "名称", kind: "text" }, { id: "secret", label: "Secret", kind: "secret" }] }]}
      planVersion={null}
      canSubmit={false}
      onPlanInvalidated={() => undefined}
      onSubmit={() => undefined}
    />
  </Demo>
);

export const C20Default = () => (
  <Demo>
    <ActionDialog open title="确认动作" scope={longZh} impact={longEn} destructive dirty onConfirm={() => undefined} onOpenChange={() => undefined} />
  </Demo>
);

export const C20Closed = () => (
  <Demo>
    <ActionDialog open={false} title="确认动作" scope="DEMO" impact="DEMO" destructive={false} dirty={false} onConfirm={() => undefined} onOpenChange={() => undefined} />
  </Demo>
);
