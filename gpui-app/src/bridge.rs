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

use std::future::Future;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use local_proxy_rust::api::local_proxy::{self, ProxyEvent, WireGuardStatus};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Updates produced off the foreground thread.
#[derive(Debug)]
pub enum CoreUpdate {
    Event(ProxyEvent),
    WireGuardStatus {
        profile_id: String,
        request_id: u64,
        status: Option<WireGuardStatus>,
        error: Option<String>,
    },
}

enum CoreCommand {
    WireGuardStatus { profile_id: String, request_id: u64 },
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
                pump(
                    thread_updates,
                    &mut events,
                    command_rx,
                    local_proxy::wireguard_status,
                    || local_proxy::wireguard_revalidate(None),
                )
                .await;
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

async fn pump<S, SF, R, RF>(
    updates: Arc<Mutex<Vec<CoreUpdate>>>,
    events: &mut tokio::sync::broadcast::Receiver<ProxyEvent>,
    mut commands: UnboundedReceiver<CoreCommand>,
    status: S,
    revalidate: R,
) where
    S: Fn(String) -> SF,
    SF: Future<Output = Result<Option<WireGuardStatus>, String>> + Send + 'static,
    R: Fn() -> RF,
    RF: Future<Output = Result<usize, String>> + Send + 'static,
{
    let mut last_revalidate: Option<std::time::Instant> = None;
    let mut revalidation: Option<tokio::task::JoinHandle<()>> = None;
    let mut revalidate_again = false;
    loop {
        tokio::select! {
            _ = async {
                if let Some(task) = &mut revalidation {
                    let _ = task.await;
                }
            }, if revalidation.is_some() => {
                revalidation = None;
                if revalidate_again
                    && last_revalidate.map(|last| last.elapsed() >= std::time::Duration::from_secs(5)).unwrap_or(true)
                {
                    last_revalidate = Some(std::time::Instant::now());
                    let revalidate = revalidate();
                    revalidation = Some(tokio::spawn(async move {
                        let _ = revalidate.await;
                    }));
                }
                revalidate_again = false;
            }
            event = events.recv() => {
                match event {
                    Ok(event) => push(&updates, CoreUpdate::Event(event)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            command = commands.recv() => {
                match command {
                    Some(CoreCommand::WireGuardStatus { profile_id, request_id }) => {
                        let status = status(profile_id.clone());
                        let updates = updates.clone();
                        tokio::spawn(async move {
                            let (status, error) = match status.await {
                                Ok(status) => (status, None),
                                Err(error) => (None, Some(error)),
                            };
                            push(&updates, CoreUpdate::WireGuardStatus {
                                profile_id,
                                request_id,
                                status,
                                error,
                            });
                        });
                    }
                    Some(CoreCommand::Revalidate) => {
                        if revalidation.is_some() {
                            revalidate_again = true;
                        } else if last_revalidate.map(|last| last.elapsed() >= std::time::Duration::from_secs(5)).unwrap_or(true) {
                            last_revalidate = Some(std::time::Instant::now());
                            let revalidate = revalidate();
                            revalidation = Some(tokio::spawn(async move {
                                let _ = revalidate.await;
                            }));
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
pub fn request_wireguard_status(profile_id: &str, request_id: u64) {
    if let Some(bridge) = BRIDGE.get() {
        let _ = bridge.commands.send(CoreCommand::WireGuardStatus {
            profile_id: profile_id.to_string(),
            request_id,
        });
    }
}

/// Ask the bridge to revalidate every running WireGuard tunnel (wake/unlock).
pub fn request_wireguard_revalidate() {
    if let Some(bridge) = BRIDGE.get() {
        let _ = bridge.commands.send(CoreCommand::Revalidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use local_proxy_rust::api::local_proxy::{EventKind, ServiceState};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::{broadcast, Notify};

    #[test]
    fn status_and_events_are_not_blocked_by_revalidation() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let updates = Arc::new(Mutex::new(Vec::new()));
            let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
            let (event_tx, mut event_rx) = broadcast::channel(8);
            let started = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            let calls = Arc::new(AtomicUsize::new(0));

            let worker = tokio::spawn({
                let updates = updates.clone();
                let started = started.clone();
                let release = release.clone();
                let calls = calls.clone();
                async move {
                    pump(
                        updates,
                        &mut event_rx,
                        command_rx,
                        |_| async { Ok(None) },
                        move || {
                            let started = started.clone();
                            let release = release.clone();
                            let calls = calls.clone();
                            async move {
                                calls.fetch_add(1, Ordering::SeqCst);
                                started.notify_one();
                                release.notified().await;
                                Ok(0)
                            }
                        },
                    )
                    .await;
                }
            });

            command_tx.send(CoreCommand::Revalidate).unwrap();
            tokio::time::timeout(Duration::from_secs(1), started.notified())
                .await
                .unwrap();
            command_tx.send(CoreCommand::Revalidate).unwrap();
            command_tx
                .send(CoreCommand::WireGuardStatus {
                    profile_id: "wg".into(),
                    request_id: 42,
                })
                .unwrap();
            event_tx
                .send(ProxyEvent {
                    profile_id: "wg".into(),
                    kind: EventKind::Log,
                    state: ServiceState::Running,
                    message: None,
                    fingerprint: None,
                    endpoints: Vec::new(),
                })
                .unwrap();

            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    let ready = {
                        let queue = updates.lock().unwrap();
                        queue
                            .iter()
                            .any(|update| matches!(update, CoreUpdate::Event(_)))
                            && queue.iter().any(|update| {
                                matches!(update, CoreUpdate::WireGuardStatus {
                                    profile_id,
                                    request_id: 42,
                                    ..
                                } if profile_id == "wg")
                            })
                    };
                    if ready {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("status and events should not wait for revalidation");
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            release.notify_one();
            drop(command_tx);
            worker.await.unwrap();
        });
    }
}
