//! Privileged WireGuard helper.
//!
//! The desktop core cannot create a utun device or touch routes/DNS, so it
//! starts this helper once through `do shell script ... with administrator
//! privileges`. Because every start costs the user an administrator password,
//! the helper is deliberately long-lived and must behave as a **single
//! instance**:
//!
//! * it takes an exclusive `flock` on a well-known lock file before creating a
//!   listener, so a stale predecessor can never be silently orphaned next to a
//!   new helper (the old behaviour unlinked the socket and left the previous
//!   helper holding its utun forever);
//! * when the lock is held it reads the pid file, asks the predecessor to exit
//!   (`SIGTERM`, then `SIGKILL` after a grace period) and only then binds;
//! * both `SIGTERM`/`SIGHUP` and the `shutdown` command run the same teardown,
//!   so addresses, routes and DNS are restored instead of leaking.
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::ToSocketAddrs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use boringtun::device::{DeviceConfig, DeviceHandle};
use boringtun::x25519::{PublicKey, StaticSecret};
use local_proxy_rust::helper_paths;
use serde::Deserialize;
use serde_json::{json, Value};

/// Grace period given to a predecessor helper after `SIGTERM` before `SIGKILL`.
const TAKEOVER_TERM_GRACE: Duration = Duration::from_secs(3);
/// Time allowed for the single-instance lock to be released.
const TAKEOVER_LOCK_WAIT: Duration = Duration::from_secs(3);
/// Poll interval while waiting for the predecessor to release the lock.
const TAKEOVER_POLL: Duration = Duration::from_millis(100);
/// Setup, Stop and Shutdown all modify the same routes, DNS and device table.
/// Keep them ordered without blocking Ping or a status read.
static TUNNEL_MUTATION: Mutex<()> = Mutex::new(());
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize)]
struct Command {
    token: String,
    #[serde(flatten)]
    action: Action,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Action {
    Setup {
        id: String,
        config: WireGuardConfig,
        private_key: String,
    },
    Stop {
        id: String,
    },
    Status {
        id: String,
    },
    Ping,
    Shutdown,
}

#[derive(Debug, Deserialize)]
struct WireGuardConfig {
    addresses: Vec<String>,
    #[serde(default)]
    dns: Vec<String>,
    #[serde(default)]
    mtu: Option<u16>,
    #[serde(default)]
    listen_port: Option<u16>,
    peers: Vec<WireGuardPeer>,
}

#[derive(Debug, Deserialize)]
struct WireGuardPeer {
    public_key: String,
    endpoint_host: String,
    endpoint_port: u16,
    allowed_ips: Vec<String>,
    #[serde(default)]
    persistent_keepalive: Option<u16>,
}

/// A route this helper installed, with enough context to remove exactly it.
#[derive(Debug, Clone)]
struct RouteEntry {
    family: &'static str,
    kind: &'static str,
    target: String,
    probe: String,
    interface: String,
}

/// An interface address this helper configured.
#[derive(Debug, Clone)]
struct AddressEntry {
    family: &'static str,
    address: String,
}

struct ActiveTunnel {
    /// Dropping this handle closes the utun device; it is the only reason the
    /// interface disappears on teardown, so it is intentionally never read.
    _device: DeviceHandle,
    interface_name: String,
    listen_port: u16,
    public_key: Option<String>,
    addresses: Vec<AddressEntry>,
    routes: Vec<RouteEntry>,
    dns_service: Option<String>,
    previous_dns: Option<Vec<String>>,
    applied_dns: Vec<String>,
}

/// Small snapshot of a tunnel that can be inspected without holding the lock.
struct TunnelSnapshot {
    interface_name: String,
    listen_port: u16,
    public_key: Option<String>,
}

impl ActiveTunnel {
    fn snapshot(&self) -> TunnelSnapshot {
        TunnelSnapshot {
            interface_name: self.interface_name.clone(),
            listen_port: self.listen_port,
            public_key: self.public_key.clone(),
        }
    }
}

fn main() -> Result<(), String> {
    let mut socket_path: Option<PathBuf> = None;
    let mut token: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket_path = args.next().map(PathBuf::from),
            "--token" => token = args.next(),
            _ => {}
        }
    }
    let socket_path = socket_path.ok_or("缺少 --socket 参数")?;
    let token = token.ok_or("缺少 --token 参数")?;
    if unsafe { libc::geteuid() } != 0 {
        return Err("WireGuard helper 必须 root 运行".into());
    }
    // A multi-threaded runtime keeps the accept loop responsive while setup or
    // teardown shells out to ifconfig/route/networksetup.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(run(socket_path, token))
}

