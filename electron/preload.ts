import { contextBridge, ipcRenderer } from 'electron';

type RustEvent = { event: string; payload: unknown };

const electronAPI = {
  isElectron: true,
  platform: process.platform,
  minimizeWindow: () => ipcRenderer.send('window:minimize'),
  maximizeWindow: () => ipcRenderer.send('window:maximize'),
  closeWindow: () => ipcRenderer.send('window:close'),
  request: <T>(method: string, params: unknown = {}) =>
    ipcRenderer.invoke('rust:request', method, params) as Promise<T>,
  choosePrivateKey: () => ipcRenderer.invoke('dialog:choose-private-key') as Promise<string | null>,
  onRustEvent: (callback: (event: RustEvent) => void) => {
    const handler = (_event: Electron.IpcRendererEvent, payload: RustEvent) => callback(payload);
    ipcRenderer.on('rust:event', handler);
    return () => ipcRenderer.removeListener('rust:event', handler);
  },
};

contextBridge.exposeInMainWorld('electronAPI', electronAPI);
