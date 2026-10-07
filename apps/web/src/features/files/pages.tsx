import { AdminSurface } from "../admin-overview/surface";
import { FileWizard } from "../admin-overview/blocks";

export function FileWizardPage() {
  return (
    <AdminSurface
      title="上传文件"
      description="格式、分片、预览、映射和导入都还没有服务端结果。"
      groups={["B07","B02"]}
      operationIds={["B07.objectPart","B07.objectComplete","B07.objectAbort","B02.plan","B02.apply"]}
      action={{ id: "a07", label: "应用" }}
      notes={["上传完成不等于导入，导入不等于发布。","没有文件名，也没有校验和。","不明格式不会被当成 CSV。"]}
    >
      <FileWizard />
    </AdminSurface>
  );
}

export function FileDetailPage() {
  return (
    <AdminSurface
      title="文件"
      description="对象版本、拒绝、保留和删除都还没有服务端证据。"
      groups={["B07","B03"]}
      operationIds={["B07.download","B07.objectDelete","B03.jobs"]}
      action={{ id: "a08", label: "删除" }}
      notes={["这里不提供下载。删除没有 absence 证据。","这个地址不代表已经存在的对象。"]}
     />
  );
}
