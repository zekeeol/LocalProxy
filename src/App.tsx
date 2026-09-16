import {
  type Dispatch,
  type FormEvent,
  type SetStateAction,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react';
import {
  Activity,
  AlertCircle,
  ArrowRight,
  Check,
  CheckCircle2,
  ChevronDown,
  CircleHelp,
  Clipboard,
  Copy,
  Globe,
  KeyRound,
  Layers,
  Link2,
  List,
  Lock,
  MoreHorizontal,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Save,
  Server,
  Settings,
  ShieldCheck,
  Square,
  Tag,
  Terminal,
  Trash2,
  Wifi,
  X,
} from 'lucide-react';
import logoMark from '../assets/brand/localproxy-logo.svg';
import type {
  WstunnelConfig,
  WireGuardStatus,
  WireGuardConfig,
  AppSettings,
  AuthMode,
  Endpoint,
  ForwardDraft,
  LogEntry,
  ProfileDraft,
  ProfileSummary,
  ProfileView,
  ProxyEndpoint,
  ProxyEvent,
  ProxyKind,
  RustBridge,
  SecretPatch,
  SecretRow,
  ServiceState,
} from './types';

type Toast = { id: number; message: string; tone: 'success' | 'info' | 'error' };
type HostKeyRequest = { id: string; fingerprint: string; message: string };
type ForwardRowState = ForwardDraft & { id: string };

const DEFAULT_FORWARD: ForwardRowState = {
  id: 'forward-new',
  local_port: 0,
  remote_host: '',
  remote_port: 0,
  tag: '',
};

function bridge(): RustBridge | null {
  return window.electronAPI ?? null;
}

function formatAddress(host: string, port: number): string {
  const formattedHost = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
  return port > 0 ? `${formattedHost}:${port}` : `${formattedHost}:随机端口`;
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function stateClass(state: ServiceState): string {
  switch (state) {
    case 'Running':
      return 'running';
    case 'Starting':
      return 'starting';
    case 'AwaitingHostKey':
      return 'awaiting';
    case 'Reconnecting':
      return 'reconnecting';
    case 'Error':
      return 'error';
    default:
      return 'stopped';
  }
}

function stateLabel(state: ServiceState): string {
  switch (state) {
    case 'Running':
      return '运行中';
    case 'Starting':
      return '启动中';
    case 'AwaitingHostKey':
      return '等待确认';
    case 'Reconnecting':
      return '自动重连';
    case 'Error':
      return '错误';
    default:
      return '已停止';
  }
}

function kindLabel(kind: ProxyKind): string {
  switch (kind) {
    case 'Http': return 'HTTP 反向代理';
    case 'Wstunnel': return 'Wstunnel 隧道';
    case 'WireGuard': return 'WireGuard 隧道';
    default: return 'SSH 端口转发';
  }
}

function isActive(state: ServiceState): boolean {
  return state === 'Running' || state === 'Starting' || state === 'AwaitingHostKey' || state === 'Reconnecting';
}

function endpointForProfile(profile: ProfileView): Endpoint[] {
  if (profile.endpoints.length > 0) return profile.endpoints;
  if (profile.wstunnel) return profile.wstunnel.forwards.map(f => ({kind: f.protocol, host: '127.0.0.1', port: f.local_port, description: `${f.protocol.toUpperCase()} → ${formatAddress(f.remote_host, f.remote_port)}`}));
  if (profile.wireguard) {
    return [{
      kind: 'wireguard',
      host: 'utun',
      port: 0,
      description: profile.wireguard.peers.map((peer) => `${peer.endpoint_host}:${peer.endpoint_port}`).join(', '),
    }];
  }
  if (profile.http) {
    return [
      {
        kind: 'http',
        host: '127.0.0.1',
        port: profile.http.listen_port,
        description: 'HTTP 本地反向代理',
      },
    ];
  }
  return (profile.ssh?.forwards ?? []).map((forward) => ({
    kind: 'sshForward',
    host: '127.0.0.1',
    port: forward.local_port,
    description: `SSH → ${formatAddress(forward.remote_host, forward.remote_port)}`,
  }));
}

function endpointForSummary(profile: ProfileSummary): string {
  const first = profile.endpoints[0];
  if (!first) return '启动后显示本地地址';
  return formatAddress(first.host, first.port);
}

function nowTime(): string {
  return new Date().toLocaleTimeString('zh-CN', { hour12: false });
}

async function copyText(value: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    return false;
  }
}

function parseBatchPairs(text: string): Array<{ name: string; value: string }> {
  const clean = text
    .replace(/\*\*/g, '')
    .replace(/`/g, '')
    .replace(/\r/g, '')
    .split('\n')
    .map((line) => line.trim().replace(/^[-*]\s+/, ''))
    .filter(Boolean);
  const pairs: Array<{ name: string; value: string }> = [];
  for (const line of clean) {
    const separator = line.indexOf(':');
    if (separator > 0 && !/^https?:\/\//i.test(line)) {
      const name = line.slice(0, separator).trim();
      const value = line.slice(separator + 1).trim();
      if (name && value) pairs.push({ name, value });
    }
  }
  if (pairs.length > 0) return pairs;

  for (let index = 0; index + 1 < clean.length; index += 2) {
    const name = clean[index].trim();
    const value = clean[index + 1].trim().replace(/^['"]|['"]$/g, '');
    if (name && value) pairs.push({ name, value });
  }
  if (pairs.length > 0) return pairs;

  return text
    .split(';')
    .map((item) => item.trim())
    .map((item) => {
      const separator = item.indexOf('=');
      return separator > 0
        ? { name: item.slice(0, separator).trim(), value: item.slice(separator + 1).trim() }
        : null;
    })
    .filter((item): item is { name: string; value: string } => item !== null && item.name.length > 0);
}

function makeSecretRows(
  fields: Array<{ name: string; configured: boolean }> | undefined,
  prefix: string,
): SecretRow[] {
  return (fields ?? []).map((field, index) => ({
    id: `${prefix}-${index}-${field.name}`,
    name: field.name,
    value: '',
    configured: field.configured,
    originalName: field.name,
  }));
}

function buildSecretPatches(
  rows: SecretRow[],
  removed: Set<string>,
  label: string,
): { patches: SecretPatch[]; error: string | null } {
  const patches: SecretPatch[] = [];
  for (const row of rows) {
    const name = row.name.trim();
    const value = row.value;
    if (!name && !value) continue;
    if (!name) return { patches: [], error: `${label}名称不能为空` };
    if (!row.configured && !value) return { patches: [], error: `${name}需要填写值` };
    if (row.originalName && row.originalName !== name) {
      patches.push({ name: row.originalName, value: null, kind: 'Clear' });
    }
    patches.push({
      name,
      value: value || null,
      kind: row.configured && !value ? 'Keep' : 'Set',
    });
  }
  for (const name of removed) patches.push({ name, value: null, kind: 'Clear' });
  return { patches, error: null };
}

function AppLogo({ large = false }: { large?: boolean }) {
  return (
    <span className={`logo-mark${large ? ' large' : ''}`} aria-hidden="true">
      <img src={logoMark} alt="" />
    </span>
  );
}

export default function App() {
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selected, setSelected] = useState<ProfileView | null>(null);
  const [settings, setSettings] = useState<AppSettings>({ http_proxy: null, socks_proxy: null });
  const [editor, setEditor] = useState<ProfileView | null | undefined>(undefined);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hostKey, setHostKey] = useState<HostKeyRequest | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [engineReady, setEngineReady] = useState(Boolean(bridge()));
  const [typePickerOpen, setTypePickerOpen] = useState(false);
  const [editorKind, setEditorKind] = useState<ProxyKind>('Http');

  const notify = useCallback((message: string, tone: Toast['tone'] = 'info') => {
    const id = Date.now() + Math.floor(Math.random() * 1000);
    setToasts((current) => [...current.slice(-3), { id, message, tone }]);
    window.setTimeout(() => setToasts((current) => current.filter((toast) => toast.id !== id)), 3800);
  }, []);

  const request = useCallback(async <T,>(method: string, params: unknown = {}): Promise<T> => {
    const currentBridge = bridge();
    if (!currentBridge) throw new Error('请通过 LocalProxy Electron 桌面端运行');
    return currentBridge.request<T>(method, params);
  }, []);

  const loadProfile = useCallback(async (id: string) => {
    const profile = await request<ProfileView>('get_profile', { id });
    setSelected(profile);
    return profile;
  }, [request]);

  const refresh = useCallback(async (quiet = false) => {
    setRefreshing(true);
    try {
      const [nextSettings, nextProfiles] = await Promise.all([
        request<AppSettings>('get_settings'),
        request<ProfileSummary[]>('list_profiles'),
      ]);
      setSettings(nextSettings);
      setProfiles(nextProfiles);
      setEngineReady(true);
      const nextId = selectedId && nextProfiles.some((profile) => profile.id === selectedId)
        ? selectedId
        : nextProfiles[0]?.id ?? null;
      setSelectedId(nextId);
      if (nextId) await loadProfile(nextId);
      else setSelected(null);
    } catch (error) {
      setEngineReady(false);
      if (!quiet) notify(error instanceof Error ? error.message : '刷新失败', 'error');
    } finally {
      setRefreshing(false);
    }
  }, [loadProfile, notify, request, selectedId]);

  const handleRustEvent = useCallback((event: { event: string; payload: unknown }) => {
    if (event.event === 'core_exit') {
      setEngineReady(false);
      const payload = event.payload as { message?: string };
      notify(payload.message || 'Rust Core 已退出', 'error');
      return;
    }
    if (event.event !== 'proxy_event') return;
    const proxyEvent = event.payload as ProxyEvent;
    setEngineReady(true);
    setProfiles((current) => current.map((profile) => {
      if (profile.id !== proxyEvent.profile_id) return profile;
      return {
        ...profile,
        state: proxyEvent.state,
        endpoints: proxyEvent.endpoints.length > 0 ? proxyEvent.endpoints : profile.endpoints,
        error: proxyEvent.state === 'Error' ? proxyEvent.message : null,
      };
    }));
    setSelected((current) => {
      if (!current || current.id !== proxyEvent.profile_id) return current;
      return {
        ...current,
        state: proxyEvent.state,
        endpoints: proxyEvent.endpoints.length > 0 ? proxyEvent.endpoints : current.endpoints,
        error: proxyEvent.state === 'Error' ? proxyEvent.message : null,
      };
    });
    if (proxyEvent.kind === 'HostKey' && proxyEvent.fingerprint) {
      setHostKey({
        id: proxyEvent.profile_id,
        fingerprint: proxyEvent.fingerprint,
        message: proxyEvent.message || '首次连接需要确认 SSH 主机密钥',
      });
    }
    if (proxyEvent.kind === 'Log' || proxyEvent.message) {
      setLogs((current) => [
        {
          id: `${Date.now()}-${Math.random()}`,
          time: nowTime(),
          state: proxyEvent.state,
          message: proxyEvent.message || '',
        },
        ...current.slice(0, 99),
      ]);
    }
  }, [notify]);

  useEffect(() => {
    const unsubscribe = bridge()?.onRustEvent(handleRustEvent);
    void refresh(true);
    return unsubscribe;
  }, [handleRustEvent, refresh]);

  const selectedSummary = profiles.find((profile) => profile.id === selectedId) ?? null;

  const handleSelect = async (id: string) => {
    setSelectedId(id);
    try {
      await loadProfile(id);
    } catch (error) {
      notify(error instanceof Error ? error.message : '读取代理配置失败', 'error');
    }
  };

  const handleToggle = async () => {
    if (!selected) return;
    try {
      if (isActive(selected.state)) {
        await request('stop_profile', { id: selected.id });
        notify(`已停止 ${selected.name}`, 'info');
      } else {
        await request('start_profile', { id: selected.id });
        notify(`正在启动 ${selected.name}`, 'success');
      }
      await refresh(true);
    } catch (error) {
      notify(error instanceof Error ? error.message : '操作失败', 'error');
      await refresh(true);
    }
  };

  const handleSave = async (draft: ProfileDraft) => {
    try {
      const saved = await request<ProfileView>('save_profile', draft);
      setEditor(undefined);
      setSelectedId(saved.id);
      setSelected(saved);
      notify('代理配置已保存', 'success');
      await refresh(true);
    } catch (error) {
      notify(error instanceof Error ? error.message : '保存配置失败', 'error');
      throw error;
    }
  };

  const handleDelete = async () => {
    if (!selected || !window.confirm(`确定删除“${selected.name}”吗？`)) return;
    try {
      await request('delete_profile', { id: selected.id });
      notify('代理配置已删除', 'info');
      await refresh(true);
    } catch (error) {
      notify(error instanceof Error ? error.message : '删除配置失败', 'error');
    }
  };

  const handleHostKey = async (accepted: boolean) => {
    if (!hostKey) return;
    const current = hostKey;
    setHostKey(null);
    try {
      await request('confirm_host_key', {
        id: current.id,
        fingerprint: current.fingerprint,
        accepted,
      });
      if (accepted) notify('已保存 SSH 主机密钥并继续连接', 'success');
      else notify('已拒绝 SSH 主机密钥', 'info');
    } catch (error) {
      notify(error instanceof Error ? error.message : '主机密钥确认失败', 'error');
    }
  };

  return (
    <div className="app-shell">
      <Titlebar
        engineReady={engineReady}
        refreshing={refreshing}
        onRefresh={() => void refresh()}
        onOpenSettings={() => setSettingsOpen(true)}
      />
      <div className="app-body">
        <Sidebar
          profiles={profiles}
          selectedId={selectedId}
          onSelect={handleSelect}
          onCreate={() => setTypePickerOpen(true)}
        />
        <main className="content">
          <div className="content-inner">
            {selected ? (
              <DetailView
                profile={selected}
                summary={selectedSummary}
                settings={settings}
                logs={logs.filter((log) => !selectedId || log.message.length > 0)}
                onToggle={() => void handleToggle()}
                onEdit={() => setEditor(selected)}
                onDelete={() => void handleDelete()}
                onCopy={async (text) => {
                  const copied = await copyText(text);
                  notify(copied ? `已复制 ${text}` : '复制失败，请手动复制', copied ? 'success' : 'error');
                }}
                onClearLogs={() => setLogs([])}
              />
            ) : (
              <EmptyState onCreate={() => setTypePickerOpen(true)} engineReady={engineReady} />
            )}
          </div>
        </main>
      </div>
      {editor !== undefined && (
        <ProfileEditor
          key={editor?.id ?? 'new'}
          initial={editor}
          initialKind={editor?.kind ?? editorKind}
          settings={settings}
          onClose={() => setEditor(undefined)}
          onSave={handleSave}
          onNotify={notify}
        />
      )}
      {typePickerOpen && (
        <TypePickerModal
          onClose={() => setTypePickerOpen(false)}
          onChoose={(kind) => {
            setTypePickerOpen(false);
            setEditorKind(kind);
            setEditor(null);
          }}
        />
      )}
      {settingsOpen && (
        <SettingsModal
          initial={settings}
          onClose={() => setSettingsOpen(false)}
          onSaved={(next) => {
            setSettings(next);
            setSettingsOpen(false);
            notify('应用设置已保存', 'success');
          }}
          request={request}
          onNotify={notify}
        />
      )}
      {hostKey && <HostKeyModal request={hostKey} onConfirm={handleHostKey} />}
      <ToastStack toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
    </div>
  );
}

function Titlebar({
  engineReady,
  refreshing,
  onRefresh,
  onOpenSettings,
}: {
  engineReady: boolean;
  refreshing: boolean;
  onRefresh: () => void;
  onOpenSettings: () => void;
}) {
  const currentBridge = bridge();
  return (
    <header className="titlebar">
      <div className="titlebar-left">
        <div className="traffic-lights" aria-label="窗口控制">
          <button className="traffic-light close" onClick={() => currentBridge?.closeWindow()} aria-label="关闭" />
          <button className="traffic-light minimize" onClick={() => currentBridge?.minimizeWindow()} aria-label="最小化" />
          <button className="traffic-light maximize" onClick={() => currentBridge?.maximizeWindow()} aria-label="最大化" />
        </div>
        <div className="brand-lockup">
          <AppLogo />
          <span className="brand-name">LocalProxy</span>
          <div className="brand-badges">
            <span className="brand-badge electron">Electron UI</span>
            <span className="brand-badge rust">Rust Core</span>
          </div>
        </div>
      </div>
      <div className="engine-pill">
        <span className={`engine-dot${engineReady ? '' : ' offline'}`} />
        {engineReady ? 'Rust Tokio · IPC 已连接' : 'Rust Core 未连接'}
      </div>
      <div className="titlebar-actions">
        <button className="ghost-button" onClick={onOpenSettings} title="应用级 HTTP / SOCKS5 出站代理设置">
          <Settings size={14} />
          应用设置
        </button>
        <button className="icon-button" onClick={onRefresh} title="刷新配置与运行状态" disabled={refreshing}>
          <RefreshCw size={17} className={refreshing ? 'spinner' : ''} />
        </button>
      </div>
    </header>
  );
}

function Sidebar({
  profiles,
  selectedId,
  onSelect,
  onCreate,
}: {
  profiles: ProfileSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
}) {
  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <div className="sidebar-heading">
          <h2>
            代理配置 <span className="count-badge">{profiles.length}</span>
          </h2>
          <button className="add-button" onClick={onCreate} title="新建代理配置">
            <Plus size={20} />
          </button>
        </div>
        <div className="loopback-badge">
          <Lock size={15} />
          <span>127.0.0.1 · 仅本机可访问</span>
        </div>
      </div>
      <div className="sidebar-list">
        {profiles.length === 0 ? (
          <div className="empty-sidebar">
            <strong>还没有代理配置</strong>
            <span>点击右上角 + 开始创建</span>
          </div>
        ) : (
          profiles.map((profile) => (
            <button
              className={`profile-card${profile.id === selectedId ? ' selected' : ''}`}
              key={profile.id}
              onClick={() => onSelect(profile.id)}
            >
              <div className="profile-title-row">
                <div>
                  <div className="profile-title">{profile.name}</div>
                  <div className="profile-type">
                    {profile.kind === 'Http' ? <Globe size={14} /> : profile.kind === 'WireGuard' ? <ShieldCheck size={14} /> : profile.kind === 'Wstunnel' ? <Link2 size={14} /> : <Terminal size={14} />}
                    {kindLabel(profile.kind)}
                  </div>
                </div>
                <span className={`state-dot ${stateClass(profile.state)}`} />
              </div>
              <div className="profile-endpoint-row">
                <Link2 size={13} />
                <span>{endpointForSummary(profile)}</span>
              </div>
              {profile.tags.length > 0 && (
                <div className="profile-tags">
                  {profile.tags.map((tag) => <span className="tag" key={tag}>{tag}</span>)}
                </div>
              )}
            </button>
          ))
        )}
      </div>
      <div className="sidebar-footer">
        <div className="daemon-state"><ShieldCheck size={15} color="#219653" /><span>Rust Core 私有通道</span></div>
        <span>NDJSON IPC</span>
      </div>
    </aside>
  );
}

function EmptyState({ onCreate, engineReady }: { onCreate: () => void; engineReady: boolean }) {
  return (
    <div className="empty-state">
      <AppLogo large />
      <h2>{engineReady ? '开始管理本地代理' : '等待 Rust Core 连接'}</h2>
      <p>{engineReady ? '创建 HTTP 反向代理、SSH 或 Wstunnel 本地端口转发。所有服务只监听 127.0.0.1，实际端口会在启动后同步显示。' : '请从 Electron 桌面端启动，并确认 Rust Core 已完成编译。'}</p>
      {engineReady && <div className="empty-actions"><button className="primary-button" onClick={onCreate}><Plus size={15} />新建代理配置</button></div>}
    </div>
  );
}

function DetailView({
  profile,
  summary,
  settings,
  logs,
  onToggle,
  onEdit,
  onDelete,
  onCopy,
  onClearLogs,
}: {
  profile: ProfileView;
  summary: ProfileSummary | null;
  settings: AppSettings;
  logs: LogEntry[];
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onCopy: (text: string) => Promise<void>;
  onClearLogs: () => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [wgStatus, setWgStatus] = useState<WireGuardStatus | null>(null);
  const endpoints = endpointForProfile(profile);
  const active = isActive(profile.state);
  const proxy = profile.kind === 'Http' ? settings.http_proxy : settings.socks_proxy;

  const copyEndpoint = async (address: string) => {
    await onCopy(address);
    setCopied(address);
    window.setTimeout(() => setCopied((current) => current === address ? null : current), 1600);
  };

  const sshCommand = profile.ssh
    ? buildSshCommand(profile.ssh, settings.socks_proxy)
    : '';

  useEffect(() => {
    if (profile.kind !== 'WireGuard' || !isActive(profile.state)) {
      setWgStatus(null);
      return;
    }
    let cancelled = false;
    const load = async () => {
      try {
        const status = await bridge()?.request<WireGuardStatus | null>('wireguard_status', { id: profile.id });
        if (!cancelled && status) setWgStatus(status);
      } catch {
        if (!cancelled) setWgStatus(null);
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [profile.id, profile.kind, profile.state]);

  return (
    <>
      <section className="detail-header">
        <div className="selected-profile-title">
          <span className="detail-icon">{profile.kind === 'Http' ? <Globe size={24} /> : profile.kind === 'WireGuard' ? <ShieldCheck size={24} /> : <Terminal size={24} />}</span>
          <div className="detail-heading-copy">
            <h1>{profile.name}</h1>
            <div className="detail-subtitle">
              <span className={`status-badge ${stateClass(profile.state)}`}>
                <span className={`status-dot ${stateClass(profile.state)}`} />
                {stateLabel(profile.state)}
              </span>
              <span className="type-pill">{kindLabel(profile.kind)}</span>
              {profile.tags.map((tag) => <span className="tag" key={tag}><Tag size={11} />{tag}</span>)}
            </div>
          </div>
        </div>
        <div className="detail-actions">
          <button className={active ? 'danger-button' : 'primary-button'} onClick={onToggle} disabled={!bridge()}>
            {active ? <><Square size={14} fill="currentColor" />停止</> : <><Play size={14} fill="currentColor" />启动</>}
          </button>
          <button className="outline-button" onClick={onEdit} disabled={!bridge()}><Pencil size={14} />编辑</button>
          <div style={{ position: 'relative' }}>
            <button className="icon-button" onClick={() => setMenuOpen((open) => !open)} title="更多操作"><MoreHorizontal size={19} /></button>
            {menuOpen && (
              <div className="floating-menu" onMouseLeave={() => setMenuOpen(false)}>
                {sshCommand && <button onClick={() => { void copyEndpoint(sshCommand); setMenuOpen(false); }}><Clipboard size={14} />复制 SSH 命令</button>}
                {endpoints[0] && <button onClick={() => { void copyEndpoint(formatAddress(endpoints[0].host, endpoints[0].port)); setMenuOpen(false); }}><Copy size={14} />复制第一个端点</button>}
                <button className="menu-danger" onClick={() => { setMenuOpen(false); onDelete(); }}><Trash2 size={14} />删除配置</button>
              </div>
            )}
          </div>
        </div>
      </section>

      {profile.error && (
        <div className="error-banner"><AlertCircle size={17} /> <span>{profile.error}</span></div>
      )}

      <section className="surface-card">
        <div className="section-title">
          <div className="section-title-left"><span className="section-icon"><Layers size={15} /></span><div><h2>本地端点</h2><p>代理的具体调用地址，启动后会显示实际绑定端口</p></div></div>
          <div className="section-title-right"><CheckCircle2 size={14} color="#219653" />&nbsp;仅监听 127.0.0.1</div>
        </div>
        {profile.kind === 'Ssh' && profile.ssh?.use_system_proxy && (
          <RouteBanner ssh={profile.ssh} proxy={settings.socks_proxy} />
        )}
        {profile.kind === 'Http' && profile.http?.use_system_proxy && (
          <div className="route-banner">
            <div className="route-banner-header"><Globe size={15} />HTTP 上游连接将经过应用级 HTTP 代理</div>
            <div className="route-flow"><span className="route-step"><strong>LocalProxy</strong> 本地监听</span><span className="route-arrow">→</span><span className="route-step"><strong>HTTP Proxy</strong> {proxy ? formatAddress(proxy.host, proxy.port) : '未配置'}</span><span className="route-arrow">→</span><span className="route-step"><strong>上游服务</strong> {profile.http.upstream_url}</span></div>
          </div>
        )}
        <div className="endpoint-grid">
          {endpoints.length === 0 ? (
            <div className="empty-sidebar"><strong>暂无端点</strong><span>添加 SSH 转发规则后启动即可使用</span></div>
          ) : endpoints.map((endpoint, index) => {
            const address = formatAddress(endpoint.host, endpoint.port);
            const wsForward = profile.wstunnel?.forwards[index];
            const forward = profile.ssh?.forwards[index] ?? (wsForward ? { ...wsForward, tag: wsForward.protocol.toUpperCase() + ' 本地转发' } : undefined);
            const isCopied = copied === address;
            if (profile.kind === 'WireGuard') {
              const peer = profile.wireguard?.peers[index] ?? profile.wireguard?.peers[0];
              return (
                <div className="endpoint-card" key={`${endpoint.kind}-${index}-${endpoint.port}`}>
                  <div className="endpoint-card-top">
                    <div className="endpoint-kind"><span className="endpoint-kind-icon"><ShieldCheck size={15} /></span><div><strong>{endpoint.host}</strong><div className="endpoint-state">{stateLabel(profile.state)} · WireGuard 接口</div></div></div>
                  </div>
                  <div className="copyable-address"><span>{profile.wireguard?.addresses.join(', ') || '未配置地址'}</span></div>
                  <div className="endpoint-remote">对端 · {peer ? `${peer.endpoint_host}:${peer.endpoint_port}` : '未配置'}</div>
                  <div className="endpoint-meta"><span className="tag neutral">WireGuard</span>{peer?.persistent_keepalive != null && <span className="tag">Keepalive {peer.persistent_keepalive}s</span>}</div>
                </div>
              );
            }
            return (
              <div className="endpoint-card" key={`${endpoint.kind}-${index}-${endpoint.port}`} onClick={() => void copyEndpoint(address)}>
                <div className="endpoint-card-top">
                  <div className="endpoint-kind"><span className="endpoint-kind-icon">{profile.kind === 'Http' ? <Server size={15} /> : <Link2 size={15} />}</span><div><strong>{profile.kind === 'Http' ? 'HTTP 反向代理' : (forward?.tag || 'SSH 本地转发')}</strong><div className="endpoint-state">{stateLabel(profile.state)} · 点击卡片复制</div></div></div>
                  <button className="icon-button" onClick={(event) => { event.stopPropagation(); void copyEndpoint(address); }} title="复制本地端点">{isCopied ? <Check size={15} color="#219653" /> : <Copy size={15} />}</button>
                </div>
                <div className="copyable-address"><span>{profile.kind === 'Http' ? `http://${address}` : address}</span></div>
                <div className="endpoint-remote">{profile.kind === 'Http' ? `上游 · ${profile.http?.upstream_url ?? '未配置'}` : `远端目标 · ${formatAddress(forward?.remote_host ?? endpoint.host, forward?.remote_port ?? 0)}`}</div>
                <div className="endpoint-meta"><span className="tag neutral">{endpoint.kind === 'http' ? 'HTTP/1.1' : profile.wstunnel ? 'WebSocket' : 'direct-tcpip'}</span>{forward?.tag && <span className="tag">{forward.tag}</span>}</div>
              </div>
            );
          })}
        </div>
      </section>

      <section className="surface-card">
        <div className="section-title"><div className="section-title-left"><span className="section-icon"><Settings size={15} /></span><div><h2>连接配置</h2><p>敏感信息只显示状态，不返回或记录具体值</p></div></div><button className="small-button" onClick={onEdit}><Pencil size={12} />编辑配置</button></div>
        <div className="key-value-grid">
          {profile.kind === 'Http' && profile.http ? <>
            <KeyValue label="上游基础 URL" value={profile.http.upstream_url} mono />
            <KeyValue label="监听端口" value={profile.http.listen_port > 0 ? formatAddress('127.0.0.1', profile.http.listen_port) : '启动后分配'} mono />
            <KeyValue label="鉴权 Headers" value={`${profile.http.headers.length} 项（值已隐藏）`} />
            <KeyValue label="鉴权 Cookies" value={`${profile.http.cookies.length} 项（值已隐藏）`} />
            <KeyValue label="应用 HTTP 代理" value={profile.http.use_system_proxy ? (proxy ? formatAddress(proxy.host, proxy.port) : '未配置') : '未启用'} mono />
          </> : profile.kind === 'WireGuard' && profile.wireguard ? <>
            <KeyValue label="接口地址" value={profile.wireguard.addresses.join(', ')} mono />
            <KeyValue label="接口 MTU" value={profile.wireguard.mtu?.toString() ?? '系统默认'} mono />
            <KeyValue label="监听端口" value={profile.wireguard.listen_port ? profile.wireguard.listen_port.toString() : '随机端口'} mono />
            <KeyValue label="DNS" value={profile.wireguard.dns.join(', ') || '未修改'} mono />
            <KeyValue label="对端数量" value={`${profile.wireguard.peers.length} 个`} />
          </> : profile.ssh ? <>
            <KeyValue label="SSH 远端" value={`${profile.ssh.username}@${formatAddress(profile.ssh.host, profile.ssh.port)}`} mono />
            <KeyValue label="认证方式" value={profile.ssh.auth_mode === 'Password' ? (profile.ssh.password_configured ? '密码（已保存）' : '密码未配置') : '私钥文件'} />
            <KeyValue label="本地转发" value={`${profile.ssh.forwards.length} 条规则`} />
            <KeyValue label="应用 SOCKS5 代理" value={profile.ssh.use_system_proxy ? (proxy ? formatAddress(proxy.host, proxy.port) : '未配置') : '未启用'} mono />
          </> : profile.wstunnel ? <>
            <KeyValue label="Wstunnel 服务端" value={profile.wstunnel.server_url} mono />
            <KeyValue label="HTTP Upgrade 路径前缀" value={profile.wstunnel.http_upgrade_path_prefix} mono />
            <KeyValue label="本地转发" value={`${profile.wstunnel.forwards.length} 条规则`} />
            <KeyValue label="TLS 证书校验" value={profile.wstunnel.tls_verify_certificate ? '已启用' : '未启用（与原命令一致）'} />
          </> : null}
        </div>
        {profile.kind === 'WireGuard' && (
          <div className="wg-status-grid">
            <KeyValue label="实际接口" value={wgStatus?.interface_name ?? '启动后显示'} mono />
            <KeyValue label="本机公钥" value={wgStatus?.public_key ? `${wgStatus.public_key.slice(0, 12)}…` : '未连接'} mono />
            {(wgStatus?.peers ?? []).map((peer) => (
              <KeyValue
                key={peer.public_key}
                label={`对端 ${peer.public_key.slice(0, 8)}…`}
                value={`${peer.endpoint ?? '未连接'} · 接收 ${formatBytes(peer.rx_bytes)} · 发送 ${formatBytes(peer.tx_bytes)} · 握手 ${peer.handshake_age_sec == null ? '未完成' : `${peer.handshake_age_sec} 秒前`}`}
                mono
              />
            ))}
          </div>
        )}
      </section>

      <section className="surface-card log-card">
        <div className="section-title"><div className="section-title-left"><span className="section-icon"><List size={15} /></span><div><h2>运行日志</h2><p>来自 Rust Core 的脱敏状态事件</p></div></div><button className="small-button" onClick={onClearLogs}>清空</button></div>
        <div className="log-list">
          {logs.length === 0 ? <div className="empty-sidebar"><span>暂无运行日志</span></div> : logs.slice(0, 30).map((log) => <div className="log-row" key={log.id}><span className="log-time">{log.time}</span><span className="log-level">{stateLabel(log.state)}</span><span className="log-message" title={log.message}>{log.message}</span></div>)}
        </div>
      </section>
      {summary?.error && !profile.error && <div className="field-hint">{summary.error}</div>}
    </>
  );
}