async fn run(socket_path: PathBuf, token: String) -> Result<(), String> {
    let parent = socket_path.parent().ok_or("socket 路径无效")?.to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;

    let pid_path = helper_paths::sibling(&socket_path, helper_paths::HELPER_PID_FILE);
    // Hold the lock for the whole process lifetime: it is what makes "one
    // helper per socket" true, and it is released by the kernel if we die.
    let _singleton = acquire_singleton(&socket_path, TAKEOVER_LOCK_WAIT, TAKEOVER_TERM_GRACE)?;
    // Recover from a helper that was killed without running its cleanup, and
    // from pre-v3 helpers that cannot be found through the pid file.
    sweep_stale_uapi_sockets();
    reap_orphan_helpers(&socket_path);

    // Only after winning the lock is the socket file ours to replace.
    let _ = std::fs::remove_file(&socket_path);
    let listener = tokio::net::UnixListener::bind(&socket_path).map_err(|e| e.to_string())?;
    // Root creates this socket with the default umask, which leaves it
    // non-writable for the user-side core. The 0700 parent directory keeps
    // the wider socket mode private to one user.
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o666))
        .map_err(|e| e.to_string())?;

    let tunnels: Arc<Mutex<HashMap<String, ActiveTunnel>>> = Arc::new(Mutex::new(HashMap::new()));
    let tunnels_for_shutdown = tunnels.clone();
    let socket_for_shutdown = socket_path.clone();
    let pid_for_shutdown = pid_path.clone();
    ctrlc::set_handler(move || {
        shutdown(
            &tunnels_for_shutdown,
            &socket_for_shutdown,
            &pid_for_shutdown,
        );
        std::process::exit(0);
    })
    .map_err(|e| format!("注册信号处理失败: {e}"))?;

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(value) => value,
            Err(_) => continue,
        };
        let token = token.clone();
        let tunnels = tunnels.clone();
        let socket = socket_path.clone();
        let pid = pid_path.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_stream(stream, token, tunnels, socket, pid).await {
                let _ = error;
            }
        });
    }
}

/// Tears every tunnel down and removes the files this helper owns.
fn shutdown(
    tunnels: &Arc<Mutex<HashMap<String, ActiveTunnel>>>,
    socket_path: &Path,
    pid_path: &Path,
) {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    let _mutation = TUNNEL_MUTATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Ok(mut tunnels) = tunnels.lock() {
        for (_, tunnel) in tunnels.drain() {
            cleanup_tunnel(&tunnel);
        }
    }
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_path);
}

// ---------------------------------------------------------------------------
// Single instance
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SingletonGuard {
    _file: File,
}

fn acquire_singleton(
    socket_path: &Path,
    lock_wait: Duration,
    term_grace: Duration,
) -> Result<SingletonGuard, String> {
    let lock_path = helper_paths::sibling(socket_path, helper_paths::HELPER_LOCK_FILE);
    let pid_path = helper_paths::sibling(socket_path, helper_paths::HELPER_PID_FILE);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|e| format!("打开 helper 锁文件失败: {e}"))?;

    if try_lock_exclusive(&file)? {
        return finish_singleton(file, &pid_path);
    }

    // Someone else holds the lock. Our launcher asks the predecessor to exit,
    // but the helper enforces it too: the lock, not the socket file, decides
    // who is the live instance.
    let previous = read_pid(&pid_path);
    if let Some(pid) = previous {
        // Only ever signal a process that is actually a LocalProxy helper: a
        // recycled pid must never cost an unrelated process its life.
        if pid != std::process::id() as i32 && process_alive(pid) && is_helper_process(pid) {
            unsafe { libc::kill(pid, libc::SIGTERM) };
            if wait_for_lock(&file, term_grace)? {
                return finish_singleton(file, &pid_path);
            }
            unsafe { libc::kill(pid, libc::SIGKILL) };
            if wait_for_lock(&file, lock_wait)? {
                return finish_singleton(file, &pid_path);
            }
            return Err(format!("无法接管 WireGuard helper：pid {pid} 仍持有锁"));
        }
    }

    if wait_for_lock(&file, lock_wait)? {
        return finish_singleton(file, &pid_path);
    }
    Err(format!(
        "helper 单实例锁被占用且 pid 文件无效: {}",
        pid_path.display()
    ))
}

fn finish_singleton(file: File, pid_path: &Path) -> Result<SingletonGuard, String> {
    std::fs::write(pid_path, format!("{}\n", std::process::id()))
        .map_err(|e| format!("写入 helper pid 文件失败: {e}"))?;
    let _ = std::fs::set_permissions(pid_path, std::fs::Permissions::from_mode(0o644));
    Ok(SingletonGuard { _file: file })
}

fn try_lock_exclusive(file: &File) -> Result<bool, String> {
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK => Ok(false),
        _ => Err(format!("锁定 helper 锁文件失败: {error}")),
    }
}

fn wait_for_lock(file: &File, total: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + total;
    loop {
        if try_lock_exclusive(file)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(TAKEOVER_POLL);
    }
}

fn read_pid(path: &Path) -> Option<i32> {
    let raw = std::fs::read_to_string(path).ok()?;
    raw.trim().parse::<i32>().ok().filter(|pid| *pid > 1)
}

