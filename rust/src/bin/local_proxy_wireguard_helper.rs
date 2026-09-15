use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::ToSocketAddrs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use boringtun::device::{DeviceConfig, DeviceHandle};
use boringtun::x25519::{PublicKey, StaticSecret};
use serde::Deserialize;
use serde_json::json;

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

struct ActiveTunnel {
    device: DeviceHandle,
    interface_name: String,
    listen_port: u16,
    public_key: Option<String>,
    routes: Vec<String>,
    dns_service: Option<String>,
    previous_dns: Option<Vec<String>>,
}

fn main() -> Result<(), String> {
    let mut socket_path: Option<String> = None;
    let mut token: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket_path = args.next(),
            "--token" => token = args.next(),
            _ => {}
        }
    }
    let socket_path = socket_path.ok_or("缺少 --socket 参数")?;
    let token = token.ok_or("缺少 --token 参数")?;
    if unsafe { libc::geteuid() } != 0 {
        return Err("WireGuard helper 必须 root 运行".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(run(socket_path, token))
}

async fn run(socket_path: String, token: String) -> Result<(), String> {
    let parent = std::path::Path::new(&socket_path)
        .parent()
        .ok_or("socket 路径无效")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&socket_path);
    let listener = tokio::net::UnixListener::bind(&socket_path).map_err(|e| e.to_string())?;
    // Root creates this socket with the default umask, which leaves it
    // non-writable for the user-side core. The 0700 parent directory keeps
    // the wider socket mode private to one user.
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o666))
        .map_err(|e| e.to_string())?;
    let tunnels: Arc<Mutex<HashMap<String, ActiveTunnel>>> = Arc::new(Mutex::new(HashMap::new()));
    let tunnels_for_shutdown = tunnels.clone();
    ctrlc::set_handler(move || {
        if let Ok(mut tunnels) = tunnels_for_shutdown.lock() {
            let ids: Vec<String> = tunnels.keys().cloned().collect();
            for id in ids {
                if let Some(tunnel) = tunnels.remove(&id) {
                    cleanup_tunnel(&tunnel);
                }
            }
        }
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
        tokio::spawn(async move {
            if let Err(error) = handle_stream(stream, token, tunnels).await {
                let _ = error;
            }
        });
    }
}

