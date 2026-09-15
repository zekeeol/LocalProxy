//! LocalProxy's private Electron bridge.
//!
//! The desktop UI starts this process and exchanges one JSON object per line
//! over stdin/stdout. It is intentionally a child-process protocol rather
//! than a network control API, so proxy configuration remains local to the
//! Electron application.

use local_proxy_rust::api::local_proxy::{AppSettingsDraft, ProfileDraft};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
struct IpcRequest {
    id: String,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct IpcResponse {
    id: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct IpcNotification {
    event: &'static str,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct ConfirmHostKeyParams {
    id: String,
    fingerprint: String,
    accepted: bool,
}

type Output = Arc<Mutex<BufWriter<tokio::io::Stdout>>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    local_proxy_rust::api::local_proxy::init_app();

    let output: Output = Arc::new(Mutex::new(BufWriter::new(tokio::io::stdout())));
    spawn_event_forwarder(output.clone());

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: IpcRequest = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(error) => {
                // stdout is reserved for protocol messages. Keep malformed
                // input diagnostics on stderr so the renderer cannot mistake
                // them for a response.
                eprintln!("LocalProxy IPC request ignored: {error}");
                continue;
            }
        };
        let should_exit = request.method == "shutdown";
        let response = handle_request(request).await;
        write_json(&output, &response).await;
        if should_exit {
            break;
        }
    }

    Ok(())
}

fn spawn_event_forwarder(output: Output) {
    let mut events = local_proxy_rust::api::local_proxy::subscribe_events();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let notification = IpcNotification {
                        event: "proxy_event",
                        payload: serde_json::to_value(event).unwrap_or_else(|_| json!({})),
                    };
                    write_json(&output, &notification).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn write_json<T: Serialize>(output: &Output, value: &T) {
    let Ok(serialized) = serde_json::to_string(value) else {
        return;
    };
    let mut output = output.lock().await;
    let _ = output.write_all(serialized.as_bytes()).await;
    let _ = output.write_all(b"\n").await;
    let _ = output.flush().await;
}

async fn handle_request(request: IpcRequest) -> IpcResponse {
    let id = request.id;
    let result = match request.method.as_str() {
        "parse_wstunnel_command" => with_string(
            &request.params,
            "command",
            local_proxy_rust::api::local_proxy::parse_wstunnel_command,
        ),
        "parse_wg_config" => with_string(
            &request.params,
            "config",
            local_proxy_rust::api::local_proxy::parse_wg_config,
        ),
        "wireguard_status" => match request.params.get("id").and_then(Value::as_str) {
            Some(id) if !id.trim().is_empty() => serialize_result(
                local_proxy_rust::api::local_proxy::wireguard_status(id.to_owned()).await,
            ),
            _ => Err("缺少参数: id".to_string()),
        },
        "ping" => Ok(json!({ "pong": true })),
        "list_profiles" => serialize_result(local_proxy_rust::api::local_proxy::list_profiles()),
        "get_profile" => with_string(
            &request.params,
            "id",
            local_proxy_rust::api::local_proxy::get_profile,
        ),
        "get_settings" => serialize_result(local_proxy_rust::api::local_proxy::get_settings()),
        "save_settings" => with_params::<AppSettingsDraft, _, _>(
            &request.params,
            local_proxy_rust::api::local_proxy::save_settings,
        ),
        "save_profile" => with_params::<ProfileDraft, _, _>(
            &request.params,
            local_proxy_rust::api::local_proxy::save_profile,
        ),
        "delete_profile" => with_string(
            &request.params,
            "id",
            local_proxy_rust::api::local_proxy::delete_profile,
        ),
        "start_profile" => with_string(
            &request.params,
            "id",
            local_proxy_rust::api::local_proxy::start_profile,
        ),
        "stop_profile" => with_string(
            &request.params,
            "id",
            local_proxy_rust::api::local_proxy::stop_profile,
        ),
        "confirm_host_key" => {
            with_params::<ConfirmHostKeyParams, _, _>(&request.params, |params| {
                local_proxy_rust::api::local_proxy::confirm_host_key(
                    params.id,
                    params.fingerprint,
                    params.accepted,
                )
            })
        }
        "shutdown" => {
            if let Ok(profiles) = local_proxy_rust::api::local_proxy::list_profiles() {
                for profile in profiles {
                    let _ = local_proxy_rust::api::local_proxy::stop_profile(profile.id);
                }
            }
            Ok(json!({ "stopped": true }))
        }
        method => Err(format!("未知 RPC 指令: {method}")),
    };

    match result {
        Ok(result) => IpcResponse {
            id,
            success: true,
            result: Some(result),
            error: None,
        },
        Err(error) => IpcResponse {
            id,
            success: false,
            result: None,
            error: Some(error),
        },
    }
}

fn serialize_result<T: Serialize>(result: Result<T, String>) -> Result<Value, String> {
    result.map(|value| serde_json::to_value(value).unwrap_or_else(|_| json!({})))
}

fn with_string<T>(
    params: &Value,
    name: &str,
    action: impl FnOnce(String) -> Result<T, String>,
) -> Result<Value, String>
where
    T: Serialize,
{
    let value = params
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("缺少参数: {name}"))?;
    serialize_result(action(value.to_string()))
}

fn with_params<P, R, F>(params: &Value, action: F) -> Result<Value, String>
where
    P: for<'de> Deserialize<'de>,
    R: Serialize,
    F: FnOnce(P) -> Result<R, String>,
{
    let typed = serde_json::from_value::<P>(params.clone())
        .map_err(|error| format!("参数解析失败: {error}"))?;
    serialize_result(action(typed))
}
