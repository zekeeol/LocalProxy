use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http_body::Frame;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderName, HeaderValue, CONNECTION, COOKIE, HOST};
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri, Version};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_service::Service;
use url::Url;

use super::{
    emit_log, http_endpoint, set_running, AppController, StoredAppSettings, StoredHttpProfile,
    StoredProxyEndpoint,
};

type BoxBody = http_body_util::combinators::BoxBody<Bytes, io::Error>;
type UpstreamClient = Client<hyper_rustls::HttpsConnector<SystemProxyConnector>, BoxBody>;

const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
struct SystemProxyConnector {
    proxy: Option<StoredProxyEndpoint>,
}

#[derive(Debug)]
struct ProxyStream {
    inner: TokioIo<tokio::net::TcpStream>,
    proxied: bool,
}

impl Connection for ProxyStream {
    fn connected(&self) -> Connected {
        Connected::new().proxy(self.proxied)
    }
}

impl Read for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl Write for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl Service<Uri> for SystemProxyConnector {
    type Response = ProxyStream;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, destination: Uri) -> Self::Future {
        let proxy = self.proxy.clone();
        Box::pin(async move { connect_transport(destination, proxy).await })
    }
}

async fn connect_transport(
    destination: Uri,
    proxy: Option<StoredProxyEndpoint>,
) -> Result<ProxyStream, io::Error> {
    let scheme = destination
        .scheme()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "上游 URI 缺少 scheme"))?;
    let host = destination
        .host()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "上游 URI 缺少主机"))?;
    let is_https = scheme == &http::uri::Scheme::HTTPS;
    let is_http = scheme == &http::uri::Scheme::HTTP;
    if !is_http && !is_https {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("不支持的上游 URI scheme: {scheme}"),
        ));
    }
    let target_port = destination
        .port_u16()
        .unwrap_or(if is_https { 443 } else { 80 });

    let (connect_host, connect_port) = match proxy.as_ref() {
        Some(proxy) => (proxy.host.as_str(), proxy.port),
        None => (host, target_port),
    };
    let mut stream = tokio::net::TcpStream::connect((connect_host, connect_port)).await?;
    let proxied = proxy.is_some() && is_http;
    if proxy.is_some() && is_https {
        establish_http_connect(&mut stream, host, target_port).await?;
    }
    Ok(ProxyStream {
        inner: TokioIo::new(stream),
        proxied,
    })
}

async fn establish_http_connect(
    stream: &mut tokio::net::TcpStream,
    host: &str,
    port: u16,
) -> Result<(), io::Error> {
    let authority = format_authority(host, port);
    let request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP 代理在 CONNECT 响应前关闭了连接",
            ));
        }
        response.extend_from_slice(&buffer[..read]);
        if response.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if response.len() > 8 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP 代理响应头过长",
            ));
        }
    }
    let response = String::from_utf8_lossy(&response);
    let status_line = response.lines().next().unwrap_or_default();
    let successful =
        status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200");
    if successful {
        return Ok(());
    }
    if status_line.starts_with("HTTP/1.1 407") || status_line.starts_with("HTTP/1.0 407") {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "HTTP 代理要求认证，LocalProxy 暂不支持代理认证",
        ));
    }
    Err(io::Error::other("HTTP 代理无法建立 HTTPS CONNECT 隧道"))
}

fn format_authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

struct HttpRuntimeConfig {
    base_url: Url,
    headers: Vec<(HeaderName, HeaderValue)>,
    cookies: Vec<(String, String)>,
}