fn process_alive(pid: i32) -> bool {
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Guards the takeover path: the pid in the pid file is signalled only when it
/// still belongs to a LocalProxy helper (pids are recycled).
fn is_helper_process(pid: i32) -> bool {
    command_output("/bin/ps", &["-o", "comm=", "-p", &pid.to_string()])
        .map(|comm| comm.trim().ends_with("local_proxy_wireguard_helper"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------

async fn handle_stream(
    stream: tokio::net::UnixStream,
    token: String,
    tunnels: Arc<Mutex<HashMap<String, ActiveTunnel>>>,
    socket_path: PathBuf,
    pid_path: PathBuf,
) -> Result<(), String> {
    let (read_half, mut writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
        .await
        .map_err(|e| e.to_string())?;
    let parsed = serde_json::from_str::<Command>(line.trim()).ok();
    let is_shutdown = parsed.as_ref().is_some_and(|command| {
        command.token == token && matches!(command.action, Action::Shutdown)
    });
    if is_shutdown {
        SHUTTING_DOWN.store(true, Ordering::SeqCst);
    }
    let result = match parsed {
        Some(command) if command.token == token => handle_command(command, &tunnels).await,
        _ => Err("未授权的 WireGuard helper 请求".into()),
    };
    let payload = serde_json::to_string(&match result {
        Ok(result) => result,
        Err(error) => json!({ "error": error }),
    })
    .map_err(|e| e.to_string())?;
    let write_result = async {
        tokio::io::AsyncWriteExt::write_all(&mut writer, payload.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        tokio::io::AsyncWriteExt::write_all(&mut writer, b"\n")
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    if is_shutdown {
        shutdown(&tunnels, &socket_path, &pid_path);
        std::process::exit(0);
    }
    write_result?;
    Ok(())
}

/// Runs one command on the blocking pool so the accept loop keeps answering
/// pings while `ifconfig`/`route`/`networksetup` are executing.
async fn handle_command(
    command: Command,
    tunnels: &Arc<Mutex<HashMap<String, ActiveTunnel>>>,
) -> Result<Value, String> {
    match command.action {
        Action::Setup {
            id,
            config,
            private_key,
        } => {
            let tunnels = tunnels.clone();
            run_blocking(move || {
                let _mutation = TUNNEL_MUTATION
                    .lock()
                    .map_err(|_| "mutation lock".to_string())?;
                if SHUTTING_DOWN.load(Ordering::SeqCst) {
                    return Err("WireGuard helper 正在退出".into());
                }
                // Serialize system changes, but only hold the tunnel map lock
                // while removing or inserting an entry.
                let previous = {
                    let mut guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
                    guard.remove(&id)
                };
                if let Some(previous) = previous {
                    cleanup_tunnel(&previous);
                }
                let tunnel = setup_tunnel(&config, private_key)?;
                let result = json!({
                    "interface_name": tunnel.interface_name,
                    "listen_port": tunnel.listen_port,
                    "public_key": tunnel.public_key,
                });
                tunnels
                    .lock()
                    .map_err(|_| "tunnel lock".to_string())?
                    .insert(id, tunnel);
                Ok(json!({ "result": result }))
            })
            .await
        }
        Action::Stop { id } => {
            let tunnels = tunnels.clone();
            run_blocking(move || {
                let _mutation = TUNNEL_MUTATION
                    .lock()
                    .map_err(|_| "mutation lock".to_string())?;
                if SHUTTING_DOWN.load(Ordering::SeqCst) {
                    return Err("WireGuard helper 正在退出".into());
                }
                let previous = {
                    let mut guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
                    guard.remove(&id)
                };
                if let Some(previous) = previous {
                    cleanup_tunnel(&previous);
                }
                Ok(json!({ "result": { "stopped": true } }))
            })
            .await
        }
        Action::Status { id } => {
            let tunnels = tunnels.clone();
            run_blocking(move || {
                let snapshot = {
                    let guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
                    guard.get(&id).map(ActiveTunnel::snapshot)
                };
                Ok(match snapshot {
                    Some(snapshot) => json!({ "result": tunnel_status(&snapshot) }),
                    None => json!({ "result": null }),
                })
            })
            .await
        }
        Action::Ping => Ok(json!({
            "result": {
                "version": helper_paths::HELPER_PROTOCOL_VERSION,
                "pid": std::process::id(),
            }
        })),
        Action::Shutdown => {
            let tunnels = tunnels.clone();
            run_blocking(move || {
                let _mutation = TUNNEL_MUTATION
                    .lock()
                    .map_err(|_| "mutation lock".to_string())?;
                let drained: Vec<ActiveTunnel> = {
                    let mut guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
                    guard.drain().map(|(_, tunnel)| tunnel).collect()
                };
                for tunnel in drained {
                    cleanup_tunnel(&tunnel);
                }
                Ok(json!({ "result": { "stopped": true } }))
            })
            .await
        }
    }
}

async fn run_blocking<F>(work: F) -> Result<Value, String>
where
    F: FnOnce() -> Result<Value, String> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| format!("WireGuard helper 任务失败: {e}"))?
}

/// Defence in depth: the core already validates, but the helper is a root
/// service whose socket lives in the user's temp directory, so it never trusts
/// a request. In particular a full-tunnel AllowedIP (`0.0.0.0/0`, `::/0`) is
/// refused here as well, and DNS values are checked so they cannot be mistaken
/// for `networksetup` options.
fn validate_config(config: &WireGuardConfig, private_key: &str) -> Result<(), String> {
    if config.addresses.is_empty() || config.addresses.len() > 8 {
        return Err("需要 1–8 个接口 Address".into());
    }
    let mut addresses = HashSet::new();
    for address in &config.addresses {
        parse_cidr(address)?;
        if !addresses.insert(address.clone()) {
            return Err(format!("Address 不能重复: {address}"));
        }
    }
    if config.dns.len() > 8 {
        return Err("最多支持 8 个 DNS 服务器".into());
    }
    for dns in &config.dns {
        validate_dns(dns)?;
    }
    decode_key(private_key).map_err(|_| "PrivateKey 必须是 32 字节 Base64 密钥".to_string())?;
    if config.peers.is_empty() || config.peers.len() > 64 {
        return Err("需要 1–64 个 [Peer] 配置".into());
    }
    let mut public_keys = HashSet::new();
    for peer in &config.peers {
        decode_key(&peer.public_key)
            .map_err(|_| format!("Peer PublicKey 无效: {}", peer.public_key))?;
        if !public_keys.insert(peer.public_key.clone()) {
            return Err("Peer PublicKey 不能重复".into());
        }
        if peer.endpoint_host.trim().is_empty() || peer.endpoint_port == 0 {
            return Err("每个 Peer 都需要 Endpoint".into());
        }
        if peer.allowed_ips.is_empty() || peer.allowed_ips.len() > 64 {
            return Err("每个 Peer 需要 1–64 个 AllowedIPs".into());
        }
        for allowed_ip in &peer.allowed_ips {
            let normalized = parse_cidr(allowed_ip)?;
            if normalized == "0.0.0.0/0" || normalized == "::/0" {
                return Err(format!("helper 拒绝接管全部流量的 AllowedIP: {normalized}"));
            }
        }
    }
    Ok(())
}

fn parse_cidr(raw: &str) -> Result<String, String> {
    let (address, prefix) = cidr_parts(raw)?;
    let ip: std::net::IpAddr = address
        .parse()
        .map_err(|_| format!("地址格式无效: {raw}"))?;
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| format!("CIDR 前缀无效: {raw}"))?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    if prefix > max {
        return Err(format!("CIDR 前缀超出范围: {raw}"));
    }
    Ok(format!("{ip}/{prefix}"))
}

fn validate_dns(value: &str) -> Result<(), String> {
    let invalid = value.is_empty()
        || value.len() > 253
        || value.starts_with('-')
        || value.chars().any(|c| {
            c.is_whitespace() || matches!(c, '/' | '\\' | '@' | '?' | ';' | '"' | '\'' | '`')
        });
    if invalid {
        return Err(format!("DNS 地址无效: {value}"));
    }
    Ok(())
}

fn setup_tunnel(config: &WireGuardConfig, private_key: String) -> Result<ActiveTunnel, String> {
    validate_config(config, &private_key)?;
    let private_bytes = base64::engine::general_purpose::STANDARD
        .decode(private_key.trim())
        .map_err(|_| "PrivateKey 必须是 Base64".to_string())?;
    let private_bytes: [u8; 32] = private_bytes
        .try_into()
        .map_err(|_| "PrivateKey 长度必须是 32 字节".to_string())?;
    let secret = StaticSecret::from(private_bytes);
    let name_file = std::env::temp_dir().join(format!(
        "localproxy-wireguard-iface-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::env::set_var("WG_TUN_NAME_FILE", &name_file);
    let device_config = DeviceConfig {
        n_threads: 2,
        use_connected_socket: true,
    };
    let device_result = DeviceHandle::new("utun", device_config);
    let name_result = std::fs::read_to_string(&name_file);
    let _ = std::fs::remove_file(&name_file);
    std::env::remove_var("WG_TUN_NAME_FILE");
    let device = device_result.map_err(|error| format!("创建 utun 设备失败: {error}"))?;
    let interface_name = name_result
        .map_err(|_| "读取实际 utun 接口名失败".to_string())?
        .trim()
        .to_string();

    let mut set_request = String::from("set=1\n");
    set_config(&mut set_request, "private_key", &hex(&secret.to_bytes()))?;
    if let Some(port) = config.listen_port {
        set_config(&mut set_request, "listen_port", &port.to_string())?;
    }
    set_config(&mut set_request, "replace_peers", "true")?;
    for peer in &config.peers {
        let endpoint = resolve_endpoint(&peer.endpoint_host, peer.endpoint_port)?;
        set_config(
            &mut set_request,
            "public_key",
            &hex(&decode_key(&peer.public_key)?),
        )?;
        set_config(&mut set_request, "endpoint", &endpoint.to_string())?;
        for allowed_ip in &peer.allowed_ips {
            set_config(&mut set_request, "allowed_ip", allowed_ip)?;
        }
        if let Some(keepalive) = peer.persistent_keepalive {
            set_config(
                &mut set_request,
                "persistent_keepalive_interval",
                &keepalive.to_string(),
            )?;
        }
    }
    // A final blank line commits the last peer; a root-level replace_peers
    // line after a peer is not valid UAPI syntax.
    set_request.push('\n');
    send_uapi(&interface_socket_path(&interface_name), &set_request)?;

    let mut addresses = Vec::new();
    let mut routes = Vec::new();
    let mut dns_service = None;
    let mut previous_dns = None;
    let mut applied_dns = Vec::new();

    // From here on every step is best-effort rolled back on failure so a
    // partially configured interface can never outlive this call.
    let setup = (|| -> Result<(), String> {
        for address in &config.addresses {
            configure_interface_address(&interface_name, address)?;
            addresses.push(address_entry(address)?);
        }
        for peer in &config.peers {
            for allowed_ip in &peer.allowed_ips {
                add_route(&interface_name, allowed_ip, &mut routes)?;
            }
        }
        if let Some(mtu) = config.mtu {
            run_command(
                "/sbin/ifconfig",
                &[&interface_name, "mtu", &mtu.to_string()],
            )?;
        }
        if !config.dns.is_empty() {
            let service = first_network_service()?;
            previous_dns = read_dns(&service).ok().flatten();
            configure_dns(&service, &config.dns)?;
            dns_service = Some(service);
            applied_dns = normalize_dns(&config.dns);
        }
        Ok(())
    })();

    if let Err(error) = setup {
        let tunnel = ActiveTunnel {
            _device: device,
            interface_name,
            listen_port: config.listen_port.unwrap_or(0),
            public_key: Some(
                base64::engine::general_purpose::STANDARD
                    .encode(PublicKey::from(&secret).as_bytes()),
            ),
            addresses,
            routes,
            dns_service,
            previous_dns,
            applied_dns,
        };
        cleanup_tunnel(&tunnel);
        return Err(error);
    }

    Ok(ActiveTunnel {
        _device: device,
        interface_name,
        listen_port: config.listen_port.unwrap_or(0),
        public_key: Some(
            base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret).as_bytes()),
        ),
        addresses,
        routes,
        dns_service,
        previous_dns,
        applied_dns,
    })
}

fn set_config(request: &mut String, key: &str, value: &str) -> Result<(), String> {
    if value.chars().any(|c| c == '\n' || c == '\r') {
        return Err("WireGuard 配置包含非法换行".into());
    }
    request.push_str(key);
    request.push('=');
    request.push_str(value);
    request.push('\n');
    Ok(())
}

fn resolve_endpoint(host: &str, port: u16) -> Result<std::net::SocketAddr, String> {
    let addresses = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|_| format!("Endpoint 解析失败: {host}"))?
        .collect::<Vec<_>>();
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or_else(|| addresses.first())
        .copied()
        .ok_or_else(|| format!("Endpoint 解析失败: {host}"))
}

fn decode_key(value: &str) -> Result<[u8; 32], String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|_| "公钥必须是 Base64".to_string())?;
    bytes
        .try_into()
        .map_err(|_| "公钥长度必须是 32 字节".to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn interface_socket_path(interface_name: &str) -> String {
    format!("/var/run/wireguard/{interface_name}.sock")
}

/// Removes UAPI sockets whose utun no longer exists. Those are left behind when
/// a helper is killed hard (`SIGKILL`) and would otherwise accumulate next to
/// the fresh `/var/run/wireguard/` sockets. A socket whose interface still
/// exists belongs to a live device (possibly another tool) and is never touched.
fn sweep_stale_uapi_sockets() {
    let Ok(entries) = std::fs::read_dir("/var/run/wireguard") else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(interface) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".sock"))
        else {
            continue;
        };
        if !interface.starts_with("utun") {
            continue;
        }
        if interface_exists(interface) {
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// Reaps helpers for this socket that survived an older protocol version:
/// before v3 a helper wrote no pid file, so the takeover above cannot find it
/// and it keeps holding its utun forever. A helper is only signalled when its
/// executable really is `local_proxy_wireguard_helper` *and* its argv carries
/// this exact socket path, so nothing else can be hit.
fn reap_orphan_helpers(socket_path: &Path) {
    let expected = format!("--socket {}", socket_path.display());
    let Ok(output) = ProcessCommand::new("/usr/bin/pgrep")
        .args(["-f", "local_proxy_wireguard_helper"])
        .output()
    else {
        return;
    };
    let me = std::process::id() as i32;
    let victims: Vec<i32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .filter(|pid| *pid != me && process_alive(*pid) && is_helper_process(*pid))
        .filter(|pid| {
            command_output("/bin/ps", &["-o", "command=", "-p", &pid.to_string()])
                .is_some_and(|argv| argv.contains(&expected))
        })
        .collect();
    if victims.is_empty() {
        return;
    }
    for pid in &victims {
        unsafe { libc::kill(*pid, libc::SIGTERM) };
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && victims.iter().any(|pid| process_alive(*pid)) {
        std::thread::sleep(TAKEOVER_POLL);
    }
    for pid in victims.iter().filter(|pid| process_alive(**pid)) {
        unsafe { libc::kill(*pid, libc::SIGKILL) };
    }
}

fn interface_exists(interface_name: &str) -> bool {
    ProcessCommand::new("/sbin/ifconfig")
        .arg(interface_name)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(true)
}

fn send_uapi(socket_path: &str, request: &str) -> Result<String, String> {
    let mut stream =
        UnixStream::connect(socket_path).map_err(|e| format!("连接 WireGuard UAPI 失败: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("写入 WireGuard 配置失败: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;
    let mut response = String::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|e| format!("读取 WireGuard 配置结果失败: {e}"))?;
        if count == 0 {
            break;
        }
        response.push_str(&String::from_utf8_lossy(&buffer[..count]));
        if response.contains("errno=") && response.ends_with("\n\n") {
            break;
        }
    }
    if !response.contains("errno=0") {
        return Err(format!("WireGuard 配置写入失败: {}", response.trim()));
    }
    Ok(response)
}

fn read_uapi(socket_path: &str) -> Result<String, String> {
    let mut stream =
        UnixStream::connect(socket_path).map_err(|e| format!("连接 WireGuard UAPI 失败: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .map_err(|e| e.to_string())?;
    stream.write_all(b"get=1\n\n").map_err(|e| e.to_string())?;
    let mut response = String::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => response.push_str(&String::from_utf8_lossy(&buffer[..count])),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(response)
}

fn run_command(program: &str, args: &[&str]) -> Result<(), String> {
    let output = ProcessCommand::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("执行 {program} 失败: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(format!("{program} 执行失败: {message}"))
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new(program).args(args).output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn route_entry(interface_name: &str, allowed_ip: &str) -> Result<RouteEntry, String> {
    let (address, prefix) = allowed_ip
        .rsplit_once('/')
        .ok_or_else(|| format!("AllowedIP 无效: {allowed_ip}"))?;
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| format!("AllowedIP 前缀无效: {allowed_ip}"))?;
    let is_ipv4 = address
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_ipv4());
    Ok(RouteEntry {
        family: if is_ipv4 { "-inet" } else { "-inet6" },
        kind: if is_ipv4 && prefix == 32 {
            "-host"
        } else {
            "-net"
        },
        target: format!("{address}/{prefix}"),
        probe: address.to_string(),
        interface: interface_name.to_string(),
    })
}

fn add_route(
    interface_name: &str,
    allowed_ip: &str,
    routes: &mut Vec<RouteEntry>,
) -> Result<(), String> {
    let entry = route_entry(interface_name, allowed_ip)?;
    // An existing route may belong to another VPN or to the user. A route
    // lookup cannot distinguish it from a stale LocalProxy route, so never
    // remove it as a side effect of starting this tunnel.
    route_add(&entry).map_err(|error| format!("无法添加路由 {}: {error}", entry.target))?;
    routes.push(entry);
    Ok(())
}

fn route_add(entry: &RouteEntry) -> Result<(), String> {
    run_command(
        "/sbin/route",
        &[
            "-n",
            "add",
            entry.family,
            entry.kind,
            &entry.target,
            "-interface",
            &entry.interface,
        ],
    )
}

fn delete_route_plain(entry: &RouteEntry) {
    let _ = run_command(
        "/sbin/route",
        &["-n", "delete", entry.family, entry.kind, &entry.target],
    );
}

/// Removes only the route this helper installed: the qualified delete only
/// matches when the destination is still owned by our interface, and the
/// fallback re-checks ownership with `route get` first. Without this a stale
/// helper could delete a live tunnel's route for the same AllowedIPs.
fn delete_route(route: &RouteEntry) {
    let qualified = run_command(
        "/sbin/route",
        &[
            "-n",
            "delete",
            route.family,
            route.kind,
            &route.target,
            "-interface",
            &route.interface,
        ],
    );
    if qualified.is_ok() {
        return;
    }
    if route_interface(&route.probe).as_deref() != Some(route.interface.as_str()) {
        return;
    }
    delete_route_plain(route);
}

/// Interface currently owning the route for `address`, if any.
fn route_interface(address: &str) -> Option<String> {
    let output = command_output("/sbin/route", &["-n", "get", address])?;
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("interface:")
            .map(|value| value.trim().to_string())
    })
}

fn address_entry(cidr: &str) -> Result<AddressEntry, String> {
    let (address, _) = cidr_parts(cidr)?;
    let is_ipv4 = address
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_ipv4());
    Ok(AddressEntry {
        family: if is_ipv4 { "inet" } else { "inet6" },
        address: address.to_string(),
    })
}

fn configure_interface_address(interface_name: &str, cidr: &str) -> Result<(), String> {
    let (address, prefix) = cidr_parts(cidr)?;
    let is_ipv4 = address
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_ipv4());
    if is_ipv4 {
        run_command(
            "/sbin/ifconfig",
            &[interface_name, "inet", address, address, "alias", "up"],
        )
    } else {
        run_command(
            "/sbin/ifconfig",
            &[interface_name, "inet6", address, "prefixlen", prefix, "up"],
        )
    }
}

fn remove_interface_address(interface_name: &str, entry: &AddressEntry) {
    let _ = run_command(
        "/sbin/ifconfig",
        &[interface_name, entry.family, &entry.address, "delete"],
    );
}

fn cidr_parts(cidr: &str) -> Result<(&str, &str), String> {
    cidr.rsplit_once('/')
        .ok_or_else(|| format!("地址无效: {cidr}"))
}

fn first_network_service() -> Result<String, String> {
    let output = ProcessCommand::new("/usr/sbin/networksetup")
        .arg("-listallnetworkservices")
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .skip(1)
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('*'))
        .map(str::to_string)
        .ok_or_else(|| "没有可用的网络服务".to_string())
}

fn configure_dns(service: &str, servers: &[String]) -> Result<(), String> {
    let mut args = vec!["-setdnsservers".to_string(), service.to_string()];
    args.extend(servers.iter().cloned());
    run_command(
        "/usr/sbin/networksetup",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

fn read_dns(service: &str) -> Result<Option<Vec<String>>, String> {
    let output = ProcessCommand::new("/usr/sbin/networksetup")
        .args(["-getdnsservers", service])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.starts_with("There aren't any") {
        return Ok(None);
    }
    let servers: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if servers.is_empty() {
        return Ok(None);
    }
    Ok(Some(servers))
}

fn normalize_dns(values: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = values
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

/// Restores the DNS values we replaced, but only when the current
/// configuration is still ours: with several tunnels or a user edit in
/// between, clobbering whatever is configured now would be worse than leaving
/// it alone.
fn restore_dns_if_owned(tunnel: &ActiveTunnel) {
    let Some(service) = tunnel.dns_service.as_ref() else {
        return;
    };
    let current = normalize_dns(&read_dns(service).ok().flatten().unwrap_or_default());
    if current != normalize_dns(&tunnel.applied_dns) {
        return;
    }
    match tunnel.previous_dns.as_ref() {
        Some(dns) if !dns.is_empty() => {
            let _ = configure_dns(service, dns);
        }
        _ => {
            let _ = run_command(
                "/usr/sbin/networksetup",
                &["-setdnsservers", service, "empty"],
            );
        }
    }
}

fn cleanup_tunnel(tunnel: &ActiveTunnel) {
    for route in &tunnel.routes {
        delete_route(route);
    }
    for address in &tunnel.addresses {
        remove_interface_address(&tunnel.interface_name, address);
    }
    restore_dns_if_owned(tunnel);
}

fn tunnel_status(snapshot: &TunnelSnapshot) -> Value {
    let response = read_uapi(&interface_socket_path(&snapshot.interface_name)).unwrap_or_default();
    let mut peers = Vec::new();
    let mut public_key: Option<String> = None;
    let mut endpoint: Option<String> = None;
    let mut allowed_ips = Vec::new();
    let mut keepalive: Option<u16> = None;
    let mut handshake: Option<u64> = None;
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in response.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "public_key" => {
                if let Some(key) = public_key.take() {
                    peers.push(status_peer(
                        key,
                        endpoint.clone(),
                        allowed_ips.clone(),
                        keepalive,
                        handshake,
                        rx,
                        tx,
                    ));
                }
                public_key = Some(bytes_to_base64(&hex_decode(value)));
                endpoint = None;
                allowed_ips.clear();
                keepalive = None;
                handshake = None;
                rx = 0;
                tx = 0;
            }
            "endpoint" => endpoint = Some(value.to_string()),
            "allowed_ip" => allowed_ips.push(value.to_string()),
            "persistent_keepalive_interval" => keepalive = value.parse().ok(),
            "last_handshake_time_sec" => {
                let seconds = value.parse::<u64>().unwrap_or(0);
                handshake = Some(
                    std::time::UNIX_EPOCH
                        .elapsed()
                        .map(|now| now.saturating_sub(Duration::from_secs(seconds)))
                        .unwrap_or_default()
                        .as_secs(),
                );
            }
            "rx_bytes" => rx = value.parse().unwrap_or(0),
            "tx_bytes" => tx = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    if let Some(key) = public_key.take() {
        peers.push(status_peer(
            key,
            endpoint.clone(),
            allowed_ips.clone(),
            keepalive,
            handshake,
            rx,
            tx,
        ));
    }
    json!({
        "interface_name": snapshot.interface_name,
        "listen_port": snapshot.listen_port,
        "public_key": snapshot.public_key,
        "peers": peers,
    })
}

fn status_peer(
    public_key: String,
    endpoint: Option<String>,
    allowed_ips: Vec<String>,
    persistent_keepalive: Option<u16>,
    handshake_age_sec: Option<u64>,
    rx_bytes: u64,
    tx_bytes: u64,
) -> Value {
    json!({
        "public_key": public_key,
        "endpoint": endpoint,
        "allowed_ips": allowed_ips,
        "persistent_keepalive": persistent_keepalive,
        "handshake_age_sec": handshake_age_sec,
        "rx_bytes": rx_bytes,
        "tx_bytes": tx_bytes,
    })
}

fn hex_decode(value: &str) -> Vec<u8> {
    let bytes = value.trim().as_bytes();
    bytes
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0))
        .collect()
}

fn bytes_to_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIVATE_KEY: &str = "TYjv11EHGusEY8Y/8Lt4kTMEnmIxUsGLI0ZBqhzmi6A=";
    const PUBLIC_KEY: &str = "kwJxAs348vRB9EV3TFbeto7YKJxM3Em25lhyozDDNh4=";

    fn valid_config() -> Value {
        json!({
            "addresses": ["192.168.1.6/32"],
            "dns": ["223.5.5.5"],
            "mtu": 1420,
            "listen_port": 50814,
            "peers": [{
                "public_key": PUBLIC_KEY,
                "endpoint_host": "localhost",
                "endpoint_port": 50814,
                "allowed_ips": ["172.16.110.0/24", "172.31.0.2/32"],
                "persistent_keepalive": 25
            }]
        })
    }

    fn config_of(value: Value) -> WireGuardConfig {
        serde_json::from_value(value).unwrap()
    }

    /// The helper is a root service reachable from the user's session, so it
    /// validates every request even though the core already did.
    #[test]
    fn validation_accepts_a_realistic_request() {
        let config = config_of(valid_config());
        assert!(validate_config(&config, PRIVATE_KEY).is_ok());
    }

    #[test]
    fn validation_refuses_full_tunnel_routes() {
        let mut value = valid_config();
        value["peers"][0]["allowed_ips"] = json!(["0.0.0.0/0"]);
        let error = validate_config(&config_of(value), PRIVATE_KEY).unwrap_err();
        assert!(error.contains("AllowedIP"), "unexpected error: {error}");

        let mut value = valid_config();
        value["peers"][0]["allowed_ips"] = json!(["::/0"]);
        assert!(validate_config(&config_of(value), PRIVATE_KEY).is_err());

        // A prefix wider than the address family is rejected as well.
        let mut value = valid_config();
        value["peers"][0]["allowed_ips"] = json!(["172.16.110.0/33"]);
        assert!(validate_config(&config_of(value), PRIVATE_KEY).is_err());
    }

    #[test]
    fn validation_refuses_unsafe_dns_and_bad_keys() {
        let mut value = valid_config();
        value["dns"] = json!(["-setdnsservers"]);
        assert!(validate_config(&config_of(value), PRIVATE_KEY).is_err());

        let mut value = valid_config();
        value["dns"] = json!(["223.5.5.5 1.1.1.1"]);
        assert!(validate_config(&config_of(value), PRIVATE_KEY).is_err());

        let mut value = valid_config();
        value["peers"][0]["public_key"] = json!("not-base64");
        assert!(validate_config(&config_of(value), PRIVATE_KEY).is_err());

        let mut value = valid_config();
        value["addresses"] = json!(["192.168.1.6/32", "192.168.1.6/32"]);
        assert!(validate_config(&config_of(value), PRIVATE_KEY).is_err());

        assert!(validate_config(&config_of(valid_config()), "short").is_err());
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "localproxy-helper-test-{name}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sibling_paths_derive_from_socket() {
        let socket = Path::new("/tmp/example/localproxy-wireguard.sock");
        assert_eq!(
            helper_paths::sibling(socket, helper_paths::HELPER_PID_FILE),
            Path::new("/tmp/example/localproxy-wireguard.pid")
        );
        assert_eq!(
            helper_paths::sibling(socket, helper_paths::HELPER_LOCK_FILE),
            Path::new("/tmp/example/localproxy-wireguard.lock")
        );
    }

    #[test]
    fn singleton_lock_is_exclusive() {
        let dir = scratch_dir("lock");
        let socket = dir.join("helper.sock");
        let guard = acquire_singleton(&socket, Duration::ZERO, Duration::ZERO).unwrap();
        // A second acquisition in another process would fail; simulate the
        // competing instance with a second file description of the same lock.
        let lock_path = helper_paths::sibling(&socket, helper_paths::HELPER_LOCK_FILE);
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        assert!(!try_lock_exclusive(&competing).unwrap());
        drop(guard);
        assert!(try_lock_exclusive(&competing).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn takeover_reports_dead_pid_holder() {
        let dir = scratch_dir("stale");
        let socket = dir.join("helper.sock");
        let lock_path = helper_paths::sibling(&socket, helper_paths::HELPER_LOCK_FILE);
        let pid_path = helper_paths::sibling(&socket, helper_paths::HELPER_PID_FILE);
        // A dead pid must never block a new helper forever.
        std::fs::write(&pid_path, "999999\n").unwrap();
        let holder = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        assert!(try_lock_exclusive(&holder).unwrap());
        let error = acquire_singleton(&socket, Duration::ZERO, Duration::ZERO).unwrap_err();
        assert!(error.contains("单实例锁"), "unexpected error: {error}");
        drop(holder);
        assert!(acquire_singleton(&socket, Duration::ZERO, Duration::ZERO).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dns_ownership_ignores_order_and_case() {
        let configured = normalize_dns(&["223.5.5.5".into(), "172.31.0.2".into()]);
        let current = normalize_dns(&["172.31.0.2 ".into(), "223.5.5.5".into()]);
        assert_eq!(configured, current);
        assert_ne!(configured, normalize_dns(&["1.1.1.1".into()]));
    }

    #[test]
    fn route_entries_choose_family_and_kind() {
        let host = route_entry("utun9", "172.31.0.2/32").unwrap();
        assert_eq!(host.family, "-inet");
        assert_eq!(host.kind, "-host");
        assert_eq!(host.probe, "172.31.0.2");
        let net = route_entry("utun9", "172.16.110.0/24").unwrap();
        assert_eq!(net.kind, "-net");
        assert_eq!(net.target, "172.16.110.0/24");
        let v6 = route_entry("utun9", "fd00:1234::/64").unwrap();
        assert_eq!(v6.family, "-inet6");
        assert_eq!(v6.kind, "-net");
    }

    #[test]
    fn address_entries_map_to_ifconfig_families() {
        let v4 = address_entry("192.168.1.6/32").unwrap();
        assert_eq!(v4.family, "inet");
        assert_eq!(v4.address, "192.168.1.6");
        let v6 = address_entry("fd00:1234:ffff::10/128").unwrap();
        assert_eq!(v6.family, "inet6");
    }
}
