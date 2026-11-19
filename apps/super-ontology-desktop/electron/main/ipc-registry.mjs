/**
 * The only way a handler is exposed to the renderer. Every call is checked: the sender must be the main frame of one of the
 * app's own windows, loaded from the app's own origin. Nothing in this app is registered on raw `ipcMain` elsewhere.
 */
export function createIpcRegistry({ ipcMain, isTrustedSender, logger = console }) {
  const registered = new Set();
  const refuse = () => ({ ok: false, error: { code: "SO_INVALID_REQUEST", retryable: false, details: {} } });

  function handle(channel, handler) {
    if (registered.has(channel)) throw new Error(`IPC channel registered twice: ${channel}`);
    registered.add(channel);
    ipcMain.handle(channel, async (event, ...args) => {
      if (!isTrustedSender(event)) { logger.warn?.("Refused an IPC call from an untrusted frame.", { channel }); return refuse(); }
      return handler(event, ...args);
    });
  }

  /** The `ipcMain` shaped facade the Super Ontology handlers register on. */
  const facade = Object.freeze({ handle });
  return Object.freeze({ handle, facade, channels: () => [...registered] });
}
