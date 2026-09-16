export type ProxyKind = 'Http' | 'Ssh' | 'Wstunnel' | 'WireGuard';
export type ServiceState =
  | 'Stopped'
  | 'Starting'
  | 'AwaitingHostKey'
  | 'Running'
  | 'Reconnecting'
  | 'Error';
export type AuthMode = 'Password' | 'PrivateKey';
export type SecretPatchKind = 'Keep' | 'Set' | 'Clear';

export type Endpoint = {
  kind: string;
  host: string;
  port: number;
  description: string;
};

export type ProfileSummary = {
  id: string;
  name: string;
  tags: string[];
  kind: ProxyKind;
  state: ServiceState;
  endpoints: Endpoint[];
  error: string | null;
};

export type SecretFieldView = {
  name: string;
  configured: boolean;
};

export type HttpView = {
  listen_port: number;
  upstream_url: string;
  headers: SecretFieldView[];
  cookies: SecretFieldView[];
  use_system_proxy: boolean;
};

export type ForwardView = {
  local_port: number;
  remote_host: string;
  remote_port: number;
  tag: string;
};

export type SshView = {
  connection_id: string;
  connection_name: string;
  host: string;
  port: number;
  username: string;
  auth_mode: AuthMode;
  private_key_path: string;
  password_configured: boolean;
  passphrase_configured: boolean;
  forwards: ForwardView[];
  use_system_proxy: boolean;
};

export type WstunnelConfig = {
  server_url: string;
  http_upgrade_path_prefix: string;
  tls_verify_certificate: boolean;
  forwards: { protocol: string; local_port: number; remote_host: string; remote_port: number; timeout_sec: number | null }[];
};

export type WireGuardPeer = {
  public_key: string;
  endpoint_host: string;
  endpoint_port: number;
  allowed_ips: string[];
  persistent_keepalive: number | null;
};

export type WireGuardConfig = {
  addresses: string[];
  dns: string[];
  mtu: number | null;
  listen_port: number | null;
  private_key: SecretPatch;
  peers: WireGuardPeer[];
};

export type WireGuardView = {
  addresses: string[];
  dns: string[];
  mtu: number | null;
  listen_port: number | null;
  private_key_configured: boolean;
  peers: WireGuardPeer[];
};

export type WireGuardStatus = {
  interface_name: string;
  listen_port: number;
  public_key: string | null;
  peers: {
    public_key: string;
    endpoint: string | null;
    allowed_ips: string[];
    persistent_keepalive: number | null;
    handshake_age_sec: number | null;
    rx_bytes: number;
    tx_bytes: number;
  }[];
};

export type ProfileView = {
  id: string;
  name: string;
  tags: string[];
  kind: ProxyKind;
  state: ServiceState;
  endpoints: Endpoint[];
  error: string | null;
  http: HttpView | null;
  ssh: SshView | null;
  wstunnel?: WstunnelConfig | null;
  wireguard?: WireGuardView | null;
};

export type SecretPatch = {
  name: string;
  value: string | null;
  kind: SecretPatchKind;
};

export type ForwardDraft = {
  local_port: number;
  remote_host: string;
  remote_port: number;
  tag: string;
};

export type ProfileDraft = {
  wstunnel?: WstunnelConfig | null;
  wireguard?: WireGuardConfig | null;
  id: string | null;
  name: string;
  tags: string[];
  kind: ProxyKind;
  http: {
    listen_port: number;
    upstream_url: string;
    headers: SecretPatch[];
    cookies: SecretPatch[];
    use_system_proxy: boolean;
  } | null;
  ssh: {
    connection_id: string;
    forwards: ForwardDraft[];
  } | null;
};

export type ProxyEndpoint = {
  host: string;
  port: number;
};

export type AppSettings = {
  http_proxy: ProxyEndpoint | null;
  socks_proxy: ProxyEndpoint | null;
  ssh_connections: SshConnection[];
};

export type SshConnection = {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_mode: AuthMode;
  private_key_path: string;
  password_configured: boolean;
  passphrase_configured: boolean;
  use_system_proxy: boolean;
};

export type SshConnectionDraft = {
  id: string | null;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_mode: AuthMode;
  private_key_path: string;
  password: SecretPatch;
  passphrase: SecretPatch;
  use_system_proxy: boolean;
};

export type ProxyEvent = {
  profile_id: string;
  kind: 'State' | 'HostKey' | 'Log';
  state: ServiceState;
  message: string | null;
  fingerprint: string | null;
  endpoints: Endpoint[];
};

export type LogEntry = {
  id: string;
  time: string;
  state: ServiceState;
  message: string;
};

export type SecretRow = {
  id: string;
  name: string;
  value: string;
  configured: boolean;
  originalName?: string;
};

export type RustBridge = {
  isElectron: boolean;
  platform: string;
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  request: <T>(method: string, params?: unknown) => Promise<T>;
  choosePrivateKey: () => Promise<string | null>;
  onRustEvent: (callback: (event: { event: string; payload: unknown }) => void) => () => void;
};

declare global {
  interface Window {
    electronAPI?: RustBridge;
  }
}
