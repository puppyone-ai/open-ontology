import { AdminSurface } from "../admin-overview/surface";
import { ReviewDesk } from "../admin-overview/blocks";

export function ReviewsPage() {
  return (
    <AdminSurface
      title="审核"
      description="待办队列、类型和影响都还没有服务端记录。"
      groups={["B04"]}
      operationIds={["B04.reviews", "listReviewItems"]}
      action={{ id: "a17", label: "刷新" }}
      notes={["这里没有审核对象，也没有数量。"]}
     />
  );
}

export function ReviewDetailPage() {
  return (
    <AdminSurface
      title="审核决定"
      description="接受、改后接受、拒绝、暂缓和转交都还不能提交。"
      groups={["B04"]}
      operationIds={["B04.reviews", "B04.candidates", "getReviewItem", "previewReviewBatch", "decideReviewItem"]}
      action={{ id: "a18", label: "转交" }}
      notes={["没有 actor、时间和 seal。","决定保存之后才会说尚未发布。这一页没有保存。","这个地址不代表已经存在的对象。"]}
    >
      <ReviewDesk />
    </AdminSurface>
  );
}
