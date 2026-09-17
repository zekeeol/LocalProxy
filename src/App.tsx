import {
  type Dispatch,
  type FormEvent,
  type SetStateAction,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
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
import i18n, { changeLanguage, LANGUAGES, type LanguageCode } from './i18n';
import type {
  WstunnelConfig,
  WireGuardStatus,
  WireGuardConfig,
  AppSettings,
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
  SshConnection,
  SshConnectionDraft,
} from './types';

type Toast = { id: number; message: string; tone: 'success' | 'info' | 'error' };
type HostKeyRequest = { id: string; fingerprint: string; message: string };
type ForwardRowState = ForwardDraft & { id: string };
type SshConnectionEditorState = SshConnection & {
  editorKey: string;
  password: string;
  passphrase: string;
  isNew: boolean;
};

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

function formatAddress(host: string, port: number, t?: TFunction): string {
  const formattedHost = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
  if (port <= 0) return `${formattedHost}:${t ? t('common.randomPort') : i18n.t('common.randomPort')}`;
  return `${formattedHost}:${port}`;
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

function stateLabel(state: ServiceState, t: TFunction): string {
  switch (state) {
    case 'Running':
      return t('state.running');
    case 'Starting':
      return t('state.starting');
    case 'AwaitingHostKey':
      return t('state.awaiting');
    case 'Reconnecting':
      return t('state.reconnecting');
    case 'Error':
      return t('state.error');
    default:
      return t('state.stopped');
  }
}

function kindLabel(kind: ProxyKind, t: TFunction): string {
  return t(`kind.${kind.toLowerCase()}`);
}

function isActive(state: ServiceState): boolean {
  return state === 'Running' || state === 'Starting' || state === 'AwaitingHostKey' || state === 'Reconnecting';
}

function endpointForProfile(profile: ProfileView, t: TFunction): Endpoint[] {
  if (profile.endpoints.length > 0) return profile.endpoints;
  if (profile.wstunnel) return profile.wstunnel.forwards.map(f => ({kind: f.protocol, host: '127.0.0.1', port: f.local_port, description: `${f.protocol.toUpperCase()} → ${formatAddress(f.remote_host, f.remote_port, t)}`}));
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
        description: t('app.localHttpProxy'),
      },
    ];
  }
  return (profile.ssh?.forwards ?? []).map((forward) => ({
    kind: 'sshForward',
    host: '127.0.0.1',
    port: forward.local_port,
    description: `SSH → ${formatAddress(forward.remote_host, forward.remote_port, t)}`,
  }));
}

function endpointForSummary(profile: ProfileSummary, t: TFunction): string {
  const first = profile.endpoints[0];
  if (!first) return t('app.localAddressAfterStart');
  return formatAddress(first.host, first.port, t);
}

