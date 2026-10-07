import { AdminSurface } from "./surface";

export function OverviewPage() {
  return (
    <AdminSurface
      title="总览"
      description="阻断、就绪、源、构建、会话和评测都还没有读数。"
      groups={["B02","B03","B04","B08","B10"]}
      operationIds={["B02.sourceConfig","B03.jobs","B04.builder","B08.session","B10.suites"]}
      action={{ id: "a03", label: "刷新" }}
      notes={["导航只列出分区，不列出源、表、会话或数量。","没有阻断计数。未知不是 0。"]}
     />
  );
}
