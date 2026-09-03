import { app, BrowserWindow, dialog, ipcMain } from 'electron';
import { join } from 'node:path';
import { RustCoreManager } from './rust-core';

let mainWindow: BrowserWindow | null = null;
const rustCore = new RustCoreManager();

function registerIpcHandlers(): void {
  ipcMain.on('window:minimize', () => mainWindow?.minimize());
  ipcMain.on('window:maximize', () => {
    if (!mainWindow) return;
    if (mainWindow.isMaximized()) mainWindow.unmaximize();
    else mainWindow.maximize();
  });
  ipcMain.on('window:close', () => mainWindow?.close());

  ipcMain.handle('rust:request', async (_event, method: string, params: unknown) => {
    return rustCore.sendRequest(method, params);
  });
  ipcMain.handle('dialog:choose-private-key', async () => {
    const result = await dialog.showOpenDialog({
      title: '选择 SSH 私钥',
      properties: ['openFile'],
      filters: [
        { name: 'SSH 私钥', extensions: ['pem', 'key', 'pub'] },
        { name: '所有文件', extensions: ['*'] },
      ],
    });
    return result.canceled ? null : result.filePaths[0] ?? null;
  });
}

function forwardRustEvents(): void {
  rustCore.on('event', (event) => {
    if (mainWindow && !mainWindow.isDestroyed()) mainWindow.webContents.send('rust:event', event);
  });
  rustCore.on('exit', (error) => {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('rust:event', {
        event: 'core_exit',
        payload: { message: error.message },
      });
    }
  });
}

async function createWindow(): Promise<void> {
  mainWindow = new BrowserWindow({
    width: 1360,
    height: 900,
    minWidth: 1060,
    minHeight: 700,
    title: 'LocalProxy',
    backgroundColor: '#edf4f6',
    titleBarStyle: process.platform === 'darwin' ? 'hidden' : 'default',
    frame: process.platform === 'darwin' ? false : true,
    webPreferences: {
      preload: join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  if (process.platform === 'darwin') mainWindow.setWindowButtonVisibility(false);

  if (app.isPackaged) {
    await mainWindow.loadFile(join(__dirname, '..', 'dist', 'index.html'));
  } else {
    await mainWindow.loadURL(process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:3000');
  }
  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

app.whenReady().then(async () => {
  registerIpcHandlers();
  forwardRustEvents();
  try {
    await rustCore.start();
  } catch (error) {
    console.error(`[LocalProxy] Rust Core 启动失败: ${String(error)}`);
  }
  await createWindow();
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) void createWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

app.on('before-quit', () => {
  void rustCore.stop();
});