function nowTime(): string {
  return new Date().toLocaleTimeString(i18n.language || 'en', { hour12: false });
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
  t: TFunction,
): { patches: SecretPatch[]; error: string | null } {
  const patches: SecretPatch[] = [];
  for (const row of rows) {
    const name = row.name.trim();
    const value = row.value;
    if (!name && !value) continue;
    if (!name) return { patches: [], error: t('common.labelNameEmpty', { label }) };
    if (!row.configured && !value) return { patches: [], error: t('common.nameRequiredValue', { name }) };
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
  const { t } = useTranslation();
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selected, setSelected] = useState<ProfileView | null>(null);
  const [settings, setSettings] = useState<AppSettings>({ http_proxy: null, socks_proxy: null, ssh_connections: [] });
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
    if (!currentBridge) throw new Error(t('app.runInDesktop'));
    return currentBridge.request<T>(method, params);
  }, [t]);

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
      if (!quiet) notify(error instanceof Error ? error.message : t('common.refreshFailed'), 'error');
    } finally {
      setRefreshing(false);
    }
  }, [loadProfile, notify, request, selectedId, t]);

  const handleRustEvent = useCallback((event: { event: string; payload: unknown }) => {
    if (event.event === 'core_exit') {
      setEngineReady(false);
      const payload = event.payload as { message?: string };
      notify(payload.message || t('app.coreExited'), 'error');
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
        message: proxyEvent.message || t('app.hostKeyNeeded'),
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
  }, [notify, t]);

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
      notify(error instanceof Error ? error.message : t('app.loadProfileFailed'), 'error');
    }
  };

  const handleToggle = async () => {
    if (!selected) return;
    try {
      if (isActive(selected.state)) {
        await request('stop_profile', { id: selected.id });
        notify(t('app.stopped', { name: selected.name }), 'info');
      } else {
        await request('start_profile', { id: selected.id });
        notify(t('app.starting', { name: selected.name }), 'success');
      }
      await refresh(true);
    } catch (error) {
      notify(error instanceof Error ? error.message : t('common.operationFailed'), 'error');
      await refresh(true);
    }
  };

  const handleSave = async (draft: ProfileDraft) => {
    try {
      const saved = await request<ProfileView>('save_profile', draft);
      setEditor(undefined);
      setSelectedId(saved.id);
      setSelected(saved);
      notify(t('app.profileSaved'), 'success');
      await refresh(true);
    } catch (error) {
      notify(error instanceof Error ? error.message : t('app.saveFailed'), 'error');
      throw error;
    }
  };

  const handleDelete = async () => {
    if (!selected || !window.confirm(t('app.deleteConfirm', { name: selected.name }))) return;
    try {
      await request('delete_profile', { id: selected.id });
      notify(t('app.profileDeleted'), 'info');
      await refresh(true);
    } catch (error) {
      notify(error instanceof Error ? error.message : t('app.deleteFailed'), 'error');
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
      if (accepted) notify(t('app.hostKeySaved'), 'success');
      else notify(t('app.hostKeyRejected'), 'info');
    } catch (error) {
      notify(error instanceof Error ? error.message : t('app.hostKeyFailed'), 'error');
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
                  notify(copied ? t('common.copied', { value: text }) : t('common.copyFailed'), copied ? 'success' : 'error');
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
            notify(t('app.settingsSaved'), 'success');
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
  const { t, i18n } = useTranslation();
  const currentBridge = bridge();
  const currentLanguage = LANGUAGES.find((language) => i18n.language.startsWith(language.code))?.code ?? 'en';
  return (
    <header className="titlebar">
      <div className="titlebar-left">
        <div className="traffic-lights" aria-label={t('titlebar.windowControls')}>
          <button className="traffic-light close" onClick={() => currentBridge?.closeWindow()} aria-label={t('titlebar.closeWindow')} />
          <button className="traffic-light minimize" onClick={() => currentBridge?.minimizeWindow()} aria-label={t('titlebar.minimizeWindow')} />
          <button className="traffic-light maximize" onClick={() => currentBridge?.maximizeWindow()} aria-label={t('titlebar.maximizeWindow')} />
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
        {engineReady ? t('titlebar.engineConnected') : t('titlebar.engineOffline')}
      </div>
      <div className="titlebar-actions">
        <label className="ghost-button language-switch" title={t('titlebar.language')}>
          <Globe size={14} />
          <select
            value={currentLanguage}
            aria-label={t('titlebar.language')}
            onChange={(event) => changeLanguage(event.target.value as LanguageCode)}
          >
            {LANGUAGES.map((language) => (
              <option key={language.code} value={language.code}>{language.label}</option>
            ))}
          </select>
        </label>
        <button className="ghost-button" onClick={onOpenSettings} title={t('titlebar.appSettingsTitle')}>
          <Settings size={14} />
          {t('titlebar.appSettings')}
        </button>
        <button className="icon-button" onClick={onRefresh} title={t('titlebar.refreshTitle')} disabled={refreshing}>
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
  const { t } = useTranslation();
  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <div className="sidebar-heading">
          <h2>
            {t('sidebar.heading')} <span className="count-badge">{profiles.length}</span>
          </h2>
          <button className="add-button" onClick={onCreate} title={t('sidebar.addTitle')}>
            <Plus size={20} />
          </button>
        </div>
        <div className="loopback-badge">
          <Lock size={15} />
          <span>{t('sidebar.loopback')}</span>
        </div>
      </div>
      <div className="sidebar-list">
        {profiles.length === 0 ? (
          <div className="empty-sidebar">
            <strong>{t('sidebar.emptyTitle')}</strong>
            <span>{t('sidebar.emptyHint')}</span>
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
                    {kindLabel(profile.kind, t)}
                  </div>
                </div>
                <span className={`state-dot ${stateClass(profile.state)}`} />
              </div>
              <div className="profile-endpoint-row">
                <Link2 size={13} />
                <span>{endpointForSummary(profile, t)}</span>
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
        <div className="daemon-state"><ShieldCheck size={15} color="#219653" /><span>{t('sidebar.privateChannel')}</span></div>
        <span>NDJSON IPC</span>
      </div>
    </aside>
  );
}

function EmptyState({ onCreate, engineReady }: { onCreate: () => void; engineReady: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="empty-state">
      <AppLogo large />
      <h2>{engineReady ? t('empty.readyTitle') : t('empty.waitingTitle')}</h2>
      <p>{engineReady ? t('empty.readyDesc') : t('empty.waitingDesc')}</p>
      {engineReady && <div className="empty-actions"><button className="primary-button" onClick={onCreate}><Plus size={15} />{t('empty.create')}</button></div>}
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
  const { t } = useTranslation();
  const [menuOpen, setMenuOpen] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [wgStatus, setWgStatus] = useState<WireGuardStatus | null>(null);
  const endpoints = endpointForProfile(profile, t);
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
                {stateLabel(profile.state, t)}
              </span>
              <span className="type-pill">{kindLabel(profile.kind, t)}</span>
              {profile.tags.map((tag) => <span className="tag" key={tag}><Tag size={11} />{tag}</span>)}
            </div>
          </div>
        </div>
        <div className="detail-actions">
          <button className={active ? 'danger-button' : 'primary-button'} onClick={onToggle} disabled={!bridge()}>
            {active ? <><Square size={14} fill="currentColor" />{t('common.stop')}</> : <><Play size={14} fill="currentColor" />{t('common.start')}</>}
          </button>
          <button className="outline-button" onClick={onEdit} disabled={!bridge()}><Pencil size={14} />{t('common.edit')}</button>
          <div style={{ position: 'relative' }}>
            <button className="icon-button" onClick={() => setMenuOpen((open) => !open)} title={t('detail.moreActions')}><MoreHorizontal size={19} /></button>
            {menuOpen && (
              <div className="floating-menu" onMouseLeave={() => setMenuOpen(false)}>
                {sshCommand && <button onClick={() => { void copyEndpoint(sshCommand); setMenuOpen(false); }}><Clipboard size={14} />{t('detail.copySshCommand')}</button>}
                {endpoints[0] && <button onClick={() => { void copyEndpoint(formatAddress(endpoints[0].host, endpoints[0].port, t)); setMenuOpen(false); }}><Copy size={14} />{t('detail.copyFirstEndpoint')}</button>}
                <button className="menu-danger" onClick={() => { setMenuOpen(false); onDelete(); }}><Trash2 size={14} />{t('detail.deleteConfig')}</button>
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
          <div className="section-title-left"><span className="section-icon"><Layers size={15} /></span><div><h2>{t('detail.endpointsTitle')}</h2><p>{t('detail.endpointsDesc')}</p></div></div>
          <div className="section-title-right"><CheckCircle2 size={14} color="#219653" />&nbsp;{t('detail.listenLocalOnly')}</div>
        </div>
        {profile.kind === 'Ssh' && profile.ssh?.use_system_proxy && (
          <RouteBanner ssh={profile.ssh} proxy={settings.socks_proxy} />
        )}
        {profile.kind === 'Http' && profile.http?.use_system_proxy && (
          <div className="route-banner">
            <div className="route-banner-header"><Globe size={15} />{t('detail.httpProxyRoute')}</div>
            <div className="route-flow"><span className="route-step"><strong>LocalProxy</strong> {t('detail.localListen')}</span><span className="route-arrow">→</span><span className="route-step"><strong>HTTP Proxy</strong> {proxy ? formatAddress(proxy.host, proxy.port, t) : t('common.notConfigured')}</span><span className="route-arrow">→</span><span className="route-step"><strong>{t('detail.upstreamService')}</strong> {profile.http.upstream_url}</span></div>
          </div>
        )}
        <div className="endpoint-grid">
          {endpoints.length === 0 ? (
            <div className="empty-sidebar"><strong>{t('detail.noEndpoints')}</strong><span>{t('detail.noEndpointsHint')}</span></div>
          ) : endpoints.map((endpoint, index) => {
            const address = formatAddress(endpoint.host, endpoint.port, t);
            const wsForward = profile.wstunnel?.forwards[index];
            const forward = profile.ssh?.forwards[index] ?? (wsForward ? { ...wsForward, tag: t('detail.localForwardTag', { proto: wsForward.protocol.toUpperCase() }) } : undefined);
            const isCopied = copied === address;
            if (profile.kind === 'WireGuard') {
              const peer = profile.wireguard?.peers[index] ?? profile.wireguard?.peers[0];
              return (
                <div className="endpoint-card" key={`${endpoint.kind}-${index}-${endpoint.port}`}>
                  <div className="endpoint-card-top">
                    <div className="endpoint-kind"><span className="endpoint-kind-icon"><ShieldCheck size={15} /></span><div><strong>{endpoint.host}</strong><div className="endpoint-state">{stateLabel(profile.state, t)} · {t('detail.wgInterface')}</div></div></div>
                  </div>
                  <div className="copyable-address"><span>{profile.wireguard?.addresses.join(', ') || t('detail.noAddress')}</span></div>
                  <div className="endpoint-remote">{t('detail.peer')} · {peer ? `${peer.endpoint_host}:${peer.endpoint_port}` : t('common.notConfigured')}</div>
                  <div className="endpoint-meta"><span className="tag neutral">WireGuard</span>{peer?.persistent_keepalive != null && <span className="tag">Keepalive {peer.persistent_keepalive}s</span>}</div>
                </div>
              );
            }
            return (
              <div className="endpoint-card" key={`${endpoint.kind}-${index}-${endpoint.port}`} onClick={() => void copyEndpoint(address)}>
                <div className="endpoint-card-top">
                  <div className="endpoint-kind"><span className="endpoint-kind-icon">{profile.kind === 'Http' ? <Server size={15} /> : <Link2 size={15} />}</span><div><strong>{profile.kind === 'Http' ? t('kind.http') : (forward?.tag || t('detail.sshLocalForward'))}</strong><div className="endpoint-state">{stateLabel(profile.state, t)} · {t('detail.clickToCopy')}</div></div></div>
                  <button className="icon-button" onClick={(event) => { event.stopPropagation(); void copyEndpoint(address); }} title={t('detail.copyEndpoint')}>{isCopied ? <Check size={15} color="#219653" /> : <Copy size={15} />}</button>
                </div>
                <div className="copyable-address"><span>{profile.kind === 'Http' ? `http://${address}` : address}</span></div>
                <div className="endpoint-remote">{profile.kind === 'Http' ? `${t('detail.upstreamPrefix')}${profile.http?.upstream_url ?? t('common.notConfigured')}` : `${t('detail.remoteTargetPrefix')}${formatAddress(forward?.remote_host ?? endpoint.host, forward?.remote_port ?? 0, t)}`}</div>
                <div className="endpoint-meta"><span className="tag neutral">{endpoint.kind === 'http' ? 'HTTP/1.1' : profile.wstunnel ? 'WebSocket' : 'direct-tcpip'}</span>{forward?.tag && <span className="tag">{forward.tag}</span>}</div>
              </div>
            );
          })}
        </div>
      </section>

      <section className="surface-card">
        <div className="section-title"><div className="section-title-left"><span className="section-icon"><Settings size={15} /></span><div><h2>{t('detail.configTitle')}</h2><p>{t('detail.configDesc')}</p></div></div><button className="small-button" onClick={onEdit}><Pencil size={12} />{t('detail.editConfig')}</button></div>
        <div className="key-value-grid">
          {profile.kind === 'Http' && profile.http ? <>
            <KeyValue label={t('detail.upstreamBaseUrl')} value={profile.http.upstream_url} mono />
            <KeyValue label={t('detail.listenPort')} value={profile.http.listen_port > 0 ? formatAddress('127.0.0.1', profile.http.listen_port, t) : t('detail.assignedAfterStart')} mono />
            <KeyValue label={t('detail.authHeaders')} value={t('common.itemsHidden', { n: profile.http.headers.length })} />
            <KeyValue label={t('detail.authCookies')} value={t('common.itemsHidden', { n: profile.http.cookies.length })} />
            <KeyValue label={t('detail.appHttpProxy')} value={profile.http.use_system_proxy ? (proxy ? formatAddress(proxy.host, proxy.port, t) : t('common.notConfigured')) : t('common.notEnabled')} mono />
          </> : profile.kind === 'WireGuard' && profile.wireguard ? <>
            <KeyValue label={t('detail.interfaceAddresses')} value={profile.wireguard.addresses.join(', ')} mono />
            <KeyValue label={t('detail.interfaceMtu')} value={profile.wireguard.mtu?.toString() ?? t('detail.systemDefault')} mono />
            <KeyValue label={t('detail.listenPort')} value={profile.wireguard.listen_port ? profile.wireguard.listen_port.toString() : t('common.randomPort')} mono />
            <KeyValue label="DNS" value={profile.wireguard.dns.join(', ') || t('detail.dnsUnchanged')} mono />
            <KeyValue label={t('detail.peerCount')} value={t('common.countUnit', { n: profile.wireguard.peers.length })} />
          </> : profile.ssh ? <>
            <KeyValue label={t('detail.sshConnection')} value={profile.ssh.connection_name} />
            <KeyValue label={t('detail.sshRemote')} value={`${profile.ssh.username}@${formatAddress(profile.ssh.host, profile.ssh.port, t)}`} mono />
            <KeyValue label={t('detail.authMode')} value={profile.ssh.auth_mode === 'Password' ? (profile.ssh.password_configured ? t('detail.passwordSaved') : t('detail.passwordNotConfigured')) : t('detail.privateKeyFile')} />
            <KeyValue label={t('detail.localForwards')} value={t('common.rulesCount', { n: profile.ssh.forwards.length })} />
            <KeyValue label={t('detail.appSocks5Proxy')} value={profile.ssh.use_system_proxy ? (proxy ? formatAddress(proxy.host, proxy.port, t) : t('common.notConfigured')) : t('common.notEnabled')} mono />
          </> : profile.wstunnel ? <>
            <KeyValue label={t('detail.wstunnelServer')} value={profile.wstunnel.server_url} mono />
            <KeyValue label={t('detail.upgradePathPrefix')} value={profile.wstunnel.http_upgrade_path_prefix} mono />
            <KeyValue label={t('detail.localForwards')} value={t('common.rulesCount', { n: profile.wstunnel.forwards.length })} />
            <KeyValue label={t('detail.tlsVerify')} value={profile.wstunnel.tls_verify_certificate ? t('common.enabled') : t('detail.tlsOffNote')} />
          </> : null}
        </div>
        {profile.kind === 'WireGuard' && (
          <div className="wg-status-grid">
            <KeyValue label={t('detail.actualInterface')} value={wgStatus?.interface_name ?? t('detail.shownAfterStart')} mono />
            <KeyValue label={t('detail.localPublicKey')} value={wgStatus?.public_key ? `${wgStatus.public_key.slice(0, 12)}…` : t('common.notConnected')} mono />
            {(wgStatus?.peers ?? []).map((peer) => (
              <KeyValue
                key={peer.public_key}
                label={t('detail.peerKey', { key: peer.public_key.slice(0, 8) })}
                value={`${peer.endpoint ?? t('common.notConnected')} · ${t('detail.rxLabel')} ${formatBytes(peer.rx_bytes)} · ${t('detail.txLabel')} ${formatBytes(peer.tx_bytes)} · ${t('detail.handshakeLabel')} ${peer.handshake_age_sec == null ? t('detail.handshakeNever') : t('detail.secondsAgo', { n: peer.handshake_age_sec })}`}
                mono
              />
            ))}
          </div>
        )}
      </section>

      <section className="surface-card log-card">
        <div className="section-title"><div className="section-title-left"><span className="section-icon"><List size={15} /></span><div><h2>{t('detail.logsTitle')}</h2><p>{t('detail.logsDesc')}</p></div></div><button className="small-button" onClick={onClearLogs}>{t('common.clear')}</button></div>
        <div className="log-list">
          {logs.length === 0 ? <div className="empty-sidebar"><span>{t('detail.noLogs')}</span></div> : logs.slice(0, 30).map((log) => <div className="log-row" key={log.id}><span className="log-time">{log.time}</span><span className="log-level">{stateLabel(log.state, t)}</span><span className="log-message" title={log.message}>{log.message}</span></div>)}
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
  const { t } = useTranslation();
  return (
    <div className="route-banner">
      <div className="route-banner-header"><Wifi size={15} />{t('route.title')}</div>
      <div className="route-flow"><span className="route-step"><strong>LocalProxy</strong> {t('route.localPort')}</span><span className="route-arrow">→</span><span className="route-step"><strong>SOCKS5</strong> {proxy ? formatAddress(proxy.host, proxy.port, t) : t('common.notConfigured')}</span><span className="route-arrow">→</span><span className="route-step"><strong>{t('route.sshSession')}</strong> {ssh.username}@{formatAddress(ssh.host, ssh.port, t)}</span><span className="route-arrow">→</span><span className="route-step"><strong>{t('route.target')}</strong> SSH direct-tcpip</span></div>
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

// Rebuilds wg-quick text from a stored profile. The private key is only
// available when the editor explicitly fetches it for the stored profile;
// otherwise it is rendered as a comment so pasted text still parses.
function wireGuardConfigText(config: NonNullable<ProfileView['wireguard']>, t: TFunction, privateKey?: string | null): string {
  const lines: string[] = ['[Interface]'];
  if (privateKey) lines.push(`PrivateKey = ${privateKey}`);
  else if (config.private_key_configured) lines.push(t('editor.wgPrivateKeySavedComment'));
  lines.push(`Address = ${config.addresses.join(', ')}`);
  if (config.mtu != null) lines.push(`MTU = ${config.mtu}`);
  if (config.listen_port) lines.push(`ListenPort = ${config.listen_port}`);
  if (config.dns.length > 0) lines.push(`DNS = ${config.dns.join(', ')}`);
  for (const peer of config.peers) {
    lines.push('', '[Peer]', `PublicKey = ${peer.public_key}`);
    const host = peer.endpoint_host.includes(':') ? `[${peer.endpoint_host}]` : peer.endpoint_host;
    lines.push(`Endpoint = ${host}:${peer.endpoint_port}`);
    lines.push(`AllowedIPs = ${peer.allowed_ips.join(', ')}`);
    if (peer.persistent_keepalive != null) lines.push(`PersistentKeepalive = ${peer.persistent_keepalive}`);
  }
  return lines.join('\n');
}

function ToastStack({ toasts, onDismiss }: { toasts: Toast[]; onDismiss: (id: number) => void }) {
  if (toasts.length === 0) return null;
  return <div className="toast-stack">{toasts.map((toast) => <div className={`toast ${toast.tone}`} key={toast.id}>{toast.tone === 'error' ? <AlertCircle size={16} /> : toast.tone === 'success' ? <CheckCircle2 size={16} /> : <Activity size={16} />}<span>{toast.message}</span><button onClick={() => onDismiss(toast.id)}><X size={14} /></button></div>)}</div>;
}

function TypePickerModal({ onClose, onChoose }: { onClose: () => void; onChoose: (kind: ProxyKind) => void }) {
  const { t } = useTranslation();
  const options: Array<{ kind: ProxyKind; hint: string; icon: JSX.Element }> = [
    { kind: 'WireGuard', hint: t('typePicker.wgHint'), icon: <ShieldCheck size={20} /> },
    { kind: 'Wstunnel', hint: t('typePicker.wstunnelHint'), icon: <Link2 size={20} /> },
    { kind: 'Http', hint: t('typePicker.httpHint'), icon: <Globe size={20} /> },
    { kind: 'Ssh', hint: t('typePicker.sshHint'), icon: <Terminal size={20} /> },
  ];
  return <div className="modal-backdrop"><div className="dialog type-picker-dialog" role="dialog" aria-modal="true"><div className="modal-heading"><div><h2>{t('typePicker.title')}</h2><p>{t('typePicker.desc')}</p></div><button className="icon-button" onClick={onClose}><X size={18} /></button></div><div className="dialog-scroll type-grid">{options.map((option) => <button key={option.kind} className="type-card" onClick={() => onChoose(option.kind)}>{option.icon}<strong>{kindLabel(option.kind, t)}</strong><span>{option.hint}</span></button>)}</div></div></div>;
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
  const { t } = useTranslation();
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
  const [wgConfigText, setWgConfigText] = useState(() => initialWireGuard ? wireGuardConfigText(initialWireGuard, t) : '');
  useEffect(() => {
    const profileId = initial?.id;
    if (!profileId || !initialWireGuard) return;
    let cancelled = false;
    void (async () => {
      try {
        const key = await bridge()?.request<string | null>('wireguard_private_key', { id: profileId });
        if (!cancelled && key) setWgConfigText(wireGuardConfigText(initialWireGuard, t, key));
      } catch { /* keep the reconstructed text without the key */ }
    })();
    return () => { cancelled = true; };
  }, [initial?.id]);
  const importWstunnel = async () => {
    const api = bridge();
    if (!api) return setFormError(t('editor.importNeedDesktop'));
    setImporting(true); setFormError(null);
    try {
      const config = await api.request<WstunnelConfig>('parse_wstunnel_command', { command: importCommand });
      setWsConfig(config);
      if (!name.trim()) setName(t('editor.defaultWstunnelName', { host: new URL(config.server_url).hostname }));
      setImportCommand('');
    } catch (error) { setFormError(String(error)); } finally { setImporting(false); }
  };
  const importWireGuardConfig = async () => {
    const api = bridge();
    if (!api) return setFormError(t('editor.importWgNeedDesktop'));
    setImporting(true); setFormError(null);
    try {
      const config = await api.request<WireGuardConfig>('parse_wg_config', { config: wgConfigText });
      // Pasted text without a PrivateKey line keeps the key already stored for
      // this profile; the core never returns the key value itself.
      if (!config.private_key.value && initialWireGuard?.private_key_configured) {
        config.private_key = { name: 'private_key', value: null, kind: 'Keep' };
      }
      setWgConfig(config);
      if (!name.trim()) setName(t('editor.defaultWgName'));
      setWgConfigText('');
      onNotify(t('editor.wgImported'), 'success');
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
  const [sshConnectionId, setSshConnectionId] = useState(initialSsh?.connection_id ?? settings.ssh_connections[0]?.id ?? '');
  const [forwardRows, setForwardRows] = useState<ForwardRowState[]>(() => (initialSsh?.forwards ?? []).map((forward, index) => ({ ...forward, id: `forward-${index}-${forward.local_port}` })));
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const selectedSshConnection = settings.ssh_connections.find((connection) => connection.id === sshConnectionId);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setFormError(null);
    const trimmedName = name.trim();
    if (!trimmedName) return setFormError(t('editor.nameRequired'));
    if (kind === 'Wstunnel') {
      setSaving(true);
      try { await onSave({ id: initial?.id ?? null, name: trimmedName, tags: tags.split(/[\n,]/).map(t => t.trim()).filter(Boolean), kind, http: null, ssh: null, wstunnel: wsConfig }); }
      catch { /* parent displays save errors */ } finally { setSaving(false); }
      return;
    }
    if (kind === 'WireGuard') {
      if (wgConfig.addresses.length === 0) return setFormError(t('editor.wgAddressRequired'));
      if (!wgConfig.private_key.value && wgConfig.private_key.kind !== 'Keep') return setFormError(t('editor.wgKeyRequired'));
      if (wgConfig.peers.length === 0) return setFormError(t('editor.wgPeerRequired'));
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
      if (!upstreamUrl.trim()) return setFormError(t('editor.upstreamRequired'));
      const headerResult = buildSecretPatches(headers, removedHeaders, 'Header', t);
      const cookieResult = buildSecretPatches(cookies, removedCookies, 'Cookie', t);
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

    if (!sshConnectionId) return setFormError(t('editor.sshConnectionRequired'));
    if (!sshConnectionId) return setFormError(t('editor.sshConnectionRequired'));
    const forwards: ForwardDraft[] = [];
    for (const row of forwardRows) {
      const hasAnyValue = row.remote_host.trim() || row.remote_port > 0 || row.local_port > 0 || row.tag.trim();
      if (!hasAnyValue) continue;
      if (!row.remote_host.trim()) return setFormError(t('editor.forwardRemoteRequired'));
      if (row.remote_port <= 0) return setFormError(t('editor.forwardPortInvalid', { host: row.remote_host }));
      forwards.push({ local_port: row.local_port, remote_host: row.remote_host.trim(), remote_port: row.remote_port, tag: row.tag.trim() });
    }
    const draft: ProfileDraft = {
      id: initial?.id ?? null,
      name: trimmedName,
      tags: tags.split(/[\n,]/).map((tag) => tag.trim()).filter(Boolean),
      kind,
      http: null,
      ssh: {
        connection_id: sshConnectionId,
        forwards,
      },
    };
    setSaving(true);
    try { await onSave(draft); } catch { /* the parent presents the error toast */ } finally { setSaving(false); }
  };

  const kindText = t(`kind.${kind.toLowerCase()}`);

  return (
    <div className="modal-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="profile-editor-title">
        <div className="modal-heading">
          <div><h2 id="profile-editor-title">{initial ? t('editor.editTitle', { kind: kindText }) : t('editor.newTitle', { kind: kindText })}</h2><p>{t('editor.desc')}</p></div>
          <button className="icon-button" onClick={onClose} title={t('common.close')}><X size={18} /></button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="dialog-scroll">
            {kind === 'Wstunnel' && (
              <section className="form-section">
                <div className="form-section-title"><h3>{t('editor.quickImportTitle')}</h3><span>{t('editor.quickImportDesc')}</span></div>
                <div className="form-field"><textarea aria-label="wstunnel client" className="mono-input" rows={3} value={importCommand} onChange={e => setImportCommand(e.target.value)} placeholder={"wstunnel client --http-upgrade-path-prefix XXXX -L 'udp://50814:localhost:50814?timeout_sec=0' wss://18.123.135.89:443"} /></div>
                <button type="button" className="small-button" disabled={importing || !importCommand.trim()} onClick={() => void importWstunnel()}>{importing ? t('editor.parsing') : t('editor.parseAndFill')}</button>
              </section>
            )}
            <section className="form-section">
              <div className="form-section-title"><h3>{t('editor.basicInfo')}</h3><span>{t('editor.basicInfoDesc')}</span></div>
              <div className="form-grid">
                <div className="form-field"><label>{t('editor.nameLabel')}</label><input value={name} onChange={(event) => setName(event.target.value)} placeholder={t('editor.namePlaceholder')} autoFocus /></div>
                <div className="form-field"><label>Tag <span>{t('editor.tagOptional')}</span></label><input value={tags} onChange={(event) => setTags(event.target.value)} placeholder="production, mysql" /></div>
              </div>
            </section>

            {kind === 'WireGuard' ? (
              <section className="form-section">
                <div className="form-section-title"><h3>{t('editor.wgImportTitle')}</h3><span>{t('editor.wgImportDesc')}</span></div>
                <div className="form-field"><textarea aria-label="wg-quick" className="mono-input" rows={10} value={wgConfigText} onChange={e => setWgConfigText(e.target.value)} placeholder={"[Interface]\nPrivateKey = ...\nAddress = 192.168.1.6/32\nMTU = 1420\nDNS = 223.5.5.5\n\n[Peer]\nPublicKey = ...\nEndpoint = localhost:50814\nAllowedIPs = 172.16.110.0/24,172.31.0.2/32\nPersistentKeepalive = 25"} /></div>
                <button type="button" className="primary-button" disabled={importing || !wgConfigText.trim()} onClick={() => void importWireGuardConfig()}>{importing ? t('editor.importing') : t('editor.parseAndImport')}</button>
              </section>
            ) : kind === 'Wstunnel' ? (
              <section className="form-section wstunnel-section">
                <div className="form-section-title"><h3>{t('editor.wstunnelTitle')}</h3><span>{t('editor.wstunnelDesc')}</span></div>
                <div className="form-grid wstunnel-server-grid">
                  <div className="form-field"><label>{t('editor.serverUrl')}</label><input aria-label={t('detail.wstunnelServer')} className="mono-input" value={wsConfig.server_url} onChange={e => setWsConfig({...wsConfig, server_url: e.target.value})} placeholder="wss://example.com:443" /></div>
                  <div className="form-field"><label>{t('detail.upgradePathPrefix')}</label><input aria-label={t('detail.upgradePathPrefix')} className="mono-input" value={wsConfig.http_upgrade_path_prefix} onChange={e => setWsConfig({...wsConfig, http_upgrade_path_prefix: e.target.value})} /></div>
                </div>
                <label className="wstunnel-tls-option">
                  <input type="checkbox" checked={wsConfig.tls_verify_certificate} onChange={e => setWsConfig({...wsConfig, tls_verify_certificate: e.target.checked})} />
                  <span className="wstunnel-checkbox"><Check size={12} /></span>
                  <span className="wstunnel-tls-copy"><strong>{t('editor.tlsVerifyLabel')}</strong><small>{t('editor.tlsVerifyHint')}</small></span>
                </label>
                <div className="wstunnel-forwarding">
                  <div className="wstunnel-forwarding-header">
                    <div><strong>{t('editor.forwardsTitle')}</strong><span>{wsConfig.forwards.length > 0 ? t('editor.forwardsConfigured', { n: wsConfig.forwards.length }) : t('editor.forwardsHint')}</span></div>
                    {wsConfig.forwards.length > 0 && <button type="button" className="add-row-button wstunnel-add-button" onClick={() => setWsConfig({...wsConfig, forwards: [...wsConfig.forwards, {protocol: 'udp', local_port: 50814, remote_host: 'localhost', remote_port: 50814, timeout_sec: 0}]})}><Plus size={13} />{t('editor.addRule')}</button>}
                  </div>
                  {wsConfig.forwards.length === 0 && <div className="wstunnel-empty-rules"><div><strong>{t('editor.noForwards')}</strong><span>{t('editor.noForwardsHint')}</span></div><button type="button" className="small-button" onClick={() => setWsConfig({...wsConfig, forwards: [...wsConfig.forwards, {protocol: 'udp', local_port: 50814, remote_host: 'localhost', remote_port: 50814, timeout_sec: 0}]})}><Plus size={14} />{t('editor.addForwardRule')}</button></div>}
                  {wsConfig.forwards.map((f, index) => {
                    const update = (patch: Partial<typeof f>) => setWsConfig({...wsConfig, forwards: wsConfig.forwards.map((row, i) => i === index ? {...row, ...patch} : row)});
                    return <div className="wstunnel-forward-card" key={index}>
                      <div className="wstunnel-forward-card-header"><strong>{t('common.ruleIndex', { n: index + 1 })}</strong><button type="button" className="remove-row" onClick={() => setWsConfig({...wsConfig, forwards: wsConfig.forwards.filter((_, i) => i !== index)})} title={t('editor.deleteRule')}><Trash2 size={14} /></button></div>
                      <div className="wstunnel-forward-grid">
                        <div className="form-field"><label>{t('common.protocol')}</label><select value={f.protocol} onChange={e => update({protocol: e.target.value, timeout_sec: null})}><option value="udp">UDP</option><option value="tcp">TCP</option></select></div>
                        <div className="form-field"><label>{t('common.localPort')}</label><input className="mono-input" type="number" min="1" max="65535" value={f.local_port} onChange={e => update({local_port: Number(e.target.value)})} /></div>
                        <div className="form-field"><label>{t('common.remoteHost')}</label><input className="mono-input" value={f.remote_host} onChange={e => update({remote_host: e.target.value})} placeholder="localhost" /></div>
                        <div className="form-field"><label>{t('common.remotePort')}</label><input className="mono-input" type="number" min="1" max="65535" value={f.remote_port} onChange={e => update({remote_port: Number(e.target.value)})} /></div>
                        {f.protocol === 'udp' && <div className="form-field"><label>{t('editor.udpTimeout')} <span>s</span></label><input className="mono-input" type="number" min="0" max="4294967295" placeholder="30" value={f.timeout_sec ?? ''} onChange={e => update({timeout_sec: e.target.value === '' ? null : Number(e.target.value)})} /><div className="field-hint">{t('editor.udpTimeoutHint')}</div></div>}
                      </div>
                    </div>;
                  })}
                </div>
              </section>
            ) : kind === 'Http' ? (
              <section className="form-section">
                <div className="form-section-title"><h3>{t('editor.httpTitle')}</h3><span>{t('editor.httpDesc')}</span></div>
                <div className="form-grid">
                  <div className="form-field"><label>{t('common.localPort')}</label><input className="mono-input" type="number" min="0" max="65535" value={listenPort} onChange={(event) => setListenPort(Number(event.target.value))} /><div className="field-hint">{t('editor.listenPortHint')}</div></div>
                  <div className="form-field"><label>{t('detail.upstreamBaseUrl')}</label><input className="mono-input" value={upstreamUrl} onChange={(event) => setUpstreamUrl(event.target.value)} placeholder="https://service.example.com" /><div className="field-hint">{t('editor.upstreamUrlHint')}</div></div>
                </div>
                <div className="proxy-toggle-card"><div className="switch-row"><div className="switch-copy"><Globe size={16} /><div><strong>{t('editor.useAppProxy')}</strong><span>{settings.http_proxy ? t('editor.currentProxy', { address: formatAddress(settings.http_proxy.host, settings.http_proxy.port, t) }) : t('editor.configureProxyFirst')}</span></div></div><label className="switch"><input type="checkbox" checked={httpUseProxy} onChange={(event) => setHttpUseProxy(event.target.checked)} /><span /></label></div></div>
                <SecretEditor title="Headers" hint={t('secrets.headersHint')} rows={headers} setRows={setHeaders} removed={removedHeaders} setRemoved={setRemovedHeaders} label="Header" onNotify={onNotify} />
                <SecretEditor title="Cookies" hint={t('secrets.cookiesHint')} rows={cookies} setRows={setCookies} removed={removedCookies} setRemoved={setRemovedCookies} label="Cookie" onNotify={onNotify} />
              </section>
            ) : (
              <section className="form-section">
                <div className="form-section-title"><h3>{t('editor.sshTitle')}</h3><span>{t('editor.sshDesc')}</span></div>
                {settings.ssh_connections.length === 0 ? <div className="ssh-connection-empty"><Terminal size={18} /><div><strong>{t('editor.noSshConnections')}</strong><span>{t('editor.noSshConnectionsHint')}</span></div></div> : <>
                  <div className="form-field"><label>{t('editor.useSshConnection')}</label><select value={sshConnectionId} onChange={(event) => setSshConnectionId(event.target.value)}>{settings.ssh_connections.map((connection) => <option value={connection.id} key={connection.id}>{connection.name} · {connection.username}@{formatAddress(connection.host, connection.port, t)}</option>)}</select></div>
                  {selectedSshConnection && <div className="selected-ssh-connection"><Terminal size={16} /><div><strong>{selectedSshConnection.name}</strong><span>{selectedSshConnection.auth_mode === 'Password' ? t('editor.passwordAuth') : t('editor.keyAuth')} · {selectedSshConnection.use_system_proxy ? t('editor.viaSocks5') : t('editor.direct')}</span></div></div>}
                </>}
                <ForwardEditor rows={forwardRows} setRows={setForwardRows} />
              </section>
            )}
            {formError && <div className="error-banner"><AlertCircle size={16} /><span>{formError}</span></div>}
          </div>
          <div className="modal-footer"><button type="button" className="outline-button" onClick={onClose}>{t('common.cancel')}</button><button type="submit" className="primary-button" disabled={saving}>{saving ? <RefreshCw size={14} className="spinner" /> : <Save size={14} />}{saving ? t('common.saving') : t('editor.save')}</button></div>
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
  const { t } = useTranslation();
  const [batchOpen, setBatchOpen] = useState(false);
  const [batchText, setBatchText] = useState('');
  const addRow = () => setRows((current) => [...current, { id: `${label}-${Date.now()}-${current.length}`, name: '', value: '', configured: false }]);
  const removeRow = (row: SecretRow) => {
    setRows((current) => current.filter((item) => item.id !== row.id));
    if (row.configured && row.originalName) setRemoved((current) => new Set(current).add(row.originalName!));
  };
  const applyBatch = () => {
    const pairs = parseBatchPairs(batchText);
    if (pairs.length === 0) return onNotify(t('secrets.batchNoPairs'), 'error');
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
    onNotify(t('secrets.batchParsed', { n: pairs.length, label }), 'success');
  };
  return <div className="secret-section"><div className="secret-heading"><div><strong>{title}</strong><span className="configured-badge"><Check size={11} />{t('common.savedCount', { n: rows.filter((row) => row.configured).length })}</span></div><span>{hint}</span></div>{rows.length === 0 && <div className="field-hint">{t('secrets.noItems')}</div>}{rows.map((row) => <div className="secret-row" key={row.id}><input value={row.name} onChange={(event) => setRows((current) => current.map((item) => item.id === row.id ? { ...item, name: event.target.value } : item))} placeholder={t('secrets.namePlaceholder', { label })} /><input value={row.value} onChange={(event) => setRows((current) => current.map((item) => item.id === row.id ? { ...item, value: event.target.value } : item))} placeholder={row.configured ? t('secrets.valuePlaceholderKept') : t('secrets.valuePlaceholder', { label })} type="text" /><button type="button" className="remove-row" onClick={() => removeRow(row)} title={t('common.remove')}><Trash2 size={14} /></button></div>)}<button type="button" className="add-row-button" onClick={addRow}><Plus size={13} />{t('secrets.addLabel', { label })}</button><button type="button" className="batch-toggle" onClick={() => setBatchOpen((open) => !open)}>{batchOpen ? t('secrets.batchCollapse') : t('secrets.batchExpand')}</button>{batchOpen && <div className="batch-box"><label className="batch-label">{t('secrets.batchHint')}</label><textarea value={batchText} onChange={(event) => setBatchText(event.target.value)} placeholder={'sec-fetch-dest\nscript\nsec-fetch-mode\nno-cors'} /><div className="batch-actions"><button type="button" className="small-button" onClick={applyBatch}><Clipboard size={12} />{t('secrets.parseAndAdd')}</button></div></div>}</div>;
}

function ForwardEditor({ rows, setRows }: { rows: ForwardRowState[]; setRows: Dispatch<SetStateAction<ForwardRowState[]>> }) {
  const { t } = useTranslation();
  const addRow = () => setRows((current) => [...current, { ...DEFAULT_FORWARD, id: `forward-${Date.now()}-${current.length}` }]);
  const removeRow = (id: string) => setRows((current) => current.filter((row) => row.id !== id));
  const update = (id: string, field: keyof ForwardDraft, value: string | number) => setRows((current) => current.map((row) => row.id === id ? { ...row, [field]: value } : row));
  return <div className="secret-section"><div className="secret-heading"><strong>{t('forwards.title')}</strong><span>{t('forwards.desc')}</span></div><div className="forward-header"><span>{t('common.localPort')}</span><span>{t('common.remoteHost')}</span><span>{t('common.remotePort')}</span><span>{t('common.tag')}</span><span /></div>{rows.length === 0 && <div className="field-hint">{t('forwards.noRules')}</div>}{rows.map((row) => <div className="forward-row" key={row.id}><input className="mono-input" type="number" min="0" max="65535" value={row.local_port} onChange={(event) => update(row.id, 'local_port', Number(event.target.value))} placeholder={t('forwards.localPortPlaceholder')} /><input className="mono-input" value={row.remote_host} onChange={(event) => update(row.id, 'remote_host', event.target.value)} placeholder="db.internal.example" /><input className="mono-input" type="number" min="1" max="65535" value={row.remote_port} onChange={(event) => update(row.id, 'remote_port', Number(event.target.value))} placeholder="3306" /><input value={row.tag} onChange={(event) => update(row.id, 'tag', event.target.value)} placeholder="mysql-prod" /><button type="button" className="remove-row" onClick={() => removeRow(row.id)} title={t('forwards.removeRule')}><Trash2 size={14} /></button></div>)}<button type="button" className="add-row-button" onClick={addRow}><Plus size={13} />{t('forwards.addRule')}</button></div>;
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
  const { t, i18n } = useTranslation();
  const currentLanguage = LANGUAGES.find((language) => i18n.language.startsWith(language.code))?.code ?? 'en';
  const [httpEnabled, setHttpEnabled] = useState(initial.http_proxy !== null);
  const [socksEnabled, setSocksEnabled] = useState(initial.socks_proxy !== null);
  const [httpHost, setHttpHost] = useState(initial.http_proxy?.host ?? '127.0.0.1');
  const [httpPort, setHttpPort] = useState(initial.http_proxy?.port ?? 8099);
  const [socksHost, setSocksHost] = useState(initial.socks_proxy?.host ?? '127.0.0.1');
  const [socksPort, setSocksPort] = useState(initial.socks_proxy?.port ?? 8099);
  const [sshConnections, setSshConnections] = useState<SshConnectionEditorState[]>(() => initial.ssh_connections.map((connection) => ({
    ...connection,
    editorKey: connection.id,
    password: '',
    passphrase: '',
    isNew: false,
  })));
  const [saving, setSaving] = useState(false);

  const addSshConnection = () => setSshConnections((current) => [...current, {
    id: '',
    editorKey: `ssh-connection-${Date.now()}-${current.length}`,
    name: '',
    host: '',
    port: 22,
    username: '',
    auth_mode: 'Password',
    private_key_path: '',
    password_configured: false,
    passphrase_configured: false,
    use_system_proxy: false,
    password: '',
    passphrase: '',
    isNew: true,
  }]);
  const updateSshConnection = (editorKey: string, patch: Partial<SshConnectionEditorState>) => setSshConnections((current) => current.map((connection) => connection.editorKey === editorKey ? { ...connection, ...patch } : connection));
  const removeSshConnection = (editorKey: string) => setSshConnections((current) => current.filter((connection) => connection.editorKey !== editorKey));
  const chooseSshPrivateKey = async (editorKey: string) => {
    const currentBridge = bridge();
    if (!currentBridge) return onNotify(t('settings.pickKeyNeedDesktop'), 'error');
    const path = await currentBridge.choosePrivateKey();
    if (path) updateSshConnection(editorKey, { private_key_path: path });
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (httpEnabled && (!httpHost.trim() || httpPort <= 0)) return onNotify(t('settings.invalidHttpProxy'), 'error');
    if (socksEnabled && (!socksHost.trim() || socksPort <= 0)) return onNotify(t('settings.invalidSocksProxy'), 'error');
    for (const connection of sshConnections) {
      if (!connection.name.trim() || !connection.host.trim() || !connection.username.trim() || connection.port <= 0) return onNotify(t('settings.invalidSshConnection'), 'error');
      if (connection.auth_mode === 'Password' && !connection.password && !connection.password_configured) return onNotify(t('settings.sshNeedsPassword', { name: connection.name }), 'error');
      if (connection.auth_mode === 'PrivateKey' && !connection.private_key_path.trim()) return onNotify(t('settings.sshNeedsKey', { name: connection.name }), 'error');
    }
    const sshDrafts: SshConnectionDraft[] = sshConnections.map((connection) => ({
      id: connection.isNew ? null : connection.id,
      name: connection.name.trim(),
      host: connection.host.trim(),
      port: connection.port,
      username: connection.username.trim(),
      auth_mode: connection.auth_mode,
      private_key_path: connection.private_key_path.trim(),
      password: { name: 'password', value: connection.password || null, kind: connection.password ? 'Set' : connection.password_configured ? 'Keep' : 'Clear' },
      passphrase: { name: 'passphrase', value: connection.passphrase || null, kind: connection.passphrase ? 'Set' : connection.passphrase_configured ? 'Keep' : 'Clear' },
      use_system_proxy: connection.use_system_proxy,
    }));
    setSaving(true);
    try {
      const next = await request<AppSettings>('save_settings', {
        http_proxy: httpEnabled ? { host: httpHost.trim(), port: httpPort } : null,
        socks_proxy: socksEnabled ? { host: socksHost.trim(), port: socksPort } : null,
        ssh_connections: sshDrafts,
      });
      onSaved(next);
    } catch (error) {
      onNotify(error instanceof Error ? error.message : t('settings.saveFailed'), 'error');
    } finally {
      setSaving(false);
    }
  };

  return <div className="modal-backdrop"><div className="dialog settings-dialog" role="dialog" aria-modal="true"><div className="modal-heading"><div><h2>{t('settings.title')}</h2><p>{t('settings.desc')}</p></div><button className="icon-button" onClick={onClose}><X size={18} /></button></div><form onSubmit={submit}><div className="dialog-scroll settings-scroll">
    <section className="settings-section"><div className="settings-section-heading"><div><strong>{t('settings.languageTitle')}</strong><span>{t('settings.languageDesc')}</span></div></div><div className="form-field language-field"><select className="language-select" value={currentLanguage} aria-label={t('settings.languageTitle')} onChange={(event) => changeLanguage(event.target.value as LanguageCode)}>{LANGUAGES.map((language) => <option key={language.code} value={language.code}>{language.label}</option>)}</select></div></section>
    <div className="settings-intro"><strong>{t('settings.introTitle')}</strong><br />{t('settings.introDesc')}</div>
    <section className="settings-section"><div className="settings-section-heading"><div><strong>{t('settings.upstreamProxyTitle')}</strong><span>{t('settings.upstreamProxyDesc')}</span></div></div><div className="proxy-settings-grid"><div className="proxy-setting"><div className="proxy-setting-header"><div className="proxy-setting-title"><Globe size={15} />{t('settings.httpProxy')}</div><label className="switch"><input type="checkbox" checked={httpEnabled} onChange={(event) => setHttpEnabled(event.target.checked)} /><span /></label></div><span>{t('settings.httpProxyUse')}</span>{httpEnabled && <div className="proxy-endpoint-fields" style={{ marginTop: 11 }}><input className="mono-input" value={httpHost} onChange={(event) => setHttpHost(event.target.value)} placeholder="127.0.0.1" /><input className="mono-input" type="number" min="1" max="65535" value={httpPort} onChange={(event) => setHttpPort(Number(event.target.value))} placeholder="8080" /></div>}</div><div className="proxy-setting"><div className="proxy-setting-header"><div className="proxy-setting-title"><Wifi size={15} />{t('settings.socksProxy')}</div><label className="switch"><input type="checkbox" checked={socksEnabled} onChange={(event) => setSocksEnabled(event.target.checked)} /><span /></label></div><span>{t('settings.socksProxyUse')}</span>{socksEnabled && <div className="proxy-endpoint-fields" style={{ marginTop: 11 }}><input className="mono-input" value={socksHost} onChange={(event) => setSocksHost(event.target.value)} placeholder="127.0.0.1" /><input className="mono-input" type="number" min="1" max="65535" value={socksPort} onChange={(event) => setSocksPort(Number(event.target.value))} placeholder="8099" /></div>}</div></div></section>
    <section className="settings-section"><div className="settings-section-heading"><div><strong>{t('settings.sshTitle')}</strong><span>{t('settings.sshDesc')}</span></div><button type="button" className="small-button" onClick={addSshConnection}><Plus size={13} />{t('settings.addSsh')}</button></div>{sshConnections.length === 0 ? <div className="settings-empty"><Terminal size={20} /><strong>{t('settings.noSsh')}</strong><span>{t('settings.noSshHint')}</span></div> : <div className="ssh-settings-list">{sshConnections.map((connection, index) => <SshConnectionSettingCard key={connection.editorKey} connection={connection} index={index} socksProxyConfigured={socksEnabled} onUpdate={(patch) => updateSshConnection(connection.editorKey, patch)} onRemove={() => removeSshConnection(connection.editorKey)} onChoosePrivateKey={() => void chooseSshPrivateKey(connection.editorKey)} />)}</div>}</section>
  </div><div className="modal-footer"><button type="button" className="outline-button" onClick={onClose}>{t('common.cancel')}</button><button type="submit" className="primary-button" disabled={saving}>{saving ? <RefreshCw size={14} className="spinner" /> : <Save size={14} />}{saving ? t('common.saving') : t('settings.save')}</button></div></form></div></div>;
}

function SshConnectionSettingCard({ connection, index, socksProxyConfigured, onUpdate, onRemove, onChoosePrivateKey }: { connection: SshConnectionEditorState; index: number; socksProxyConfigured: boolean; onUpdate: (patch: Partial<SshConnectionEditorState>) => void; onRemove: () => void; onChoosePrivateKey: () => void }) {
  const { t } = useTranslation();
  return <article className="ssh-setting-card"><div className="ssh-setting-card-header"><div className="ssh-setting-title"><span className="ssh-setting-icon"><Terminal size={15} /></span><div><strong>{connection.name.trim() || t('sshCard.defaultName', { n: index + 1 })}</strong><span>{connection.host.trim() ? `${connection.username || t('sshCard.userFallback')}@${formatAddress(connection.host, connection.port, t)}` : t('sshCard.notComplete')}</span></div></div><button type="button" className="remove-row" onClick={onRemove} title={t('sshCard.deleteSsh')}><Trash2 size={14} /></button></div>
    <div className="ssh-setting-grid"><div className="form-field"><label>{t('sshCard.nameLabel')}</label><input value={connection.name} onChange={(event) => onUpdate({ name: event.target.value })} placeholder={t('sshCard.namePlaceholder')} /></div><div className="form-field ssh-host-field"><label>{t('sshCard.hostLabel')}</label><input className="mono-input" value={connection.host} onChange={(event) => onUpdate({ host: event.target.value })} placeholder="bastion.example.com" /></div><div className="form-field"><label>{t('sshCard.portLabel')}</label><input className="mono-input" type="number" min="1" max="65535" value={connection.port} onChange={(event) => onUpdate({ port: Number(event.target.value) })} /></div><div className="form-field"><label>{t('sshCard.usernameLabel')}</label><input className="mono-input" value={connection.username} onChange={(event) => onUpdate({ username: event.target.value })} placeholder="deploy" /></div></div>
    <div className="ssh-auth-row"><div><label className="batch-label">{t('sshCard.authModeLabel')}</label><div className="segment-control"><button type="button" className={connection.auth_mode === 'Password' ? 'active' : ''} onClick={() => onUpdate({ auth_mode: 'Password' })}><KeyRound size={13} />{t('sshCard.authPassword')}</button><button type="button" className={connection.auth_mode === 'PrivateKey' ? 'active' : ''} onClick={() => onUpdate({ auth_mode: 'PrivateKey' })}><Lock size={13} />{t('sshCard.authKey')}</button></div></div>{connection.auth_mode === 'Password' ? <div className="form-field ssh-secret-field"><label>{t('sshCard.passwordLabel')} {connection.password_configured && <span className="configured-badge"><Check size={11} />{t('common.saved')}</span>}</label><input type="password" value={connection.password} onChange={(event) => onUpdate({ password: event.target.value })} placeholder={connection.password_configured ? t('sshCard.keepPassword') : t('sshCard.enterPassword')} autoComplete="new-password" /></div> : <div className="ssh-private-key-fields"><div className="form-field"><label>{t('sshCard.privateKeyLabel')}</label><div className="field-with-action"><input className="mono-input" value={connection.private_key_path} onChange={(event) => onUpdate({ private_key_path: event.target.value })} placeholder="~/.ssh/id_ed25519" /><button type="button" className="small-button" onClick={onChoosePrivateKey}>{t('common.choose')}</button></div></div><div className="form-field"><label>{t('sshCard.passphraseLabel')} {connection.passphrase_configured && <span className="configured-badge"><Check size={11} />{t('common.saved')}</span>}</label><input type="password" value={connection.passphrase} onChange={(event) => onUpdate({ passphrase: event.target.value })} placeholder={connection.passphrase_configured ? t('sshCard.keepPassphrase') : t('sshCard.optional')} autoComplete="new-password" /></div></div>}</div>
    <div className="ssh-proxy-option"><span><Wifi size={14} /><span><strong>{t('sshCard.viaSocks5Title')}</strong><small>{socksProxyConfigured ? t('sshCard.viaSocks5Desc') : t('sshCard.socksNotEnabled')}</small></span></span><label className="switch"><input type="checkbox" checked={connection.use_system_proxy} onChange={(event) => onUpdate({ use_system_proxy: event.target.checked })} /><span /></label></div>
  </article>;
}

function HostKeyModal({ request, onConfirm }: { request: HostKeyRequest; onConfirm: (accepted: boolean) => Promise<void> }) {
  const { t } = useTranslation();
  return <div className="modal-backdrop"><div className="dialog confirm-dialog" role="alertdialog" aria-modal="true"><div className="modal-heading"><div><h2>{t('hostKey.title')}</h2><p>{t('hostKey.desc')}</p></div><CircleHelp size={22} color="#b87812" /></div><div className="dialog-scroll"><div className="host-key-copy">{request.message}<br />{t('hostKey.body')}</div><div className="fingerprint">{request.fingerprint}</div><div className="field-hint">{t('hostKey.hint')}</div></div><div className="confirm-actions"><button className="outline-button" onClick={() => void onConfirm(false)}>{t('hostKey.reject')}</button><button className="primary-button" onClick={() => void onConfirm(true)}><Check size={14} />{t('hostKey.trustAndContinue')}</button></div></div></div>;
}
