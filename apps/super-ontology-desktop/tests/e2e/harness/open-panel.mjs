// Driving the native open panel of the upload button through keystrokes: Go to folder (Cmd+Shift+G), the path pasted, Return, Right (into the
// folder's files), Cmd+A, Open. The panel is a window of the app under test and keystrokes go to whatever window is key, so a notification, another
// app that took focus or a panel that was still opening makes the first attempt miss (a full run once stayed at the root folder with the panel open
// and no file uploaded). Each attempt therefore: brings the app to the front, waits for the panel, and either ends with the panel closed or cancels it
// (Escape) so the next attempt starts from the button again. The number of attempts is reported, never hidden.

const RETURN = 36;
const RIGHT = 124;
const ESCAPE = 53;

/**
 * deps: { sheetCount(): Promise<number>, press(chord): Promise, key(code): Promise, setClipboard(text): Promise, front(): Promise,
 *         reopen(): Promise (clicks the button that opens the panel again), sleep(ms): Promise, now(): number }
 * Resolves { attempts, closedAt } and throws when every attempt left the panel open.
 */
export async function chooseThroughPanel(folder, deps, { attempts = 3, waitPanelMs = 15_000, settleMs = 1000 } = {}) {
  const { sheetCount, press, key, setClipboard, front, reopen, sleep, now } = deps;
  const waitFor = async (predicate, limitMs) => { for (let waited = 0; waited < limitMs; waited += 250) { if (await predicate()) return true; await sleep(250); } return predicate(); };
  const closePanel = async () => {
    for (let tries = 0; tries < 3 && (await sheetCount()) > 0; tries += 1) { await key(ESCAPE); await sleep(700); }
  };
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    if (attempt > 1) { await closePanel(); await reopen(); }
    if (!(await waitFor(async () => (await sheetCount()) > 0, waitPanelMs))) continue;
    await front();
    await sleep(settleMs);
    await setClipboard(folder);
    await press("cmd+shift+g");
    await sleep(800);
    await press("cmd+v");
    await sleep(800);
    await key(RETURN);
    await sleep(1500);
    await key(RIGHT);
    await sleep(600);
    await press("cmd+a");
    await sleep(500);
    for (let tries = 0; tries < 3; tries += 1) {
      await key(RETURN);
      await sleep(settleMs);
      if ((await sheetCount()) === 0) return { attempts: attempt, closedAt: now() };
    }
  }
  await closePanel();
  throw new Error(`the open panel did not accept ${folder} in ${attempts} attempts`);
}
