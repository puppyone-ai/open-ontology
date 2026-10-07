import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ActionDialog } from "./action-dialog";
import { ChatComposer } from "./chat-composer";
import { DiffViewer } from "./diff-viewer";
import { GateList } from "./gate-list";
import { NumericChart } from "./numeric-chart";
import { PermissionPreview } from "./permission-preview";
import { QueryResultTable } from "./query-result-table";
import { ResourceTable } from "./resource-table";
import { ReviewDecisionForm } from "./review-decision-form";
import { SafeMarkdown } from "./safe-markdown";
import { SecretInput } from "./secret-input";
import { StatusBadge } from "./status-badge";

describe("shared public components", () => {
  it("keeps selection on the current page and separates empty from forbidden", async () => {
    const user = userEvent.setup();
    function Harness() {
      const [page, setPage] = useState(1);
      const rows = page === 1 ? [{ id: "row-a", label: "甲" }] : [{ id: "row-b", label: "乙" }];
      return (
        <ResourceTable
          rows={rows}
          columns={[{ id: "label", header: "名称", cell: (row) => row.label }]}
          page={page}
          pageCount={2}
          onPageChange={setPage}
          sort={null}
          onSortChange={() => undefined}
          filter=""
          onFilterChange={() => undefined}
          count={{ exact: 2 }}
          state="ready"
        />
      );
    }
    const view = render(<Harness />);
    await user.click(screen.getByRole("checkbox", { name: "选择 row-a" }));
    expect(screen.getAllByText("本页已选1项").length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "下一页" }));
    expect(screen.getByText("选择已清除")).toBeTruthy();
    expect(screen.getByText("本页已选0项")).toBeTruthy();
    view.rerender(
      <ResourceTable
        rows={[{ id: "hidden-object", label: "不应出现" }]}
        columns={[{ id: "label", header: "名称", cell: (row: { id: string; label: string }) => row.label }]}
        page={1}
        pageCount={1}
        onPageChange={() => undefined}
        sort={null}
        onSortChange={() => undefined}
        filter=""
        onFilterChange={() => undefined}
        count={{ exact: null, reason: "没有数量" }}
        state="forbidden"
      />,
    );
    expect(screen.queryByText("不应出现")).toBeNull();
    expect(screen.getByText("当前身份没有执行此操作的权限。")).toBeTruthy();
    expect(screen.queryByText("没有符合当前筛选的记录。")).toBeNull();
  });

  it("does not treat unknown or not_run as pass", () => {
    render(
      <div>
        <StatusBadge kind="unknown" />
        <GateList items={[{ id: "G1", state: "not_run", summary: "还没有跑", reportHash: null }]} />
      </div>,
    );
    expect(screen.getByText("不兼容").dataset.tone).toBe("neutral");
    expect(screen.getByText("未运行").dataset.tone).not.toBe("pass");
  });

  it("preserves literal diffs, NULL, and exact decimals", async () => {
    const user = userEvent.setup();
    render(
      <div>
        <DiffViewer rows={[{ id: "scale", label: "比例", before: "1.0", after: "1.00" }]} />
        <QueryResultTable
          columns={["值", "文本"]}
          rows={[{ id: "r1", cells: [{ type: "null" }, { type: "decimal", value: "12345678901234567890.50" }, { type: "text", value: "" }] }]}
          queryId={null}
          truncated
          warnings={["结果被截断"]}
          receipt={null}
        />
        <NumericChart points={[{ id: "n", label: "样本", exact: "12345678901234567890.50" }]} unit="1" />
      </div>,
    );
    expect(screen.getByText("1.0")).toBeTruthy();
    expect(screen.getByText("1.00")).toBeTruthy();
    expect(screen.getByText("NULL")).toBeTruthy();
    expect(screen.getAllByText("12345678901234567890.50").length).toBeGreaterThan(0);
    expect(screen.getByLabelText("空字符串")).toBeTruthy();
    expect(screen.getByText("没有 receipt，不能标成已完成。")).toBeTruthy();
    expect(screen.getByText("单位 1。条形长度是近似。表格里的数字是原文字。")).toBeTruthy();
    await user.click(screen.getAllByRole("button", { name: "复制" })[0]!);
  });

  it("renders markdown as text and links only a safe citation", () => {
    const { container } = render(
      <SafeMarkdown
        source={"<script>alert(1)</script>\n\n![图](https://example.invalid/a)\n\n[^fake]\n\n[^ok]"}
        citations={[
          { id: "fake", href: "javascript:alert(1)" },
          { id: "ok", href: "/receipts/ok" },
        ]}
      />,
    );
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("<script>alert(1)</script>")).toBeTruthy();
    expect(screen.getByRole("link", { name: "[^ok]" }).getAttribute("href")).toBe("/receipts/ok");
    expect(screen.queryByRole("link", { name: "[^fake]" })).toBeNull();
  });

  it("does not send while an IME composition is active", async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<ChatComposer state="ready" onSend={onSend} onStop={vi.fn()} onEndSession={vi.fn()} />);
    const box = screen.getByRole("textbox", { name: "问题" });
    await user.type(box, "你好");
    fireEvent.compositionStart(box);
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.compositionEnd(box);
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSend).toHaveBeenCalledWith("你好");
  });

  it("keeps an empty secret from overwriting storage", async () => {
    const user = userEvent.setup();
    const onReplace = vi.fn();
    const onClear = vi.fn();
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    render(<SecretInput configured refLabel="secret-ref" canWrite onReplace={onReplace} onClear={onClear} />);
    await user.click(screen.getByRole("button", { name: "替换" }));
    expect(onReplace).not.toHaveBeenCalled();
    expect(screen.getByText("空输入不会覆盖已有值。")).toBeTruthy();
    await user.type(screen.getByLabelText("新的 Secret"), "not-a-real-secret");
    await user.click(screen.getByRole("button", { name: "替换" }));
    expect(onReplace).toHaveBeenCalledWith("not-a-real-secret");
    expect(screen.getByLabelText("新的 Secret")).toHaveValue("");
    await user.click(screen.getByRole("button", { name: "清除" }));
    expect(onClear).toHaveBeenCalledOnce();
    expect(setItem).not.toHaveBeenCalled();
    setItem.mockRestore();
  });

  it("asks for a new review decision when the evidence is stale", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const view = render(<ReviewDecisionForm candidateId="candidate" revision="1" onSubmit={onSubmit} />);
    await user.click(screen.getByRole("radio", { name: "接受" }));
    await user.click(screen.getByRole("button", { name: "提交决定" }));
    expect(onSubmit).toHaveBeenCalledOnce();
    view.rerender(<ReviewDecisionForm candidateId="candidate" revision="2" stale onSubmit={onSubmit} />);
    expect(screen.getByText("证据已变化，需要重新决定。")).toBeTruthy();
    expect(screen.getByRole("button", { name: "提交决定" })).toBeDisabled();
    expect(onSubmit).toHaveBeenCalledOnce();
  });

  it("requires a reason before a destructive confirmation", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(
      <ActionDialog
        open
        title="确认动作"
        scope="DEMO 范围"
        impact="只影响这个 DEMO 范围"
        destructive
        dirty={false}
        onConfirm={onConfirm}
        onOpenChange={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "确认" })).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "理由" }), "需要留下原因");
    await user.click(screen.getByRole("button", { name: "确认" }));
    expect(onConfirm).toHaveBeenCalledWith("需要留下原因");
  });

  it("shows a denied preview object instead of filtering it away", () => {
    render(
      <PermissionPreview
        principalId="example-principal"
        role="example-role"
        projectId="example-project"
        visible={[{ id: "visible", label: "DEMO 可见" }]}
        denied={[{ id: "denied", label: "DEMO 拒绝" }]}
      />,
    );
    expect(screen.getByText("DEMO 拒绝")).toBeTruthy();
    expect(screen.getByText("浏览器不能据此授权。", { exact: false })).toBeTruthy();
  });
});