function KeyValue({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return <div className="key-value"><span className="key">{label}</span><span className={`value${mono ? ' mono' : ''}`}>{value}</span></div>;
}

function RouteBanner({ ssh, proxy }: { ssh: NonNullable<ProfileView['ssh']>; proxy: ProxyEndpoint | null }) {
  return (
    <div className="route-banner">
      <div className="route-banner-header"><Wifi size={15} />SSH 连接链路</div>
      <div className="route-flow"><span className="route-step"><strong>LocalProxy</strong> 本地端口</span><span className="route-arrow">→</span><span className="route-step"><strong>SOCKS5</strong> {proxy ? formatAddress(proxy.host, proxy.port) : '未配置'}</span><span className="route-arrow">→</span><span className="route-step"><strong>SSH 会话</strong> {ssh.username}@{formatAddress(ssh.host, ssh.port)}</span><span className="route-arrow">→</span><span className="route-step"><strong>目标地址</strong> SSH direct-tcpip</span></div>
    </div>
  );
}

function buildSshCommand(ssh: NonNullable<ProfileView['ssh']>, socksProxy: ProxyEndpoint | null): string {
  const proxy = ssh.use_system_proxy && socksProxy
    ? `-o 'ProxyCommand=connect -S ${formatAddress(socksProxy.host, socksProxy.port)} %h %p' `
    : '';
  const forwards = ssh.forwards.map((forward) => `-L ${forward.local_port}:${forward.remote_host}:${forward.remote_port}`).join(' ');
  return `ssh -N ${proxy}${forwards} -p ${ssh.port} ${ssh.username}@${ssh.host}`.replace(/\s+/g, ' ').trim();
}

function ToastStack({ toasts, onDismiss }: { toasts: Toast[]; onDismiss: (id: number) => void }) {
  if (toasts.length === 0) return null;
  return <div className="toast-stack">{toasts.map((toast) => <div className={`toast ${toast.tone}`} key={toast.id}>{toast.tone === 'error' ? <AlertCircle size={16} /> : toast.tone === 'success' ? <CheckCircle2 size={16} /> : <Activity size={16} />}<span>{toast.message}</span><button onClick={() => onDismiss(toast.id)}><X size={14} /></button></div>)}</div>;
}

function TypePickerModal({ onClose, onChoose }: { onClose: () => void; onChoose: (kind: ProxyKind) => void }) {
  const options: Array<{ kind: ProxyKind; title: string; hint: string; icon: JSX.Element }> = [
    { kind: 'WireGuard', title: 'WireGuard 隧道', hint: 'wg-quick 导入 · utun · 路由和 DNS', icon: <ShieldCheck size={20} /> },
    { kind: 'Wstunnel', title: 'Wstunnel 隧道', hint: 'UDP / TCP over WebSocket', icon: <Link2 size={20} /> },
    { kind: 'Http', title: 'HTTP 反向代理', hint: '上游替换与 Header / Cookie 注入', icon: <Globe size={20} /> },
    { kind: 'Ssh', title: 'SSH 端口转发', hint: '本地端口 → SSH 远端', icon: <Terminal size={20} /> },
  ];
  return <div className="modal-backdrop"><div className="dialog type-picker-dialog" role="dialog" aria-modal="true"><div className="modal-heading"><div><h2>选择代理类型</h2><p>不同类型的配置卡片相互独立</p></div><button className="icon-button" onClick={onClose}><X size={18} /></button></div><div className="dialog-scroll type-grid">{options.map((option) => <button key={option.kind} className="type-card" onClick={() => onChoose(option.kind)}>{option.icon}<strong>{option.title}</strong><span>{option.hint}</span></button>)}</div></div></div>;
}

function ProfileEditor({
  initial,
  initialKind,
  settings,
  onClose,
  onSave,
  onNotify,
}: {
  initial: ProfileView | null;
  initialKind: ProxyKind;
  settings: AppSettings;
  onClose: () => void;
  onSave: (draft: ProfileDraft) => Promise<void>;
  onNotify: (message: string, tone?: Toast['tone']) => void;
}) {
  const initialHttp = initial?.http;
  const initialSsh = initial?.ssh;
  const initialWireGuard = initial?.wireguard;
  const [wsConfig, setWsConfig] = useState<WstunnelConfig>(initial?.wstunnel ?? {server_url: '', http_upgrade_path_prefix: 'v1', tls_verify_certificate: false, forwards: []});
  const [importCommand, setImportCommand] = useState('');
  const [importing, setImporting] = useState(false);
  const [wgConfig, setWgConfig] = useState<WireGuardConfig>(
    initialWireGuard ? {
      addresses: initialWireGuard.addresses,
      dns: initialWireGuard.dns,
      mtu: initialWireGuard.mtu,
      listen_port: initialWireGuard.listen_port,
      private_key: { name: 'private_key', value: null, kind: initialWireGuard.private_key_configured ? 'Keep' : 'Clear' },
      peers: initialWireGuard.peers,
    } : {
      addresses: [],
      dns: [],
      mtu: 1420,
      listen_port: null,
      private_key: { name: 'private_key', value: null, kind: 'Clear' },
      peers: [],
    },
  );
  const [wgConfigText, setWgConfigText] = useState('');
  const importWstunnel = async () => {
    const api = bridge();
    if (!api) return setFormError('请在 Electron 桌面端导入命令');
    setImporting(true); setFormError(null);
    try {
      const config = await api.request<WstunnelConfig>('parse_wstunnel_command', { command: importCommand });
      setWsConfig(config);
      if (!name.trim()) setName(`Wstunnel ${new URL(config.server_url).hostname}`);
      setImportCommand('');
    } catch (error) { setFormError(String(error)); } finally { setImporting(false); }
  };
  const importWireGuardConfig = async () => {
    const api = bridge();
    if (!api) return setFormError('请在 Electron 桌面端导入配置');
    setImporting(true); setFormError(null);
    try {
      const config = await api.request<WireGuardConfig>('parse_wg_config', { config: wgConfigText });
      setWgConfig(config);
      if (!name.trim()) setName('WireGuard 隧道');
      setWgConfigText('');
      onNotify('已导入 wg-quick 配置', 'success');
    } catch (error) {
      setFormError(error instanceof Error ? error.message : String(error));
    } finally {
      setImporting(false);
    }
  };
  const [kind] = useState<ProxyKind>(initialKind);
  const [name, setName] = useState(initial?.name ?? '');
  const [tags, setTags] = useState(initial?.tags.join(', ') ?? '');
  const [listenPort, setListenPort] = useState(initialHttp?.listen_port ?? 0);
  const [upstreamUrl, setUpstreamUrl] = useState(initialHttp?.upstream_url ?? '');
  const [httpUseProxy, setHttpUseProxy] = useState(initialHttp?.use_system_proxy ?? false);
  const [headers, setHeaders] = useState<SecretRow[]>(() => makeSecretRows(initialHttp?.headers, 'header'));
  const [cookies, setCookies] = useState<SecretRow[]>(() => makeSecretRows(initialHttp?.cookies, 'cookie'));
  const [removedHeaders, setRemovedHeaders] = useState<Set<string>>(() => new Set());
  const [removedCookies, setRemovedCookies] = useState<Set<string>>(() => new Set());
  const [sshHost, setSshHost] = useState(initialSsh?.host ?? '');
  const [sshPort, setSshPort] = useState(initialSsh?.port ?? 22);
  const [sshUsername, setSshUsername] = useState(initialSsh?.username ?? '');
  const [authMode, setAuthMode] = useState<AuthMode>(initialSsh?.auth_mode ?? 'Password');
  const [privateKeyPath, setPrivateKeyPath] = useState(initialSsh?.private_key_path ?? '');
  const [password, setPassword] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [passwordConfigured] = useState(initialSsh?.password_configured ?? false);
  const [passphraseConfigured] = useState(initialSsh?.passphrase_configured ?? false);
  const [sshUseProxy, setSshUseProxy] = useState(initialSsh?.use_system_proxy ?? false);
  const [forwardRows, setForwardRows] = useState<ForwardRowState[]>(() => (initialSsh?.forwards ?? []).map((forward, index) => ({ ...forward, id: `forward-${index}-${forward.local_port}` })));
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setFormError(null);
    const trimmedName = name.trim();
    if (!trimmedName) return setFormError('请填写代理名称');
    if (kind === 'Wstunnel') {
      setSaving(true);
      try { await onSave({ id: initial?.id ?? null, name: trimmedName, tags: tags.split(/[\n,]/).map(t => t.trim()).filter(Boolean), kind, http: null, ssh: null, wstunnel: wsConfig }); }
      catch { /* parent displays save errors */ } finally { setSaving(false); }
      return;
    }
    if (kind === 'WireGuard') {
      if (wgConfig.addresses.length === 0) return setFormError('请导入或填写接口 Address');
      if (!wgConfig.private_key.value && wgConfig.private_key.kind !== 'Keep') return setFormError('请填写 PrivateKey');
      if (wgConfig.peers.length === 0) return setFormError('至少需要一个 [Peer]');
      setSaving(true);
      try {
        await onSave({
          id: initial?.id ?? null,
          name: trimmedName,
          tags: tags.split(/[\n,]/).map(t => t.trim()).filter(Boolean),
          kind,
          http: null,
          ssh: null,
          wstunnel: null,
          wireguard: wgConfig,
        });
      } finally {
        setSaving(false);
      }
      return;
    }
    if (kind === 'Http') {
      if (!upstreamUrl.trim()) return setFormError('请填写上游基础 URL');
      const headerResult = buildSecretPatches(headers, removedHeaders, 'Header');
      const cookieResult = buildSecretPatches(cookies, removedCookies, 'Cookie');
      if (headerResult.error || cookieResult.error) return setFormError(headerResult.error || cookieResult.error);
      const draft: ProfileDraft = {
        id: initial?.id ?? null,
        name: trimmedName,
        tags: tags.split(/[\n,]/).map((tag) => tag.trim()).filter(Boolean),
        kind,
        http: {
          listen_port: listenPort,
          upstream_url: upstreamUrl.trim(),
          headers: headerResult.patches,
          cookies: cookieResult.patches,
          use_system_proxy: httpUseProxy,
        },
        ssh: null,
      };
      setSaving(true);
      try { await onSave(draft); } catch { /* the parent presents the error toast */ } finally { setSaving(false); }
      return;
    }

    if (!sshHost.trim()) return setFormError('请填写 SSH 主机');
    if (!sshUsername.trim()) return setFormError('请填写 SSH 用户名');
    if (sshPort <= 0) return setFormError('SSH 端口必须大于 0');
    if (authMode === 'PrivateKey' && !privateKeyPath.trim()) return setFormError('私钥认证需要选择私钥文件');
    const forwards: ForwardDraft[] = [];
    for (const row of forwardRows) {
      const hasAnyValue = row.remote_host.trim() || row.remote_port > 0 || row.local_port > 0 || row.tag.trim();
      if (!hasAnyValue) continue;
      if (!row.remote_host.trim()) return setFormError('每条 SSH 转发规则都需要远端主机');
      if (row.remote_port <= 0) return setFormError(`转发规则 ${row.remote_host} 的远端端口必须大于 0`);
      forwards.push({ local_port: row.local_port, remote_host: row.remote_host.trim(), remote_port: row.remote_port, tag: row.tag.trim() });
    }
    const draft: ProfileDraft = {
      id: initial?.id ?? null,
      name: trimmedName,
      tags: tags.split(/[\n,]/).map((tag) => tag.trim()).filter(Boolean),
      kind,
      http: null,
      ssh: {
        host: sshHost.trim(),
        port: sshPort,
        username: sshUsername.trim(),
        auth_mode: authMode,
        private_key_path: privateKeyPath.trim(),
        password: { name: 'password', value: password || null, kind: password ? 'Set' : passwordConfigured ? 'Keep' : 'Clear' },
        passphrase: { name: 'passphrase', value: passphrase || null, kind: passphrase ? 'Set' : passphraseConfigured ? 'Keep' : 'Clear' },
        forwards,
        use_system_proxy: sshUseProxy,
      },
    };
    setSaving(true);
    try { await onSave(draft); } catch { /* the parent presents the error toast */ } finally { setSaving(false); }
  };

  const choosePrivateKey = async () => {
    const currentBridge = bridge();
    if (!currentBridge) return onNotify('请通过 Electron 桌面端选择私钥文件', 'error');
    const path = await currentBridge.choosePrivateKey();
    if (path) setPrivateKeyPath(path);
  };

  return (
    <div className="modal-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="profile-editor-title">
        <div className="modal-heading">
          <div><h2 id="profile-editor-title">{initial ? `编辑${kindLabel(kind)}` : `新建${kindLabel(kind)}`}</h2><p>每种代理类型使用独立配置卡片</p></div>
          <button className="icon-button" onClick={onClose} title="关闭"><X size={18} /></button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="dialog-scroll">
            {kind === 'Wstunnel' && (
              <section className="form-section">
                <div className="form-section-title"><h3>从命令快速导入</h3><span>粘贴 wstunnel client 命令，解析后可编辑并保存</span></div>
                <div className="form-field"><textarea aria-label="Wstunnel 命令" className="mono-input" rows={3} value={importCommand} onChange={e => setImportCommand(e.target.value)} placeholder={"wstunnel client --http-upgrade-path-prefix XXXX -L 'udp://50814:localhost:50814?timeout_sec=0' wss://18.123.135.89:443"} /></div>
                <button type="button" className="small-button" disabled={importing || !importCommand.trim()} onClick={() => void importWstunnel()}>{importing ? '解析中…' : '解析并填入配置'}</button>
              </section>
            )}
            <section className="form-section">
              <div className="form-section-title"><h3>基本信息</h3><span>可用标签帮助快速定位配置</span></div>
              <div className="form-grid">
                <div className="form-field"><label>代理名称</label><input value={name} onChange={(event) => setName(event.target.value)} placeholder="例如：生产 SSH 隧道" autoFocus /></div>
                <div className="form-field"><label>Tag <span>可选，逗号分隔</span></label><input value={tags} onChange={(event) => setTags(event.target.value)} placeholder="production, mysql" /></div>
              </div>
            </section>

            {kind === 'WireGuard' ? (
              <section className="form-section">
                <div className="form-section-title"><h3>从 wg-quick 快速导入</h3><span>粘贴 [Interface] / [Peer] 配置，私钥只写入本地私密文件</span></div>
                <div className="form-field"><textarea aria-label="WireGuard 配置" className="mono-input" rows={10} value={wgConfigText} onChange={e => setWgConfigText(e.target.value)} placeholder={"[Interface]\nPrivateKey = ...\nAddress = 192.168.1.6/32\nMTU = 1420\nDNS = 223.5.5.5\n\n[Peer]\nPublicKey = ...\nEndpoint = localhost:50814\nAllowedIPs = 172.16.110.0/24,172.31.0.2/32\nPersistentKeepalive = 25"} /></div>
                <button type="button" className="primary-button" disabled={importing || !wgConfigText.trim()} onClick={() => void importWireGuardConfig()}>{importing ? '导入中…' : '解析并导入'}</button>
              </section>
            ) : kind === 'Wstunnel' ? (
              <section className="form-section">
                <div className="form-section-title"><h3>Wstunnel 隧道</h3><span>内置 Rust 客户端 · UDP / TCP over WebSocket</span></div>
                <div className="form-grid">
                  <div className="form-field"><label>服务端 URL</label><input aria-label="Wstunnel 服务端" className="mono-input" value={wsConfig.server_url} onChange={e => setWsConfig({...wsConfig, server_url: e.target.value})} placeholder="wss://example.com:443" /></div>
                  <div className="form-field"><label>HTTP Upgrade 路径前缀</label><input aria-label="Upgrade 路径前缀" className="mono-input" value={wsConfig.http_upgrade_path_prefix} onChange={e => setWsConfig({...wsConfig, http_upgrade_path_prefix: e.target.value})} /></div>
                </div>
                <label><input type="checkbox" checked={wsConfig.tls_verify_certificate} onChange={e => setWsConfig({...wsConfig, tls_verify_certificate: e.target.checked})} /> 校验 TLS 证书</label>
                <p className="field-hint">导入时保留命令的证书校验选项。UDP 超时为 0 表示永不超时，留空使用默认 30 秒。</p>
                {wsConfig.forwards.map((f, index) => {
                  const update = (patch: Partial<typeof f>) => setWsConfig({...wsConfig, forwards: wsConfig.forwards.map((row, i) => i === index ? {...row, ...patch} : row)});
                  return <div className="form-section" key={index}>
                    <div className="form-grid three">
                      <div className="form-field"><label>协议</label><select value={f.protocol} onChange={e => update({protocol: e.target.value, timeout_sec: null})}><option value="udp">UDP</option><option value="tcp">TCP</option></select></div>
                      <div className="form-field"><label>本地端口</label><input type="number" min="1" max="65535" value={f.local_port} onChange={e => update({local_port: Number(e.target.value)})} /></div>
                      <div className="form-field"><label>远端端口</label><input type="number" min="1" max="65535" value={f.remote_port} onChange={e => update({remote_port: Number(e.target.value)})} /></div>
                      <div className="form-field"><label>远端主机</label><input value={f.remote_host} onChange={e => update({remote_host: e.target.value})} /></div>
                      {f.protocol === 'udp' && <div className="form-field"><label>UDP 超时（秒）</label><input type="number" min="0" max="4294967295" placeholder="30" value={f.timeout_sec ?? ''} onChange={e => update({timeout_sec: e.target.value === '' ? null : Number(e.target.value)})} /></div>}
                    </div>
                    <button type="button" className="small-button" onClick={() => setWsConfig({...wsConfig, forwards: wsConfig.forwards.filter((_, i) => i !== index)})}>删除规则</button>
                  </div>;
                })}
                <button type="button" className="small-button" onClick={() => setWsConfig({...wsConfig, forwards: [...wsConfig.forwards, {protocol: 'udp', local_port: 50814, remote_host: 'localhost', remote_port: 50814, timeout_sec: 0}]})}><Plus size={14} />添加转发规则</button>
              </section>
            ) : kind === 'Http' ? (
              <section className="form-section">
                <div className="form-section-title"><h3>HTTP 转发</h3><span>支持 HTTP/HTTPS 上游与流式请求体</span></div>
                <div className="form-grid">
                  <div className="form-field"><label>本地监听端口</label><input className="mono-input" type="number" min="0" max="65535" value={listenPort} onChange={(event) => setListenPort(Number(event.target.value))} /><div className="field-hint">填写 0 由系统分配随机空闲端口</div></div>
                  <div className="form-field"><label>上游基础 URL</label><input className="mono-input" value={upstreamUrl} onChange={(event) => setUpstreamUrl(event.target.value)} placeholder="https://service.example.com" /><div className="field-hint">本地路径和 query 会追加到上游地址</div></div>
                </div>
                <div className="proxy-toggle-card"><div className="switch-row"><div className="switch-copy"><Globe size={16} /><div><strong>经过应用级 HTTP 代理</strong><span>{settings.http_proxy ? `当前代理：${formatAddress(settings.http_proxy.host, settings.http_proxy.port)}` : '请先在应用设置中配置 HTTP 代理'}</span></div></div><label className="switch"><input type="checkbox" checked={httpUseProxy} onChange={(event) => setHttpUseProxy(event.target.checked)} /><span /></label></div></div>
                <SecretEditor title="Headers" hint="值只写入本地私密文件；客户端同名 header 会被覆盖" rows={headers} setRows={setHeaders} removed={removedHeaders} setRemoved={setRemovedHeaders} label="Header" onNotify={onNotify} />
                <SecretEditor title="Cookies" hint="按 cookie 名称合并，客户端同名 cookie 会被覆盖" rows={cookies} setRows={setCookies} removed={removedCookies} setRemoved={setRemovedCookies} label="Cookie" onNotify={onNotify} />
              </section>
            ) : (
              <section className="form-section">
                <div className="form-section-title"><h3>SSH 连接</h3><span>支持密码或私钥认证，域名会由 SSH 服务器解析</span></div>
                <div className="form-grid three">
                  <div className="form-field"><label>SSH 主机</label><input className="mono-input" value={sshHost} onChange={(event) => setSshHost(event.target.value)} placeholder="bastion.example.com" /></div>
                  <div className="form-field"><label>端口</label><input className="mono-input" type="number" min="1" max="65535" value={sshPort} onChange={(event) => setSshPort(Number(event.target.value))} /></div>
                  <div className="form-field"><label>用户名</label><input className="mono-input" value={sshUsername} onChange={(event) => setSshUsername(event.target.value)} placeholder="deploy" /></div>
                </div>
                <div className="secret-section">
                  <div className="secret-heading"><strong>认证方式</strong><span>密码、私钥口令不会进入配置文件或日志</span></div>
                  <div className="segment-control"><button type="button" className={authMode === 'Password' ? 'active' : ''} onClick={() => setAuthMode('Password')}><KeyRound size={14} />密码</button><button type="button" className={authMode === 'PrivateKey' ? 'active' : ''} onClick={() => setAuthMode('PrivateKey')}><Lock size={14} />私钥</button></div>
                  {authMode === 'Password' ? <div className="form-field" style={{ marginTop: 12 }}><label>SSH 密码 {passwordConfigured && <span className="configured-badge"><Check size={11} />已保存，留空保持</span>}</label><input type="password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder={passwordConfigured ? '留空保持现有密码' : '请输入密码'} autoComplete="new-password" /></div> : <>
                    <div className="form-field" style={{ marginTop: 12 }}><label>私钥文件路径</label><div className="field-with-action"><input className="mono-input" value={privateKeyPath} onChange={(event) => setPrivateKeyPath(event.target.value)} placeholder="~/.ssh/id_ed25519" /><button type="button" className="small-button" onClick={() => void choosePrivateKey()}>选择文件</button></div></div>
                    <div className="form-field" style={{ marginTop: 12 }}><label>私钥口令 {passphraseConfigured && <span className="configured-badge"><Check size={11} />已保存，留空保持</span>}</label><input type="password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} placeholder={passphraseConfigured ? '留空保持现有口令' : '没有口令可留空'} autoComplete="new-password" /></div>
                  </>}
                </div>
                <div className="proxy-toggle-card"><div className="switch-row"><div className="switch-copy"><Wifi size={16} /><div><strong>通过应用级 SOCKS5 代理连接 SSH</strong><span>{settings.socks_proxy ? `当前代理：${formatAddress(settings.socks_proxy.host, settings.socks_proxy.port)}；不提供 SOCKS5 出口` : '请先在应用设置中配置 SOCKS5 代理'}</span></div></div><label className="switch"><input type="checkbox" checked={sshUseProxy} onChange={(event) => setSshUseProxy(event.target.checked)} /><span /></label></div></div>
                <ForwardEditor rows={forwardRows} setRows={setForwardRows} />
              </section>
            )}
            {formError && <div className="error-banner"><AlertCircle size={16} /><span>{formError}</span></div>}
          </div>
          <div className="modal-footer"><button type="button" className="outline-button" onClick={onClose}>取消</button><button type="submit" className="primary-button" disabled={saving}>{saving ? <RefreshCw size={14} className="spinner" /> : <Save size={14} />}{saving ? '保存中…' : '保存配置'}</button></div>
        </form>
      </div>
    </div>
  );
}

