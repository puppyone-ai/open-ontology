import { GateList, type GateItem } from "../../components/shared/gate-list";
import { JobTimeline } from "../../components/shared/job-timeline";
import { StatusBadge } from "../../components/shared/status-badge";
import { Wizard, type WizardStep } from "../../components/shared/wizard";
import { EvidencePanel } from "../../components/shared/evidence-panel";
import { DiffViewer } from "../../components/shared/diff-viewer";

const RELEASE_GATES: readonly GateItem[] = [
  "G1",
  "G2",
  "G3",
  "G4",
  "G5a",
  "G5b",
  "G6",
  "G7-pre",
  "G7-post",
  "G8",
  "G9",
  "G10",
  "G11",
  "G12",
  "G13",
].map((id) => ({ id, state: "not_run", summary: "未运行", reportHash: null }));

const SOURCE_STEPS: readonly WizardStep[] = [
  {
    id: "connect",
    title: "连接",
    fields: [{ id: "name", label: "数据源名称", kind: "text" }],
  },
  { id: "inspect", title: "体检", fields: [] },
  { id: "tables", title: "选表", fields: [] },
  { id: "dba", title: "DBA 清单", fields: [] },
  { id: "plan", title: "计划", fields: [] },
  { id: "apply", title: "应用", fields: [] },
];

const FILE_STEPS: readonly WizardStep[] = [
  { id: "format", title: "格式", fields: [{ id: "label", label: "文件标签", kind: "text" }] },
  { id: "upload", title: "上传", fields: [] },
  { id: "preview", title: "预览", fields: [] },
  { id: "mapping", title: "映射", fields: [] },
  { id: "plan", title: "计划", fields: [] },
  { id: "apply", title: "应用", fields: [] },
];

const RECOVERY_STEPS: readonly WizardStep[] = [
  { id: "backup", title: "选择备份", fields: [] },
  { id: "precheck", title: "兼容预检", fields: [] },
  { id: "confirm", title: "确认", fields: [{ id: "owner", label: "负责人", kind: "text" }] },
  { id: "run", title: "运行", fields: [] },
  { id: "evidence", title: "恢复证据", fields: [] },
];

function inertWizard(steps: readonly WizardStep[]) {
  return (
    <Wizard
      steps={steps}
      planVersion={null}
      canSubmit={false}
      onPlanInvalidated={() => undefined}
      onSubmit={() => undefined}
    />
  );
}

export function SourceWizard() {
  return inertWizard(SOURCE_STEPS);
}

export function FileWizard() {
  return inertWizard(FILE_STEPS);
}

export function RecoveryWizard() {
  return inertWizard(RECOVERY_STEPS);
}

export function ReviewDesk() {
  return (
    <>
      <p>没有候选</p>
      <EvidencePanel items={[]} state="empty" />
    </>
  );
}

export function ReleaseGates() {
  return (
    <>
      <GateList items={RELEASE_GATES} />
      <DiffViewer rows={[]} state="empty" />
    </>
  );
}

export function JobRecord() {
  return <JobTimeline events={[]} progress={null} state="disabled" />;
}

export function BuildStages() {
  const stages = ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10"];
  return (
    <ol>
      {stages.map((stage) => (
        <li key={stage}>
          <StatusBadge kind="not_run" /> {stage} 没有运行记录
        </li>
      ))}
    </ol>
  );
}

export function MetricDimensions() {
  const dimensions = ["可执行", "对账", "owner 认证", "已发布"];
  return (
    <ul>
      {dimensions.map((dimension) => (
        <li key={dimension}>{dimension}：尚未接入</li>
      ))}
    </ul>
  );
}
