// A bare Electron main that opens one URL (the atlas lab or performance page served by vite) in a window with the real renderer runtime.
const { app, BrowserWindow } = require("electron");
const url = process.env.ATLAS_LAB_URL;
app.commandLine.appendSwitch("disable-renderer-backgrounding");
app.whenReady().then(() => {
  const win = new BrowserWindow({ width: Number(process.env.ATLAS_LAB_W ?? 1280), height: Number(process.env.ATLAS_LAB_H ?? 800), show: process.env.ATLAS_LAB_SHOW === "1", webPreferences: { backgroundThrottling: false } });
  void win.loadURL(url);
});
app.on("window-all-closed", () => app.quit());