function SecretEditor({
  title,
  hint,
  rows,
  setRows,
  removed,
  setRemoved,
  label,
  onNotify,
}: {
  title: string;
  hint: string;
  rows: SecretRow[];
  setRows: Dispatch<SetStateAction<SecretRow[]>>;
  removed: Set<string>;
  setRemoved: Dispatch<SetStateAction<Set<string>>>;
  label: string;
  onNotify: (message: string, tone?: Toast['tone']) => void;
}) {
  const [batchOpen, setBatchOpen] = useState(false);
  const [batchText, setBatchText] = useState('');
  const addRow = () => setRows((current) => [...current, { id: `${label}-${Date.now()}-${current.length}`, name: '', value: '', configured: false }]);
  const removeRow = (row: SecretRow) => {
    setRows((current) => current.filter((item) => item.id !== row.id));
    if (row.configured && row.originalName) setRemoved((current) => new Set(current).add(row.originalName!));
  };
  const applyBatch = () => {
    const pairs = parseBatchPairs(batchText);
    if (pairs.length === 0) return onNotify('没有识别到可用的 name/value 内容', 'error');
    setRows((current) => {
      const next = [...current];
      for (const pair of pairs) {
        const index = next.findIndex((row) => row.name.trim().toLowerCase() === pair.name.toLowerCase());
        if (index >= 0) next[index] = { ...next[index], value: pair.value };
        else next.push({ id: `${label}-${Date.now()}-${next.length}`, name: pair.name, value: pair.value, configured: false });
      }
      return next;
    });
    setRemoved((current) => {
      const next = new Set(current);
      for (const pair of pairs) next.delete(pair.name);
      return next;
    });
    setBatchText('');
    setBatchOpen(false);
    onNotify(`已解析 ${pairs.length} 个${label}`, 'success');
  };
  return <div className="secret-section"><div className="secret-heading"><div><strong>{title}</strong><span className="configured-badge"><Check size={11} />{rows.filter((row) => row.configured).length} 项已保存</span></div><span>{hint}</span></div>{rows.length === 0 && <div className="field-hint">暂无配置项</div>}{rows.map((row) => <div className="secret-row" key={row.id}><input value={row.name} onChange={(event) => setRows((current) => current.map((item) => item.id === row.id ? { ...item, name: event.target.value } : item))} placeholder={`${label}名称`} /><input value={row.value} onChange={(event) => setRows((current) => current.map((item) => item.id === row.id ? { ...item, value: event.target.value } : item))} placeholder={row.configured ? '已保存，留空保持' : `${label}值`} type={label === 'Cookie' || label === 'Header' ? 'text' : 'text'} /><button type="button" className="remove-row" onClick={() => removeRow(row)} title="移除"><Trash2 size={14} /></button></div>)}<button type="button" className="add-row-button" onClick={addRow}><Plus size={13} />添加{label}</button><button type="button" className="batch-toggle" onClick={() => setBatchOpen((open) => !open)}>{batchOpen ? '收起批量解析' : '批量粘贴解析'}</button>{batchOpen && <div className="batch-box"><label className="batch-label">粘贴格式：name: value，或浏览器复制的“名称 / 值”交替文本</label><textarea value={batchText} onChange={(event) => setBatchText(event.target.value)} placeholder={'sec-fetch-dest\nscript\nsec-fetch-mode\nno-cors'} /><div className="batch-actions"><button type="button" className="small-button" onClick={applyBatch}><Clipboard size={12} />解析并添加</button></div></div>}</div>;
}

