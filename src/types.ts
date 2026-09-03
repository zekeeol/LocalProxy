export type ProxyKind = 'Http' | 'Ssh';
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
    host: string;
    port: number;
    username: string;
    auth_mode: AuthMode;
    private_key_path: string;
    password: SecretPatch;
    passphrase: SecretPatch;
    forwards: ForwardDraft[];
    use_system_proxy: boolean;
  } | null;
};

export type ProxyEndpoint = {
  host: string;
  port: number;
};

export type AppSettings = {
  http_proxy: ProxyEndpoint | null;
  socks_proxy: ProxyEndpoint | null;
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
