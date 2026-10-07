import { AdminSurface } from "../admin-overview/surface";

export function EnumPage() {
  return (
    <AdminSurface
      title="码表"
      description="码、分类、有效期和未知签名都还没有记录。"
      groups={["B05"]}
      operationIds={["B05.codeTables"]}
      action={{ id: "a20", label: "保存" }}
      notes={["新码不会被塞进已有业务类。","这里没有行数。unknown 不是 0。","这个地址不代表已经存在的对象。"]}
     />
  );
}

export function CalendarPage() {
  return (
    <AdminSurface
      title="业务日历"
      description="时区、财年、周期、锚点和结账都还没有记录。"
      groups={["B05"]}
      operationIds={["B05.calendar"]}
      action={{ id: "a21", label: "保存" }}
      notes={["没有旧值、新值或生效时刻。"]}
     />
  );
}