function ForwardEditor({ rows, setRows }: { rows: ForwardRowState[]; setRows: Dispatch<SetStateAction<ForwardRowState[]>> }) {
  const addRow = () => setRows((current) => [...current, { ...DEFAULT_FORWARD, id: `forward-${Date.now()}-${current.length}` }]);
  const removeRow = (id: string) => setRows((current) => current.filter((row) => row.id !== id));
  const update = (id: string, field: keyof ForwardDraft, value: string | number) => setRows((current) => current.map((row) => row.id === id ? { ...row, [field]: value } : row));
  return <div className="secret-section"><div className="secret-heading"><strong>本地端口转发规则</strong><span>远端主机支持域名、IPv4 和 IPv6</span></div><div className="forward-header"><span>本地端口</span><span>远端主机</span><span>远端端口</span><span>Tag</span><span /></div>{rows.length === 0 && <div className="field-hint">暂无规则；启动 SSH 前至少添加一条转发规则</div>}{rows.map((row) => <div className="forward-row" key={row.id}><input className="mono-input" type="number" min="0" max="65535" value={row.local_port} onChange={(event) => update(row.id, 'local_port', Number(event.target.value))} placeholder="0=随机" /><input className="mono-input" value={row.remote_host} onChange={(event) => update(row.id, 'remote_host', event.target.value)} placeholder="db.internal.example" /><input className="mono-input" type="number" min="1" max="65535" value={row.remote_port} onChange={(event) => update(row.id, 'remote_port', Number(event.target.value))} placeholder="3306" /><input value={row.tag} onChange={(event) => update(row.id, 'tag', event.target.value)} placeholder="mysql-prod" /><button type="button" className="remove-row" onClick={() => removeRow(row.id)} title="移除规则"><Trash2 size={14} /></button></div>)}<button type="button" className="add-row-button" onClick={addRow}><Plus size={13} />添加转发规则</button></div>;
}

