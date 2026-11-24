// Renderer-agnostic: asks the app's own bridge for the engine snapshot and waits for `healthy` (the app started its engine itself).
export default async function engineHealthy({ cdp, args }) {
  const deadline = Date.now() + (args.withinMs ?? 150_000);
  let last = "no answer";
  while (Date.now() < deadline) {
    const snapshot = await cdp.evaluate("window.superOntology?.invoke('engine-read', {}).then((r) => r.ok ? { status: r.value.status, owned: r.value.ownedByDesktop } : { status: 'error' })").catch(() => null);
    if (snapshot) {
      last = `${snapshot.status}${snapshot.owned ? " (owned)" : ""}`;
      if (snapshot.status === "healthy") return { ok: true, detail: last };
    }
    await new Promise((resolve) => { setTimeout(resolve, 1000); });
  }
  return { ok: false, detail: `engine status ${last}` };
}
