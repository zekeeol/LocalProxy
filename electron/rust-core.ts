import { app } from 'electron';
import { existsSync } from 'node:fs';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';
import { resolve } from 'node:path';
import { EventEmitter } from 'node:events';

type PendingRequest = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timeout: NodeJS.Timeout;
};

export type RustEvent = {
  event: string;
  payload: unknown;
};

export class RustCoreManager extends EventEmitter {
  private child: ChildProcessWithoutNullStreams | null = null;
  private readonly pending = new Map<string, PendingRequest>();
  private sequence = 0;
  private ready = false;

  async start(): Promise<void> {
    if (this.child && this.ready) return;

    const binary = this.findBinary();
    if (!binary) {
      throw new Error(
        '找不到 Rust Core。请先执行 npm run build:rust，或设置 LOCALPROXY_RUST_BIN。',
      );
    }

    const child = spawn(binary, [], {
      // `__dirname` is inside app.asar in a packaged build, which is a
      // virtual path and cannot be used as a child-process working directory.
      // Resources is a real directory in both the packaged app and the dev
      // layout, so the Rust sidecar can start reliably in either mode.
      cwd: app.isPackaged ? process.resourcesPath : resolve(__dirname, '..'),
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        RUST_BACKTRACE: '1',
      },
    });
    this.child = child;

    const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
    lines.on('line', (line) => this.handleLine(line));
    child.stderr.on('data', (data: Buffer) => {
      // Rust stdout is reserved for NDJSON. Diagnostics stay in the Electron
      // process log and never become visible as a proxy payload.
      console.error(`[LocalProxy Rust] ${data.toString().trim()}`);
    });
    child.once('error', (error) => this.handleExit(error));
    child.once('close', (code, signal) => {
      if (this.child === child) {
        this.handleExit(
          new Error(`Rust Core 已退出（code=${code ?? 'null'}, signal=${signal ?? 'null'}）`),
        );
      }
    });

    this.ready = true;
    try {
      await this.sendRequest<{ pong: boolean }>('ping');
    } catch (error) {
      this.stop();
      throw error;
    }
  }

  async sendRequest<T>(method: string, params: unknown = {}): Promise<T> {
    if (!this.child || !this.ready || !this.child.stdin.writable) {
      throw new Error('Rust Core 尚未就绪');
    }

    const id = `req-${Date.now()}-${++this.sequence}`;
    const request = `${JSON.stringify({ id, method, params })}\n`;
    return new Promise<T>((resolveRequest, rejectRequest) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        rejectRequest(new Error(`Rust RPC ${method} 请求超时`));
      }, 15_000);
      this.pending.set(id, {
        timeout,
        resolve: (value) => resolveRequest(value as T),
        reject: rejectRequest,
      });
      this.child?.stdin.write(request, (error) => {
        if (error) {
          clearTimeout(timeout);
          this.pending.delete(id);
          rejectRequest(error);
        }
      });
    });
  }

  async stop(): Promise<void> {
    const child = this.child;
    if (!child) return;

    if (this.ready && child.stdin.writable) {
      try {
        await this.sendRequest('shutdown');
      } catch {
        // The process may already have exited; killing the child below is the
        // final cleanup path.
      }
    }
    child.kill('SIGTERM');
    this.ready = false;
    this.child = null;
    this.rejectPending(new Error('Rust Core 已停止'));
  }

  private findBinary(): string | null {
    const name = process.platform === 'win32' ? 'local_proxy_core.exe' : 'local_proxy_core';
    const candidates = process.env.LOCALPROXY_RUST_BIN
      ? [process.env.LOCALPROXY_RUST_BIN]
      : app.isPackaged
        ? [resolve(process.resourcesPath, 'bin', name)]
        : [
            resolve(__dirname, '..', 'rust', 'target', 'debug', name),
            resolve(__dirname, '..', 'rust', 'target', 'release', name),
          ];
    return candidates.find((candidate): candidate is string => Boolean(candidate && existsSync(candidate))) ?? null;
  }

  private handleLine(line: string): void {
    if (!line.trim()) return;
    let message: { id?: string; event?: string; payload?: unknown; success?: boolean; result?: unknown; error?: string };
    try {
      message = JSON.parse(line) as typeof message;
    } catch {
      console.error('[LocalProxy Rust] 忽略无效 NDJSON 消息');
      return;
    }

    if (message.event) {
      this.emit('event', { event: message.event, payload: message.payload } satisfies RustEvent);
      return;
    }
    if (!message.id) return;
    const pending = this.pending.get(message.id);
    if (!pending) return;
    clearTimeout(pending.timeout);
    this.pending.delete(message.id);
    if (message.success) {
      pending.resolve(message.result);
    } else {
      pending.reject(new Error(message.error || 'Rust RPC 执行失败'));
    }
  }

  private handleExit(error: Error): void {
    this.ready = false;
    this.rejectPending(error);
    this.emit('exit', error);
  }

  private rejectPending(error: Error): void {
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timeout);
      pending.reject(error);
      this.pending.delete(id);
    }
  }
}