function SettingsModal({
  initial,
  onClose,
  onSaved,
  request,
  onNotify,
}: {
  initial: AppSettings;
  onClose: () => void;
  onSaved: (settings: AppSettings) => void;
  request: <T>(method: string, params?: unknown) => Promise<T>;
  onNotify: (message: string, tone?: Toast['tone']) => void;
}) {
  const [httpEnabled, setHttpEnabled] = useState(initial.http_proxy !== null);
  const [socksEnabled, setSocksEnabled] = useState(initial.socks_proxy !== null);
  const [httpHost, setHttpHost] = useState(initial.http_proxy?.host ?? '127.0.0.1');
  const [httpPort, setHttpPort] = useState(initial.http_proxy?.port ?? 8099);
  const [socksHost, setSocksHost] = useState(initial.socks_proxy?.host ?? '127.0.0.1');
  const [socksPort, setSocksPort] = useState(initial.socks_proxy?.port ?? 8099);
  const [saving, setSaving] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (httpEnabled && (!httpHost.trim() || httpPort <= 0)) return onNotify('请填写有效的 HTTP 代理主机和端口', 'error');
    if (socksEnabled && (!socksHost.trim() || socksPort <= 0)) return onNotify('请填写有效的 SOCKS5 代理主机和端口', 'error');
    setSaving(true);
    try {
      const next = await request<AppSettings>('save_settings', {
        http_proxy: httpEnabled ? { host: httpHost.trim(), port: httpPort } : null,
        socks_proxy: socksEnabled ? { host: socksHost.trim(), port: socksPort } : null,
      });
      onSaved(next);
    } catch (error) {
      onNotify(error instanceof Error ? error.message : '保存应用设置失败', 'error');
    } finally {
      setSaving(false);
    }
  };

  return <div className="modal-backdrop"><div className="dialog settings-dialog" role="dialog" aria-modal="true"><div className="modal-heading"><div><h2>应用设置</h2><p>集中配置外部代理；每个 HTTP / SSH profile 可单独决定是否使用</p></div><button className="icon-button" onClick={onClose}><X size={18} /></button></div><form onSubmit={submit}><div className="dialog-scroll"><div className="settings-intro"><strong>连接路径说明</strong><br />这里配置的是 LocalProxy 连接上游时使用的代理，不会额外开启 SOCKS5 出口。SSH 勾选“经过应用级 SOCKS5”后，链路为 LocalProxy → SOCKS5 → SSH 会话 → 目标地址。</div><div className="proxy-setting"><div className="proxy-setting-header"><div className="proxy-setting-title"><Globe size={15} />HTTP 代理</div><label className="switch"><input type="checkbox" checked={httpEnabled} onChange={(event) => setHttpEnabled(event.target.checked)} /><span /></label></div><span>用于 HTTP profile 访问 HTTP/HTTPS 上游</span>{httpEnabled && <div className="proxy-endpoint-fields" style={{ marginTop: 11 }}><input className="mono-input" value={httpHost} onChange={(event) => setHttpHost(event.target.value)} placeholder="127.0.0.1" /><input className="mono-input" type="number" min="1" max="65535" value={httpPort} onChange={(event) => setHttpPort(Number(event.target.value))} placeholder="8080" /></div>}</div><div className="proxy-setting"><div className="proxy-setting-header"><div className="proxy-setting-title"><Wifi size={15} />SOCKS5 代理</div><label className="switch"><input type="checkbox" checked={socksEnabled} onChange={(event) => setSocksEnabled(event.target.checked)} /><span /></label></div><span>用于 SSH profile 建立 SSH TCP 会话；支持无认证 SOCKS5</span>{socksEnabled && <div className="proxy-endpoint-fields" style={{ marginTop: 11 }}><input className="mono-input" value={socksHost} onChange={(event) => setSocksHost(event.target.value)} placeholder="127.0.0.1" /><input className="mono-input" type="number" min="1" max="65535" value={socksPort} onChange={(event) => setSocksPort(Number(event.target.value))} placeholder="8099" /></div>}</div></div><div className="modal-footer"><button type="button" className="outline-button" onClick={onClose}>取消</button><button type="submit" className="primary-button" disabled={saving}>{saving ? <RefreshCw size={14} className="spinner" /> : <Save size={14} />}{saving ? '保存中…' : '保存设置'}</button></div></form></div></div>;
}

function HostKeyModal({ request, onConfirm }: { request: HostKeyRequest; onConfirm: (accepted: boolean) => Promise<void> }) {
  return <div className="modal-backdrop"><div className="dialog confirm-dialog" role="alertdialog" aria-modal="true"><div className="modal-heading"><div><h2>确认 SSH 主机密钥</h2><p>首次连接未知主机时需要人工确认</p></div><CircleHelp size={22} color="#b87812" /></div><div className="dialog-scroll"><div className="host-key-copy">{request.message}<br />请确认该指纹来自你信任的 SSH 主机。拒绝后不会保存。</div><div className="fingerprint">{request.fingerprint}</div><div className="field-hint">主机密钥保存后，如果后续指纹变化，连接会直接阻断，不会自动接受新密钥。</div></div><div className="confirm-actions"><button className="outline-button" onClick={() => void onConfirm(false)}>拒绝</button><button className="primary-button" onClick={() => void onConfirm(true)}><Check size={14} />信任并继续</button></div></div></div>;
}