async fn handle_stream(
    stream: tokio::net::UnixStream,
    token: String,
    tunnels: Arc<Mutex<HashMap<String, ActiveTunnel>>>,
) -> Result<(), String> {
    let (read_half, mut writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
        .await
        .map_err(|e| e.to_string())?;
    let parsed = serde_json::from_str::<Command>(line.trim()).ok();
    let is_shutdown = parsed
        .as_ref()
        .is_some_and(|command| matches!(command.action, Action::Shutdown));
    let result = match parsed {
        Some(command) if command.token == token => handle_command(command, &tunnels).await,
        _ => Err("未授权的 WireGuard helper 请求".into()),
    };
    let payload = serde_json::to_string(&match result {
        Ok(result) => serde_json::Value::Object(
            result
                .as_object()
                .map(|object| object.clone())
                .unwrap_or_default(),
        ),
        Err(error) => json!({ "error": error }),
    })
    .map_err(|e| e.to_string())?;
    tokio::io::AsyncWriteExt::write_all(&mut writer, payload.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    tokio::io::AsyncWriteExt::write_all(&mut writer, b"\n")
        .await
        .map_err(|e| e.to_string())?;
    if is_shutdown {
        std::process::exit(0);
    }
    Ok(())
}

async fn handle_command(
    command: Command,
    tunnels: &Arc<Mutex<HashMap<String, ActiveTunnel>>>,
) -> Result<serde_json::Value, String> {
    match command.action {
        Action::Setup {
            id,
            config,
            private_key,
        } => {
            let mut tunnels_guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
            if let Some(existing) = tunnels_guard.remove(&id) {
                cleanup_tunnel(&existing);
            }
            let tunnel = setup_tunnel(&config, private_key).map_err(|error| error)?;
            let result = json!({
                "interface_name": tunnel.interface_name,
                "listen_port": tunnel.listen_port,
                "public_key": tunnel.public_key,
            });
            tunnels_guard.insert(id, tunnel);
            Ok(json!({ "result": result }))
        }
        Action::Stop { id } => {
            let mut tunnels_guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
            if let Some(tunnel) = tunnels_guard.remove(&id) {
                cleanup_tunnel(&tunnel);
            }
            Ok(json!({ "result": { "stopped": true } }))
        }
        Action::Status { id } => {
            let tunnels_guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
            match tunnels_guard.get(&id) {
                Some(tunnel) => Ok(json!({ "result": tunnel_status(tunnel) })),
                None => Ok(json!({ "result": null })),
            }
        }
        Action::Ping => Ok(json!({ "result": { "version": 2 } })),
        Action::Shutdown => {
            let mut tunnels_guard = tunnels.lock().map_err(|_| "tunnel lock".to_string())?;
            for (_, tunnel) in tunnels_guard.drain() {
                cleanup_tunnel(&tunnel);
            }
            Ok(json!({ "result": { "stopped": true } }))
        }
    }
}

fn setup_tunnel(config: &WireGuardConfig, private_key: String) -> Result<ActiveTunnel, String> {
    let private_bytes = base64::engine::general_purpose::STANDARD
        .decode(private_key.trim())
        .map_err(|_| "PrivateKey 必须是 Base64".to_string())?;
    let private_bytes: [u8; 32] = private_bytes
        .try_into()
        .map_err(|_| "PrivateKey 长度必须是 32 字节".to_string())?;
    let secret = StaticSecret::from(private_bytes);
    let name_file = std::env::temp_dir().join("localproxy-wireguard-iface");
    let _ = std::fs::remove_file(&name_file);
    std::env::set_var("WG_TUN_NAME_FILE", &name_file);
    let device_config = DeviceConfig {
        n_threads: 2,
        use_connected_socket: true,
    };
    let device = DeviceHandle::new("utun", device_config)
        .map_err(|error| format!("创建 utun 设备失败: {error}"))?;
    let interface_name = std::fs::read_to_string(&name_file)
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

    for address in &config.addresses {
        configure_interface_address(&interface_name, address)?;
    }
    let mut routes = Vec::new();
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

    let previous_dns = if config.dns.is_empty() {
        None
    } else {
        first_network_service()
            .and_then(|service| read_dns(&service))
            .ok()
            .flatten()
    };
    let dns_result = if config.dns.is_empty() {
        Ok(None)
    } else {
        configure_dns(&config.dns).map(Some)
    };
    let dns_service = match dns_result {
        Ok(service) => service,
        Err(error) => {
            let tunnel = ActiveTunnel {
                device,
                interface_name,
                listen_port: config.listen_port.unwrap_or(0),
                public_key: Some(
                    base64::engine::general_purpose::STANDARD
                        .encode(PublicKey::from(&secret).as_bytes()),
                ),
                routes,
                dns_service: None,
                previous_dns: None,
            };
            cleanup_tunnel(&tunnel);
            return Err(error);
        }
    };
    Ok(ActiveTunnel {
        device,
        interface_name,
        listen_port: config.listen_port.unwrap_or(0),
        public_key: Some(
            base64::engine::general_purpose::STANDARD.encode(PublicKey::from(&secret).as_bytes()),
        ),
        routes,
        dns_service,
        previous_dns,
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

fn add_route(
    interface_name: &str,
    allowed_ip: &str,
    routes: &mut Vec<String>,
) -> Result<(), String> {
    let (address, prefix) = allowed_ip
        .rsplit_once('/')
        .ok_or_else(|| format!("AllowedIP 无效: {allowed_ip}"))?;
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| format!("AllowedIP 前缀无效: {allowed_ip}"))?;
    let is_ipv4 = address
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_ipv4());
    let family = if is_ipv4 { "-inet" } else { "-inet6" };
    let target = if is_ipv4 && prefix == 32 {
        "-host"
    } else {
        "-net"
    };
    let route = format!("{address}/{prefix}");
    run_command(
        "/sbin/route",
        &[
            "-n",
            "add",
            family,
            target,
            &route,
            "-interface",
            interface_name,
        ],
    )?;
    routes.push(route);
    Ok(())
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
            &[
                interface_name,
                "inet6",
                address,
                "prefixlen",
                &prefix.to_string(),
                "up",
            ],
        )
    }
}

fn cidr_parts(cidr: &str) -> Result<(&str, &str), String> {
    cidr.rsplit_once('/')
        .ok_or_else(|| format!("地址无效: {cidr}").into())
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

fn configure_dns(servers: &[String]) -> Result<String, String> {
    let service = first_network_service()?;
    let mut args = vec!["-setdnsservers".to_string(), service.clone()];
    args.extend(servers.iter().cloned());
    run_command(
        "/usr/sbin/networksetup",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    Ok(service)
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
    Ok(Some(
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    ))
}

fn restore_dns(tunnel: &ActiveTunnel) {
    let Some(service) = tunnel.dns_service.as_ref() else {
        return;
    };
    match tunnel.previous_dns.as_ref() {
        Some(dns) if !dns.is_empty() => {
            let mut args = vec!["-setdnsservers".to_string(), service.clone()];
            args.extend(dns.iter().cloned());
            let _ = ProcessCommand::new("/usr/sbin/networksetup")
                .args(&args)
                .output();
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
        let _ = run_command("/sbin/route", &["-n", "delete", route]);
    }
    if tunnel.dns_service.is_some() {
        restore_dns(tunnel);
    }
}

fn tunnel_status(tunnel: &ActiveTunnel) -> serde_json::Value {
    let response = read_uapi(&interface_socket_path(&tunnel.interface_name)).unwrap_or_default();
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
        "interface_name": tunnel.interface_name,
        "listen_port": tunnel.listen_port,
        "public_key": tunnel.public_key,
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
) -> serde_json::Value {
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
