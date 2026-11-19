// A read-only look at the renderer over the DevTools protocol. Operating the app is Peekaboo's job; this only checks what it shows.
// args: { testid, attributes?: {name: value}, min?: n, max?: n, equals?: n, text?: regex source, attribute?: name, matches?: regex source }
export default async function dom({ cdp, args }) {
  const css = `[data-testid="${args.testid}"]${Object.entries(args.attributes ?? {}).map(([key, value]) => `[${key}=${JSON.stringify(String(value))}]`).join("")}`;
  const deadline = Date.now() + (args.withinMs ?? 30_000);
  let last = "";
  while (Date.now() < deadline) {
    const found = await cdp.evaluate(`(() => { const all = [...document.querySelectorAll(${JSON.stringify(css)})]; return { count: all.length, text: all[0]?.innerText ?? null, attribute: ${JSON.stringify(args.attribute ?? null)} ? all[0]?.getAttribute(${JSON.stringify(args.attribute ?? "")}) : null }; })()`).catch(() => null);
    if (found) {
      const problems = [];
      if (args.min !== undefined && found.count < args.min) problems.push(`${found.count} < ${args.min}`);
      if (args.max !== undefined && found.count > args.max) problems.push(`${found.count} > ${args.max}`);
      if (args.equals !== undefined && found.count !== args.equals) problems.push(`${found.count} != ${args.equals}`);
      if (args.min === undefined && args.max === undefined && args.equals === undefined && found.count === 0) problems.push("not present");
      if (args.text && !new RegExp(args.text, "u").test(found.text ?? "")) problems.push(`text ${JSON.stringify(found.text)} !~ ${args.text}`);
      if (args.matches && !new RegExp(args.matches, "u").test(found.attribute ?? "")) problems.push(`${args.attribute}=${JSON.stringify(found.attribute)} !~ ${args.matches}`);
      last = problems.join("; ");
      if (problems.length === 0) return { ok: true, detail: `${css} x${found.count}` };
    }
    await new Promise((resolve) => { setTimeout(resolve, 300); });
  }
  return { ok: false, detail: `${css}: ${last || "no answer from the page"}` };
}
