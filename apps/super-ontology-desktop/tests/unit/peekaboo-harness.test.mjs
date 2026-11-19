import { describe, expect, it } from "vitest";
import { createPeekaboo } from "../e2e/harness/peekaboo.mjs";

const element = (id, role, label, actionable = true) => ({ id, role, label, title: label, is_actionable: actionable });

function recorder(elements) {
  const calls = [];
  const run = async (_file, args, options) => {
    calls.push({ args, env: options?.env ?? {} });
    return { stdout: args[0] === "see" ? JSON.stringify({ data: { snapshot_id: "ps1_x", ui_elements: elements } }) : "" };
  };
  return { calls, peekaboo: createPeekaboo({ app: "Super Ontology", run, flags: ["--no-remote"] }) };
}

describe("the Peekaboo harness", () => {
  it("clicks the element named exactly, not another whose name only contains the text", async () => {
    const { calls, peekaboo } = recorder([element("elem_9", "button", "Ontology version 1"), element("elem_13", "button", "Ontology")]);
    await peekaboo.click("Ontology");
    const click = calls.find((call) => call.args[0] === "click");
    expect(click.args.slice(0, 5)).toEqual(["click", "--on", "elem_13", "--snapshot", "ps1_x"]);
  });

  it("falls back to the query when nothing is called exactly that, and asks for the web content", async () => {
    const { calls, peekaboo } = recorder([element("elem_2", "other", "Ontology", false)]);
    await peekaboo.click("Dana Lee");
    const click = calls.find((call) => call.args[0] === "click");
    expect(click.args[1]).toBe("Dana Lee");
    expect(calls.find((call) => call.args[0] === "see").args).toContain("--web-focus");
  });
});