pub(crate) async fn run(
    weak: Weak<AppController>,
    id: String,
    config: StoredHttpProfile,
    settings: StoredAppSettings,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), String> {
    let controller = super::upgrade(&weak)?;
    let runtime_config = Arc::new(load_runtime_config(&controller, &id, &config)?);
    let connector = SystemProxyConnector {
        proxy: if config.use_system_proxy {
            settings.http_proxy
        } else {
            None
        },
    };
    let connector = HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(|error| format!("加载系统 TLS 根证书失败: {error}"))?
        .https_or_http()
        .enable_http1()
        .wrap_connector(connector);
    let client: UpstreamClient = Client::builder(TokioExecutor::new()).build(connector);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", config.listen_port))
        .await
        .map_err(|error| format!("HTTP 监听失败: {error}"))?;
    if matches!(
        stop_rx.try_recv(),
        Ok(()) | Err(tokio::sync::oneshot::error::TryRecvError::Closed)
    ) {
        return Ok(());
    }
    let local_port = listener
        .local_addr()
        .map_err(|error| format!("读取 HTTP 监听端口失败: {error}"))?
        .port();
    let endpoint = http_endpoint(local_port);
    set_running(
        &controller,
        &id,
        vec![endpoint],
        Some("HTTP 反向代理已启动".to_string()),
    );
    emit_log(
        &controller,
        &id,
        format!("HTTP 监听于 127.0.0.1:{local_port}"),
    );

    loop {
        tokio::select! {
            _ = &mut stop_rx => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|error| format!("接受 HTTP 连接失败: {error}"))?;
                let io = TokioIo::new(stream);
                let request_config = runtime_config.clone();
                let request_client = client.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        proxy_request(request, request_client.clone(), request_config.clone())
                    });
                    if let Err(error) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await
                    {
                        // A client closing its socket is a normal occurrence and should not
                        // turn into a persistent service error.
                        let _ = error;
                    }
                });
            }
        }
    }
}

fn load_runtime_config(
    controller: &AppController,
    profile_id: &str,
    config: &StoredHttpProfile,
) -> Result<HttpRuntimeConfig, String> {
    let base_url =
        Url::parse(&config.upstream_url).map_err(|error| format!("上游 URL 无效: {error}"))?;
    if base_url.fragment().is_some() {
        return Err("上游 URL 不能包含 fragment".to_string());
    }
    let mut headers = Vec::with_capacity(config.headers.len());
    for field in &config.headers {
        let name = HeaderName::from_bytes(field.name.as_bytes())
            .map_err(|_| format!("header 名称无效: {}", field.name))?;
        if is_forbidden_configured_header(&name) {
            return Err(format!(
                "header 不能覆盖受保护的 hop-by-hop/Host 字段: {}",
                field.name
            ));
        }
        let value = controller
            .secrets
            .get(profile_id, "http-header", &field.name)?;
        let value =
            HeaderValue::from_str(&value).map_err(|_| format!("header 值无效: {}", field.name))?;
        headers.push((name, value));
    }
    let mut cookies = Vec::with_capacity(config.cookies.len());
    for field in &config.cookies {
        let value = controller
            .secrets
            .get(profile_id, "http-cookie", &field.name)?;
        HeaderValue::from_str(&format!("{}={value}", field.name))
            .map_err(|_| format!("cookie 值无效: {}", field.name))?;
        cookies.push((field.name.clone(), value));
    }
    Ok(HttpRuntimeConfig {
        base_url,
        headers,
        cookies,
    })
}

async fn proxy_request(
    request: Request<Incoming>,
    client: UpstreamClient,
    config: Arc<HttpRuntimeConfig>,
) -> Result<Response<BoxBody>, Infallible> {
    let upstream_url = match build_upstream_url(&config.base_url, request.uri()) {
        Ok(url) => url,
        Err(message) => return Ok(error_response(StatusCode::BAD_GATEWAY, message)),
    };
    let mut headers = copy_request_headers(request.headers());
    apply_configured_headers(&mut headers, &config.headers);
    apply_configured_cookies(&mut headers, &config.cookies);

    let method = match hyper::Method::from_bytes(request.method().as_str().as_bytes()) {
        Ok(method) => method,
        Err(_) => return Ok(error_response(StatusCode::BAD_REQUEST, "请求方法无效")),
    };
    let body_stream = request.into_body().into_data_stream().map(|result| {
        result
            .map(Frame::data)
            .map_err(|error| io::Error::other(error.to_string()))
    });
    let body = BodyExt::boxed(StreamBody::new(body_stream));
    let upstream_uri = match upstream_url.as_str().parse::<Uri>() {
        Ok(uri) => uri,
        Err(_) => return Ok(error_response(StatusCode::BAD_GATEWAY, "上游 URL 无效")),
    };
    let mut upstream_request = match Request::builder()
        .method(method)
        .uri(upstream_uri)
        .version(Version::HTTP_11)
        .body(body)
    {
        Ok(request) => request,
        Err(_) => return Ok(error_response(StatusCode::BAD_GATEWAY, "上游请求无效")),
    };
    *upstream_request.headers_mut() = headers;

    let response =
        match tokio::time::timeout(UPSTREAM_TIMEOUT, client.request(upstream_request)).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => return Ok(error_response(StatusCode::BAD_GATEWAY, "上游服务不可用")),
            Err(_) => {
                return Ok(error_response(
                    StatusCode::GATEWAY_TIMEOUT,
                    "上游服务响应超时",
                ))
            }
        };
    Ok(convert_response(response))
}

