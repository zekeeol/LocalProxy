import { app, BrowserWindow, dialog, ipcMain, nativeImage, powerMonitor } from 'electron';
import { join } from 'node:path';
import { RustCoreManager } from './rust-core';

let mainWindow: BrowserWindow | null = null;
const rustCore = new RustCoreManager();

function appIconPath(): string {
  if (app.isPackaged) {
    return join(process.resourcesPath, 'appicon', 'localproxy.png');
  }
  return join(__dirname, '..', 'assets', 'brand', 'localproxy-icon.png');
}

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
    setTimeout(() => {
      rustCore.start().catch((restartError) => {
        console.error(`[LocalProxy] Rust Core 重启失败: ${String(restartError)}`);
      });
    }, 1_000);
  });
}

/**
 * Sleep and screen lock both drop the WireGuard handshake, and the privileged
 * helper cannot notice that by itself. Ask the core for a read-only health
 * check; it re-applies the tunnel configuration through the helper that is
 * already running, so this never triggers an administrator password prompt.
 */
function registerPowerHandlers(): void {
  let lastCheck = 0;
  const recheck = (reason: string): void => {
    const now = Date.now();
    if (now - lastCheck < 5_000) return;
    lastCheck = now;
    setTimeout(() => {
      void rustCore
        .sendRequest('wireguard_revalidate', {})
        .then((checked) => {
          const count = typeof checked === 'number' ? checked : 0;
          if (count > 0) {
            console.info(`[LocalProxy] ${reason}后检查了 ${count} 条 WireGuard 隧道`);
          }
        })
        .catch((error: unknown) => {
          console.warn(`[LocalProxy] ${reason}后检查 WireGuard 隧道失败: ${String(error)}`);
        });
    }, 3_000);
  };
  powerMonitor.on('resume', () => recheck('系统唤醒'));
  powerMonitor.on('unlock-screen', () => recheck('解锁屏幕'));
}

async function createWindow(): Promise<void> {
  mainWindow = new BrowserWindow({
    width: 1360,
    height: 900,
    minWidth: 1060,
    minHeight: 700,
    title: 'LocalProxy',
    backgroundColor: '#edf4f6',
    icon: nativeImage.createFromPath(appIconPath()),
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
  const appIcon = nativeImage.createFromPath(appIconPath());
  if (!appIcon.isEmpty() && process.platform === 'darwin' && app.dock) {
    app.dock.setIcon(appIcon);
  }
  registerIpcHandlers();
  forwardRustEvents();
  registerPowerHandlers();
  try {
    await rustCore.start();
  } catch (error) {
    console.error(`[LocalProxy] Rust Core 启动失败: ${String(error)}`);
    setTimeout(() => {
      void rustCore.start();
    }, 1_000);
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
