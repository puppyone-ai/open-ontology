// The page text never contains an engine token (oodt_...) and shows an agent token (ooa_...) only where a scenario expects it.
export default async function noSecretInPage({ cdp, args }) {
  const text = String(await cdp.evaluate("document.body.innerText"));
  if (/oodt_[A-Za-z0-9_-]{16,}/u.test(text)) return { ok: false, detail: "an engine token is visible" };
  if (args.allowAgentToken !== true && /ooa_[A-Za-z0-9_-]{16,}/u.test(text)) return { ok: false, detail: "an agent token is visible" };
  return { ok: true, detail: "no engine token in the page" };
}