fn build_upstream_url(base: &Url, request_uri: &Uri) -> Result<Url, String> {
    let request_path = request_uri.path();
    let base_path = base.path().trim_end_matches('/');
    let path = if request_path == "/" {
        if base_path.is_empty() {
            "/".to_string()
        } else {
            base_path.to_string()
        }
    } else if base_path.is_empty() {
        request_path.to_string()
    } else {
        format!("{base_path}{request_path}")
    };
    let mut url = base.clone();
    url.set_path(&path);
    url.set_query(request_uri.query());
    Ok(url)
}

fn copy_request_headers(input: &HeaderMap) -> HeaderMap {
    let mut output = HeaderMap::new();
    let connection_tokens = connection_tokens(input);
    for (name, value) in input {
        if is_hop_by_hop(name)
            || connection_tokens
                .iter()
                .any(|token| token.eq_ignore_ascii_case(name.as_str()))
            || *name == HOST
        {
            continue;
        }
        if *name == hyper::header::TRANSFER_ENCODING {
            continue;
        }
        output.append(name.clone(), value.clone());
    }
    output
}

fn apply_configured_headers(headers: &mut HeaderMap, configured: &[(HeaderName, HeaderValue)]) {
    for (name, value) in configured {
        headers.insert(name.clone(), value.clone());
    }
}

fn apply_configured_cookies(headers: &mut HeaderMap, configured: &[(String, String)]) {
    if configured.is_empty() && !headers.contains_key(COOKIE) {
        return;
    }
    let mut cookies: Vec<(String, String)> = Vec::new();
    for value in headers.get_all(COOKIE).iter() {
        if let Ok(value) = value.to_str() {
            for item in value.split(';') {
                let Some((name, cookie_value)) = item.trim().split_once('=') else {
                    continue;
                };
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                if let Some(existing) = cookies.iter_mut().find(|(key, _)| key == name) {
                    existing.1 = cookie_value.trim().to_string();
                } else {
                    cookies.push((name.to_string(), cookie_value.trim().to_string()));
                }
            }
        }
    }
    for (name, value) in configured {
        if let Some(existing) = cookies.iter_mut().find(|(key, _)| key == name) {
            existing.1 = value.clone();
        } else {
            cookies.push((name.clone(), value.clone()));
        }
    }
    headers.remove(COOKIE);
    if let Ok(value) = HeaderValue::from_str(
        &cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; "),
    ) {
        headers.insert(COOKIE, value);
    }
}

fn convert_response(response: Response<Incoming>) -> Response<BoxBody> {
    let (parts, body) = response.into_parts();
    let mut builder = Response::builder().status(parts.status);
    for (name, value) in &parts.headers {
        if is_hop_by_hop(name)
            || *name == hyper::header::CONTENT_LENGTH
            || *name == hyper::header::TRANSFER_ENCODING
        {
            continue;
        }
        builder = builder.header(name, value);
    }
    let body = body.into_data_stream().map(|result| {
        result
            .map(Frame::data)
            .map_err(|error| io::Error::other(error.to_string()))
    });
    let body = BodyExt::boxed(StreamBody::new(body));
    builder
        .body(body)
        .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "上游响应无效"))
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response<BoxBody> {
    let body = Full::new(Bytes::from(message.into()))
        .map_err(|never| match never {})
        .map_err(|never: Infallible| io::Error::other(never))
        .boxed();
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(body)
        .unwrap_or_else(|_| {
            Response::new(
                Full::new(Bytes::from("proxy error"))
                    .map_err(|never| match never {})
                    .boxed(),
            )
        })
}

