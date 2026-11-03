//! Background bridge between the GPUI foreground thread and the networking
//! core.
//!
//! The core owns a Tokio runtime for its services and exposes the same typed
//! API the Electron sidecar used to wrap in NDJSON. The native UI calls that
//! API in-process:
//!
//! * synchronous calls (`save_profile`, `list_profiles`, …) run directly on
//!   the foreground thread — they are file/state operations and return fast;
//! * long-running calls (`wireguard_status`, wake-time revalidation) are
//!   forwarded to a dedicated Tokio thread and delivered back through a
//!   lock-protected queue polled by the UI;
//! * core events (`ProxyEvent`) are forwarded the same way, replacing the
//!   Electron IPC event channel.

use std::sync::{mpsc, Arc, Mutex, OnceLock};

use local_proxy_rust::api::local_proxy::{self, ProxyEvent, WireGuardStatus};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Updates produced off the foreground thread.
#[derive(Debug)]
pub enum CoreUpdate {
    Event(ProxyEvent),
    WireGuardStatus {
        profile_id: String,
        status: Option<WireGuardStatus>,
        error: Option<String>,
    },
}

enum CoreCommand {
    WireGuardStatus(String),
    Revalidate,
}

struct Bridge {
    updates: Arc<Mutex<Vec<CoreUpdate>>>,
    commands: UnboundedSender<CoreCommand>,
    _thread: std::thread::JoinHandle<()>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

/// Initialize the core once and start its event bridge thread.
pub fn start() {
    if BRIDGE.get().is_some() {
        return;
    }
    local_proxy::init_app();

    let updates = Arc::new(Mutex::new(Vec::new()));
    let thread_updates = updates.clone();
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = mpsc::channel::<()>();

    let thread = std::thread::Builder::new()
        .name("localproxy-core-bridge".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create LocalProxy bridge runtime");
            runtime.block_on(async move {
                let mut events = local_proxy::subscribe_events();
                let _ = ready_tx.send(());
                pump(thread_updates, &mut events, command_rx).await;
            });
        })
        .expect("failed to spawn LocalProxy bridge thread");

    // Wait until the event subscription exists so no event can be missed by
    // the initial refresh.
    let _ = ready_rx.recv_timeout(std::time::Duration::from_secs(5));

    let _ = BRIDGE.set(Bridge {
        updates,
        commands: command_tx,
        _thread: thread,
    });
}

async fn pump(
    updates: Arc<Mutex<Vec<CoreUpdate>>>,
    events: &mut tokio::sync::broadcast::Receiver<ProxyEvent>,
    mut commands: UnboundedReceiver<CoreCommand>,
) {
    let mut last_revalidate: Option<std::time::Instant> = None;
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(event) => push(&updates, CoreUpdate::Event(event)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            command = commands.recv() => {
                match command {
                    Some(CoreCommand::WireGuardStatus(profile_id)) => {
                        let result = local_proxy::wireguard_status(profile_id.clone()).await;
                        let update = match result {
                            Ok(status) => CoreUpdate::WireGuardStatus {
                                profile_id,
                                status,
                                error: None,
                            },
                            Err(error) => CoreUpdate::WireGuardStatus {
                                profile_id,
                                status: None,
                                error: Some(error),
                            },
                        };
                        push(&updates, update);
                    }
                    Some(CoreCommand::Revalidate) => {
                        if last_revalidate.map(|last| last.elapsed() >= std::time::Duration::from_secs(5)).unwrap_or(true) {
                            last_revalidate = Some(std::time::Instant::now());
                            let _ = local_proxy::wireguard_revalidate(None).await;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

fn push(updates: &Arc<Mutex<Vec<CoreUpdate>>>, update: CoreUpdate) {
    if let Ok(mut queue) = updates.lock() {
        queue.push(update);
    }
}

/// Take everything that happened since the last poll.
pub fn drain() -> Vec<CoreUpdate> {
    match BRIDGE.get() {
        Some(bridge) => bridge
            .updates
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Ask the bridge thread for a WireGuard status snapshot.
pub fn request_wireguard_status(profile_id: &str) {
    if let Some(bridge) = BRIDGE.get() {
        let _ = bridge
            .commands
            .send(CoreCommand::WireGuardStatus(profile_id.to_string()));
    }
}

/// Ask the bridge to revalidate every running WireGuard tunnel (wake/unlock).
pub fn request_wireguard_revalidate() {
    if let Some(bridge) = BRIDGE.get() {
        let _ = bridge.commands.send(CoreCommand::Revalidate);
    }
}