fn connection_tokens(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn is_forbidden_configured_header(name: &HeaderName) -> bool {
    is_hop_by_hop(name)
        || *name == HOST
        || *name == hyper::header::CONTENT_LENGTH
        || *name == hyper::header::TRANSFER_ENCODING
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};

    use http_body_util::{BodyExt, Full};
    use hyper::body::Incoming;
    use hyper::header::{HeaderMap, HeaderName, HeaderValue};
    use hyper::service::service_fn;
    use hyper::{Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::{broadcast, oneshot};

    use super::*;
    use crate::core::{
        status_for, ActiveService, AppController, PendingHostKey, RuntimeSnapshot, SecretStore,
        StoreFile,
    };

    fn test_controller() -> Arc<AppController> {
        let (events, _) = broadcast::channel(16);
        Arc::new(AppController {
            data_dir: std::env::temp_dir()
                .join(format!("local-proxy-test-{}", uuid::Uuid::new_v4())),
            store: Mutex::new(StoreFile::default()),
            secrets: SecretStore,
            active: Mutex::new(HashMap::<String, ActiveService>::new()),
            statuses: Mutex::new(HashMap::<String, RuntimeSnapshot>::new()),
            pending_host_keys: Mutex::new(HashMap::<String, PendingHostKey>::new()),
            events,
        })
    }

    #[test]
    fn appends_request_path_and_query_to_base_path() {
        let base = Url::parse("https://api.example.com/v1").unwrap();
        let uri: Uri = "/users?active=true".parse().unwrap();
        assert_eq!(
            build_upstream_url(&base, &uri).unwrap().as_str(),
            "https://api.example.com/v1/users?active=true"
        );

        let root_uri: Uri = "/?active=true".parse().unwrap();
        assert_eq!(
            build_upstream_url(&base, &root_uri).unwrap().as_str(),
            "https://api.example.com/v1?active=true"
        );
    }

    #[test]
    fn filters_hop_by_hop_and_connection_token_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("x-internal"));
        headers.insert(HOST, HeaderValue::from_static("local.test"));
        headers.insert(
            HeaderName::from_static("x-internal"),
            HeaderValue::from_static("remove-me"),
        );
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("test-header-value"),
        );
        let copied = copy_request_headers(&headers);
        assert!(!copied.contains_key(CONNECTION));
        assert!(!copied.contains_key(HOST));
        assert!(!copied.contains_key("x-internal"));
        assert_eq!(copied["authorization"], "test-header-value");
    }

    #[test]
    fn configured_cookies_override_client_cookies() {
        let mut headers = HeaderMap::new();
        headers.append(
            COOKIE,
            HeaderValue::from_static("session=client; theme=dark"),
        );
        headers.append(COOKIE, HeaderValue::from_static("flag=1"));
        apply_configured_cookies(
            &mut headers,
            &[
                ("session".to_string(), "server".to_string()),
                ("token".to_string(), "secret".to_string()),
            ],
        );
        assert_eq!(
            headers[COOKIE],
            "session=server; theme=dark; flag=1; token=secret"
        );
    }

    #[test]
    fn rejects_protected_configured_headers() {
        assert!(is_forbidden_configured_header(&HeaderName::from_static(
            "host"
        )));
        assert!(is_forbidden_configured_header(&HeaderName::from_static(
            "connection"
        )));
        assert!(!is_forbidden_configured_header(&HeaderName::from_static(
            "authorization"
        )));
    }

    #[tokio::test]
    async fn sends_http_upstream_requests_in_proxy_absolute_form() {
        let proxy_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        let proxy_task = tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            let service = service_fn(|request: Request<Incoming>| async move {
                assert_eq!(
                    request.uri().to_string(),
                    "http://example.test/resource?q=1"
                );
                let mut response = Response::new(Full::new(Bytes::from_static(b"proxied")));
                response
                    .headers_mut()
                    .insert(CONNECTION, HeaderValue::from_static("close"));
                Ok::<_, Infallible>(response)
            });
            hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });

        let connector = SystemProxyConnector {
            proxy: Some(StoredProxyEndpoint {
                host: "127.0.0.1".to_string(),
                port: proxy_port,
            }),
        };
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .unwrap()
            .https_or_http()
            .enable_http1()
            .wrap_connector(connector);
        let client: UpstreamClient = Client::builder(TokioExecutor::new()).build(connector);
        let body = Full::new(Bytes::new())
            .map_err(|never: Infallible| io::Error::other(never))
            .boxed();
        let request = Request::builder()
            .method("GET")
            .uri("http://example.test/resource?q=1")
            .body(body)
            .unwrap();
        let response = client.request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "proxied"
        );
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn establishes_connect_tunnel_for_https_upstream_requests() {
        let proxy_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        let proxy_task = tokio::spawn(async move {
            let (mut stream, _) = proxy_listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 256];
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("CONNECT example.test:8443 HTTP/1.1"));
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
        });

        let stream = connect_transport(
            "https://example.test:8443/".parse().unwrap(),
            Some(StoredProxyEndpoint {
                host: "127.0.0.1".to_string(),
                port: proxy_port,
            }),
        )
        .await
        .unwrap();
        drop(stream);
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn reverse_proxy_streams_binary_post_and_response() {
        let upstream_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream_port = upstream_listener.local_addr().unwrap().port();
        let upstream_task = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let service = service_fn(|mut request: Request<Incoming>| async move {
                let body = request.body_mut().collect().await.unwrap().to_bytes();
                assert_eq!(request.method(), hyper::Method::POST);
                assert_eq!(
                    request.uri().path_and_query().unwrap().as_str(),
                    "/base/upload?part=2"
                );
                assert_eq!(body.as_ref(), &[0, 1, 255, 7]);
                assert_eq!(request.headers()["authorization"], "test-header-value");
                let mut response =
                    Response::new(Full::new(bytes::Bytes::from_static(&[9, 0, 255])));
                *response.status_mut() = StatusCode::CREATED;
                response.headers_mut().insert(
                    HeaderName::from_static("x-upstream"),
                    HeaderValue::from_static("ok"),
                );
                response
                    .headers_mut()
                    .insert(CONNECTION, HeaderValue::from_static("close"));
                Ok::<_, Infallible>(response)
            });
            hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });

        let controller = test_controller();
        let profile_id = "http-stream-test".to_string();
        let config = StoredHttpProfile {
            listen_port: 0,
            upstream_url: format!("http://127.0.0.1:{upstream_port}/base"),
            headers: Vec::new(),
            cookies: Vec::new(),
            use_system_proxy: false,
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let proxy_task = tokio::spawn(run(
            Arc::downgrade(&controller),
            profile_id.clone(),
            config,
            StoredAppSettings::default(),
            stop_rx,
        ));
        let local_port = wait_for_endpoint(&controller, &profile_id).await;

        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", local_port))
            .await
            .unwrap();
        client
            .write_all(
                b"POST /upload?part=2 HTTP/1.1\r\nHost: local.test\r\nAuthorization: test-header-value\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n\x00\x01\xff\x07\r\n0\r\n\r\n",
            )
            .await
            .unwrap();
        let mut raw_response = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            client.read_to_end(&mut raw_response),
        )
        .await
        .unwrap()
        .unwrap();
        let response_text = String::from_utf8_lossy(&raw_response);
        assert!(response_text.starts_with("HTTP/1.1 201"));
        assert!(response_text.contains("x-upstream: ok"));
        assert!(raw_response.windows(3).any(|window| window == [9, 0, 255]));

        let _ = stop_tx.send(());
        assert!(proxy_task.await.unwrap().is_ok());
        assert!(upstream_task.await.is_ok());
        assert!(TcpListener::bind(("127.0.0.1", local_port)).await.is_ok());
    }

    async fn wait_for_endpoint(controller: &Arc<AppController>, id: &str) -> u16 {
        for _ in 0..100 {
            if let Some(endpoint) = status_for(controller, id).endpoints.first() {
                if endpoint.port != 0 {
                    return endpoint.port;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("HTTP proxy did not publish an endpoint");
    }
}
