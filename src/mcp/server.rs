use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::cdp::client::CdpClient;

/// Timeout for HTTP requests (e.g., debug endpoint discovery)
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Extract a human-readable error message from CDP exception details
fn extract_cdp_error(exception: &Value) -> String {
    // Try to get the most useful error information
    // CDP exception details can have: text, exception.description, exception.value, lineNumber, columnNumber

    let mut parts = Vec::new();

    // Main error text
    if let Some(text) = exception.get("text").and_then(|t| t.as_str()) {
        parts.push(text.to_string());
    }

    // Exception object details
    if let Some(exc) = exception.get("exception") {
        if let Some(desc) = exc.get("description").and_then(|d| d.as_str()) {
            parts.push(desc.to_string());
        } else if let Some(value) = exc.get("value").and_then(|v| v.as_str()) {
            parts.push(value.to_string());
        }
    }

    // Line/column info for syntax errors
    if let (Some(line), Some(col)) = (
        exception.get("lineNumber").and_then(|l| l.as_i64()),
        exception.get("columnNumber").and_then(|c| c.as_i64()),
    ) {
        parts.push(format!("at line {}, column {}", line, col));
    }

    if parts.is_empty() {
        "Unknown error".to_string()
    } else {
        parts.join(" - ")
    }
}

/// Map key name to (key, code, text) tuple for CDP Input.dispatchKeyEvent
fn map_key(key: &str) -> (String, String, String) {
    match key {
        "Enter" => ("Enter".into(), "Enter".into(), "\r".into()),
        "Tab" => ("Tab".into(), "Tab".into(), "\t".into()),
        "Escape" => ("Escape".into(), "Escape".into(), "".into()),
        "Backspace" => ("Backspace".into(), "Backspace".into(), "".into()),
        "Delete" => ("Delete".into(), "Delete".into(), "".into()),
        "ArrowUp" => ("ArrowUp".into(), "ArrowUp".into(), "".into()),
        "ArrowDown" => ("ArrowDown".into(), "ArrowDown".into(), "".into()),
        "ArrowLeft" => ("ArrowLeft".into(), "ArrowLeft".into(), "".into()),
        "ArrowRight" => ("ArrowRight".into(), "ArrowRight".into(), "".into()),
        "Home" => ("Home".into(), "Home".into(), "".into()),
        "End" => ("End".into(), "End".into(), "".into()),
        "PageUp" => ("PageUp".into(), "PageUp".into(), "".into()),
        "PageDown" => ("PageDown".into(), "PageDown".into(), "".into()),
        "Space" | " " => (" ".into(), "Space".into(), " ".into()),
        c if c.len() == 1 => (c.into(), format!("Key{}", c.to_uppercase()), c.into()),
        other => (other.into(), other.into(), "".into()),
    }
}

/// Get virtual key code for a key name
fn get_key_code(key: &str) -> u32 {
    match key {
        "Enter" => 13,
        "Tab" => 9,
        "Escape" => 27,
        "Backspace" => 8,
        "Delete" => 46,
        "ArrowUp" => 38,
        "ArrowDown" => 40,
        "ArrowLeft" => 37,
        "ArrowRight" => 39,
        "Home" => 36,
        "End" => 35,
        "PageUp" => 33,
        "PageDown" => 34,
        "Space" | " " => 32,
        c if c.len() == 1 => {
            let c = c.chars().next().unwrap().to_ascii_uppercase();
            c as u32
        }
        _ => 0,
    }
}

use super::protocol::{
    error_codes, InitializeParams, InitializeResult, JsonRpcRequest, JsonRpcResponse,
    ServerCapabilities, ServerInfo, ToolCallParams, ToolCallResult, ToolsCapability,
    ToolsListResult,
};
use super::tools::{
    get_tool_definitions,
    // Utility tool params
    BlurParams,
    // Enhancement tool params
    CheckParams,
    ClearParams,
    ClickParams,
    ConnectParams,
    DispatchParams,
    // New tool params (Phase 1 & 2)
    DragParams,
    EvaluateOnSelectorParams,
    ExecuteJsParams,
    FillParams,
    FocusParams,
    GetAttributeParams,
    GetBoundingBoxParams,
    // Cookie and additional tool params
    GetCookiesParams,
    GetElementInfoParams,
    GetLogsParams,
    GetNetworkRequestsParams,
    GetStateParams,
    GetStorageParams,
    GetTextParams,
    HandleDialogParams,
    HoverParams,
    NavigateParams,
    NavigationParams,
    PressKeyParams,
    QueryAllDomParams,
    QueryDomParams,
    ReloadParams,
    RemoveCookieParams,
    RemoveStorageParams,
    ResizeViewportParams,
    ScreenshotParams,
    ScrollParams,
    SelectOptionParams,
    // Additional enhancement tool params
    SelectTextParams,
    SetAttributeParams,
    SetCookieParams,
    SetStateParams,
    SetStorageParams,
    TypeTextParams,
    VerifyComponentParams,
    WaitCondition,
    WaitForParams,
};

pub struct McpServer {
    cdp_client: Arc<Mutex<Option<CdpClient>>>,
    http_client: reqwest::Client,
    initialized: bool,
}

#[derive(Clone, Copy, Debug)]
enum TransportMode {
    ContentLength,
    LineDelimited,
}

impl McpServer {
    pub fn new() -> Self {
        // Create a shared HTTP client with sensible defaults
        let http_client = reqwest::Client::builder()
            .timeout(HTTP_REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(2)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            cdp_client: Arc::new(Mutex::new(None)),
            http_client,
            initialized: false,
        }
    }

    fn score_debug_target(page: &Value) -> i32 {
        let page_type = page.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if page_type != "page" {
            return i32::MIN;
        }

        let title = page.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let url = page.get("url").and_then(|u| u.as_str()).unwrap_or("");

        let mut score = 0;

        if !url.is_empty() {
            score += 100;
        }

        if !title.is_empty() {
            score += 10;
        }

        if title.starts_with("DevTools") || url.starts_with("devtools://") {
            score -= 10_000;
        }

        if url.contains("127.0.0.1:5173") || url.contains("localhost:5173") {
            score += 5_000;
        } else if url.starts_with("http://127.0.0.1:")
            || url.starts_with("https://127.0.0.1:")
            || url.starts_with("http://localhost:")
            || url.starts_with("https://localhost:")
        {
            score += 4_000;
        } else if url.starts_with("http://") || url.starts_with("https://") {
            score += 3_000;
        }

        if title == "Dataful" || title == "Testing" {
            score += 250;
        }

        score
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut stdin = BufReader::new(tokio::io::stdin());
        let mut stdout = tokio::io::stdout();

        while let Some((message, transport_mode)) = Self::read_message(&mut stdin).await? {
            debug!("Received: {}", message);

            let request: JsonRpcRequest = match serde_json::from_str(&message) {
                Ok(r) => r,
                Err(e) => {
                    let response = JsonRpcResponse::error(
                        None,
                        error_codes::PARSE_ERROR,
                        format!("Parse error: {}", e),
                    );
                    self.send_response(&mut stdout, &response, transport_mode)
                        .await?;
                    continue;
                }
            };

            let response = self.handle_request(request).await;

            if let Some(resp) = response {
                self.send_response(&mut stdout, &resp, transport_mode)
                    .await?;
            }
        }

        Ok(())
    }

    async fn read_message(
        stdin: &mut BufReader<tokio::io::Stdin>,
    ) -> Result<Option<(String, TransportMode)>> {
        loop {
            let mut first_line = String::new();
            let bytes_read = stdin.read_line(&mut first_line).await?;

            if bytes_read == 0 {
                return Ok(None);
            }

            if first_line.trim().is_empty() {
                continue;
            }

            let first_line = first_line.trim_end_matches(['\r', '\n']).to_string();

            if first_line.starts_with('{') {
                return Ok(Some((first_line, TransportMode::LineDelimited)));
            }

            let mut headers = vec![first_line];

            loop {
                let mut header_line = String::new();
                let bytes_read = stdin.read_line(&mut header_line).await?;

                if bytes_read == 0 {
                    return Ok(None);
                }

                let header_line = header_line.trim_end_matches(['\r', '\n']).to_string();

                if header_line.is_empty() {
                    break;
                }

                headers.push(header_line);
            }

            let content_length = headers.iter().find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .map(str::trim)
                    .and_then(|value| value.parse::<usize>().ok())
            });

            let Some(content_length) = content_length else {
                warn!(
                    "Ignoring MCP message with no Content-Length header: {:?}",
                    headers
                );
                continue;
            };

            let mut content = vec![0; content_length];
            stdin.read_exact(&mut content).await?;
            let message = String::from_utf8(content)?;
            return Ok(Some((message, TransportMode::ContentLength)));
        }
    }

    async fn send_response(
        &self,
        stdout: &mut tokio::io::Stdout,
        response: &JsonRpcResponse,
        transport_mode: TransportMode,
    ) -> Result<()> {
        let json = serde_json::to_string(response)?;
        debug!("Sending: {}", json);

        match transport_mode {
            TransportMode::ContentLength => {
                let header = format!("Content-Length: {}\r\n\r\n", json.len());
                stdout.write_all(header.as_bytes()).await?;
                stdout.write_all(json.as_bytes()).await?;
            }
            TransportMode::LineDelimited => {
                stdout.write_all(json.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
            }
        }

        stdout.flush().await?;
        Ok(())
    }

    async fn handle_request(&mut self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = request.id.clone();

        match request.method.as_str() {
            "initialize" => Some(self.handle_initialize(id, request.params).await),
            "initialized" => {
                // Notification, no response
                info!("Client initialized");
                None
            }
            "shutdown" => {
                info!("Shutdown requested, cleaning up...");
                let mut client = self.cdp_client.lock().await;
                if client.is_some() {
                    info!("Disconnecting CDP client");
                }
                *client = None;
                drop(client);
                info!("Shutdown complete");
                Some(JsonRpcResponse::success(id, json!(null)))
            }
            "tools/list" => Some(self.handle_tools_list(id).await),
            "tools/call" => Some(self.handle_tool_call(id, request.params).await),
            "ping" => Some(JsonRpcResponse::success(id, json!({}))),
            "notifications/initialized" => None,
            _ => {
                warn!("Unknown method: {}", request.method);
                Some(JsonRpcResponse::error(
                    id,
                    error_codes::METHOD_NOT_FOUND,
                    format!("Method not found: {}", request.method),
                ))
            }
        }
    }

    async fn handle_initialize(
        &mut self,
        id: Option<Value>,
        params: Option<Value>,
    ) -> JsonRpcResponse {
        let _params: InitializeParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(
                        id,
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {}", e),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(id, error_codes::INVALID_PARAMS, "Missing params");
            }
        };

        self.initialized = true;
        info!("MCP initialized");

        let result = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability {
                    list_changed: false,
                },
            },
            server_info: ServerInfo {
                name: "dataful-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
    }

    async fn handle_tools_list(&self, id: Option<Value>) -> JsonRpcResponse {
        let result = ToolsListResult {
            tools: get_tool_definitions(),
        };
        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
    }

    async fn handle_tool_call(&self, id: Option<Value>, params: Option<Value>) -> JsonRpcResponse {
        let params: ToolCallParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(
                        id,
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {}", e),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(id, error_codes::INVALID_PARAMS, "Missing params");
            }
        };

        let result = self.execute_tool(&params.name, params.arguments).await;

        match result {
            Ok(tool_result) => {
                JsonRpcResponse::success(id, serde_json::to_value(tool_result).unwrap())
            }
            Err(e) => JsonRpcResponse::success(
                id,
                serde_json::to_value(ToolCallResult::error(format!("Tool error: {}", e))).unwrap(),
            ),
        }
    }

    async fn execute_tool(&self, name: &str, args: Option<Value>) -> Result<ToolCallResult> {
        let args = args.unwrap_or(json!({}));

        match name {
            "connect" => {
                let params: ConnectParams = serde_json::from_value(args)?;
                self.tool_connect(params).await
            }
            "disconnect" => self.tool_disconnect().await,
            "ping" => self.tool_ping().await,
            "execute_js" => {
                let params: ExecuteJsParams = serde_json::from_value(args)?;
                self.tool_execute_js(params).await
            }
            "query_dom" => {
                let params: QueryDomParams = serde_json::from_value(args)?;
                self.tool_query_dom(params).await
            }
            "query_all_dom" => {
                let params: QueryAllDomParams = serde_json::from_value(args)?;
                self.tool_query_all_dom(params).await
            }
            "verify_component" => {
                let params: VerifyComponentParams = serde_json::from_value(args)?;
                self.tool_verify_component(params).await
            }
            "get_logs" => {
                let params: GetLogsParams = serde_json::from_value(args)?;
                self.tool_get_logs(params).await
            }
            "clear_logs" => self.tool_clear_logs().await,
            "get_state" => {
                let params: GetStateParams = serde_json::from_value(args)?;
                self.tool_get_state(params).await
            }
            "set_state" => {
                let params: SetStateParams = serde_json::from_value(args)?;
                self.tool_set_state(params).await
            }
            "dispatch" => {
                let params: DispatchParams = serde_json::from_value(args)?;
                self.tool_dispatch(params).await
            }
            "navigate" => {
                let params: NavigateParams = serde_json::from_value(args)?;
                self.tool_navigate(params).await
            }
            "screenshot" => {
                let params: ScreenshotParams = serde_json::from_value(args)?;
                self.tool_screenshot(params).await
            }
            "get_app_info" => self.tool_get_app_info().await,
            "click" => {
                let params: ClickParams = serde_json::from_value(args)?;
                self.tool_click(params).await
            }
            "type_text" => {
                let params: TypeTextParams = serde_json::from_value(args)?;
                self.tool_type_text(params).await
            }
            "wait_for" => {
                let params: WaitForParams = serde_json::from_value(args)?;
                self.tool_wait_for(params).await
            }
            // New tools (Phase 1 & 2)
            "hover" => {
                let params: HoverParams = serde_json::from_value(args)?;
                self.tool_hover(params).await
            }
            "press_key" => {
                let params: PressKeyParams = serde_json::from_value(args)?;
                self.tool_press_key(params).await
            }
            "select_option" => {
                let params: SelectOptionParams = serde_json::from_value(args)?;
                self.tool_select_option(params).await
            }
            "go_back" => {
                let params: NavigationParams = serde_json::from_value(args)?;
                self.tool_go_back(params).await
            }
            "go_forward" => {
                let params: NavigationParams = serde_json::from_value(args)?;
                self.tool_go_forward(params).await
            }
            "handle_dialog" => {
                let params: HandleDialogParams = serde_json::from_value(args)?;
                self.tool_handle_dialog(params).await
            }
            "get_last_dialog" => self.tool_get_last_dialog().await,
            "drag" => {
                let params: DragParams = serde_json::from_value(args)?;
                self.tool_drag(params).await
            }
            "get_network_requests" => {
                let params: GetNetworkRequestsParams = serde_json::from_value(args)?;
                self.tool_get_network_requests(params).await
            }
            "resize_viewport" => {
                let params: ResizeViewportParams = serde_json::from_value(args)?;
                self.tool_resize_viewport(params).await
            }
            // Enhancement tools
            "focus" => {
                let params: FocusParams = serde_json::from_value(args)?;
                self.tool_focus(params).await
            }
            "scroll" => {
                let params: ScrollParams = serde_json::from_value(args)?;
                self.tool_scroll(params).await
            }
            "fill" => {
                let params: FillParams = serde_json::from_value(args)?;
                self.tool_fill(params).await
            }
            "check" => {
                let params: CheckParams = serde_json::from_value(args)?;
                self.tool_check(params).await
            }
            "uncheck" => {
                let params: CheckParams = serde_json::from_value(args)?;
                self.tool_uncheck(params).await
            }
            "get_element_info" => {
                let params: GetElementInfoParams = serde_json::from_value(args)?;
                self.tool_get_element_info(params).await
            }
            // Utility tools
            "blur" => {
                let params: BlurParams = serde_json::from_value(args)?;
                self.tool_blur(params).await
            }
            "clear" => {
                let params: ClearParams = serde_json::from_value(args)?;
                self.tool_clear(params).await
            }
            "reload" => {
                let params: ReloadParams = serde_json::from_value(args)?;
                self.tool_reload(params).await
            }
            "get_url" => self.tool_get_url().await,
            "get_title" => self.tool_get_title().await,
            // Additional enhancement tools
            "select_text" => {
                let params: SelectTextParams = serde_json::from_value(args)?;
                self.tool_select_text(params).await
            }
            "get_attribute" => {
                let params: GetAttributeParams = serde_json::from_value(args)?;
                self.tool_get_attribute(params).await
            }
            "set_attribute" => {
                let params: SetAttributeParams = serde_json::from_value(args)?;
                self.tool_set_attribute(params).await
            }
            "get_local_storage" => {
                let params: GetStorageParams = serde_json::from_value(args)?;
                self.tool_get_local_storage(params).await
            }
            "set_local_storage" => {
                let params: SetStorageParams = serde_json::from_value(args)?;
                self.tool_set_local_storage(params).await
            }
            "remove_local_storage" => {
                let params: RemoveStorageParams = serde_json::from_value(args)?;
                self.tool_remove_local_storage(params).await
            }
            "get_session_storage" => {
                let params: GetStorageParams = serde_json::from_value(args)?;
                self.tool_get_session_storage(params).await
            }
            "set_session_storage" => {
                let params: SetStorageParams = serde_json::from_value(args)?;
                self.tool_set_session_storage(params).await
            }
            "remove_session_storage" => {
                let params: RemoveStorageParams = serde_json::from_value(args)?;
                self.tool_remove_session_storage(params).await
            }
            // Cookie and additional tools
            "get_cookies" => {
                let params: GetCookiesParams = serde_json::from_value(args)?;
                self.tool_get_cookies(params).await
            }
            "set_cookie" => {
                let params: SetCookieParams = serde_json::from_value(args)?;
                self.tool_set_cookie(params).await
            }
            "remove_cookie" => {
                let params: RemoveCookieParams = serde_json::from_value(args)?;
                self.tool_remove_cookie(params).await
            }
            "get_bounding_box" => {
                let params: GetBoundingBoxParams = serde_json::from_value(args)?;
                self.tool_get_bounding_box(params).await
            }
            "evaluate_on_selector" => {
                let params: EvaluateOnSelectorParams = serde_json::from_value(args)?;
                self.tool_evaluate_on_selector(params).await
            }
            "get_inner_text" => {
                let params: GetTextParams = serde_json::from_value(args)?;
                self.tool_get_inner_text(params).await
            }
            "get_inner_html" => {
                let params: GetTextParams = serde_json::from_value(args)?;
                self.tool_get_inner_html(params).await
            }
            _ => Ok(ToolCallResult::error(format!("Unknown tool: {}", name))),
        }
    }

    // ============ Tool Implementations ============

    async fn tool_connect(&self, params: ConnectParams) -> Result<ToolCallResult> {
        // Check if already connected
        {
            let existing = self.cdp_client.lock().await;
            if existing.is_some() {
                warn!("Already connected, disconnecting first");
                drop(existing);
                // Disconnect first
                let mut client = self.cdp_client.lock().await;
                *client = None;
            }
        }

        info!("Connecting to {}:{}", params.host, params.port);

        // First, query the debug endpoint to get the WebSocket URL
        let debug_url = format!("http://{}:{}/json/list", params.host, params.port);

        let pages: Vec<serde_json::Value> = self.http_client
            .get(&debug_url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to debug port: {}. Make sure Dataful is running with WEBKIT_INSPECTOR_SERVER=127.0.0.1:{}", e, params.port))?
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse debug response: {}", e))?;

        // Find a suitable page
        let page = pages
            .into_iter()
            .max_by_key(Self::score_debug_target)
            .filter(|page| Self::score_debug_target(page) != i32::MIN)
            .ok_or_else(|| anyhow!("No page found in debug endpoint"))?;

        let ws_url = page
            .get("webSocketDebuggerUrl")
            .and_then(|u| u.as_str())
            .ok_or_else(|| anyhow!("No WebSocket URL in page info"))?;

        info!(
            "Connecting to WebSocket: {} (title: {:?}, url: {:?})",
            ws_url,
            page.get("title").and_then(|t| t.as_str()).unwrap_or(""),
            page.get("url").and_then(|u| u.as_str()).unwrap_or("")
        );

        let client = CdpClient::connect(ws_url).await?;

        // Store the client - use a block to release the lock before calling ensure_devtools_bridge
        {
            let mut cdp_client = self.cdp_client.lock().await;
            *cdp_client = Some(client);
        } // Lock is released here
        // Initialize the devtools bridge if not already present
        self.ensure_devtools_bridge().await?;

        let title = page
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown");
        let url = page
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("Unknown");

        Ok(ToolCallResult::json(json!({
            "connected": true,
            "page_title": title,
            "page_url": url,
            "ws_url": ws_url
        })))
    }

    /// Helper to extract value from CDP result, returning error result if JS exception occurred
    fn extract_cdp_result(result: &Value) -> Result<Value, String> {
        if let Some(exception) = result.get("exceptionDetails") {
            return Err(format!(
                "JavaScript error: {}",
                extract_cdp_error(exception)
            ));
        }
        Ok(result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// Clamp timeout to reasonable bounds (100ms - 60000ms) to prevent runaway waits.
    /// Preserves 0 as a special value meaning "no wait".
    fn clamp_timeout(timeout_ms: u64) -> u64 {
        if timeout_ms == 0 {
            0
        } else {
            timeout_ms.clamp(100, 60000)
        }
    }

    async fn ensure_devtools_bridge(&self) -> Result<()> {
        // Check if bridge exists - minimize lock scope
        let needs_injection = {
            let client_guard = self.cdp_client.lock().await;
            let client = client_guard
                .as_ref()
                .ok_or_else(|| anyhow!("Not connected"))?;

            let check = client
                .evaluate("typeof window.__DATAFUL__ !== 'undefined'", false)
                .await?;

            check
                .get("result")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.as_bool())
                != Some(true)
        }; // Lock released here

        if needs_injection {
            // Re-acquire lock for injection
            let client_guard = self.cdp_client.lock().await;
            let client = client_guard
                .as_ref()
                .ok_or_else(|| anyhow!("Not connected"))?;

            info!("Injecting devtools bridge");
            let bridge_code = include_str!("../bridge_inject.js");
            client.evaluate(bridge_code, false).await?;
        }

        Ok(())
    }

    async fn tool_disconnect(&self) -> Result<ToolCallResult> {
        let mut client = self.cdp_client.lock().await;
        *client = None;
        drop(client);
        info!("Disconnected");
        Ok(ToolCallResult::json(json!({ "disconnected": true })))
    }

    async fn tool_ping(&self) -> Result<ToolCallResult> {
        let client_guard = self.cdp_client.lock().await;

        match client_guard.as_ref() {
            Some(client) => {
                // Actually verify the connection is alive by making a simple CDP call
                match client.evaluate("1", false).await {
                    Ok(_) => Ok(ToolCallResult::json(json!({
                        "connected": true,
                        "status": "healthy"
                    }))),
                    Err(e) => Ok(ToolCallResult::json(json!({
                        "connected": true,
                        "status": "unhealthy",
                        "error": format!("{}", e)
                    }))),
                }
            }
            None => Ok(ToolCallResult::json(json!({
                "connected": false,
                "status": "disconnected"
            }))),
        }
    }

    async fn tool_execute_js(&self, params: ExecuteJsParams) -> Result<ToolCallResult> {
        // Re-inject bridge if needed (handles page navigation/refresh)
        // ensure_devtools_bridge handles not-connected case with proper error
        if let Err(e) = self.ensure_devtools_bridge().await {
            // If not connected, return proper error; otherwise just warn and continue
            if e.to_string().contains("Not connected") {
                return Err(e);
            }
            debug!("Warning: Failed to ensure devtools bridge: {}", e);
        }

        // Single lock for actual work - no race condition
        let client = self.cdp_client.lock().await;
        let client = client
            .as_ref()
            .ok_or_else(|| anyhow!("Not connected. Call 'connect' first."))?;

        // Wrap code in IIFE if it contains return statements or multiple statements
        // This allows users to write either "document.title" or "return document.title"
        let code = if params.code.contains("return ") || params.code.contains(';') {
            format!("(() => {{ {} }})()", params.code)
        } else {
            params.code.clone()
        };

        let result = client.evaluate(&code, params.await_promise).await?;

        // Extract the actual value from CDP response
        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(json!({ "result": value }))),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_query_dom(&self, params: QueryDomParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const wait = {};
                const selector = {};
                const includeText = {};
                const includeHtml = {};
                
                function query() {{
                    const el = document.querySelector(selector);
                    if (!el) return null;
                    
                    const rect = el.getBoundingClientRect();
                    const result = {{
                        exists: true,
                        tagName: el.tagName.toLowerCase(),
                        id: el.id || null,
                        className: el.className || null,
                        attributes: {{}},
                        boundingRect: {{
                            x: rect.x,
                            y: rect.y,
                            width: rect.width,
                            height: rect.height
                        }},
                        childCount: el.children.length,
                        visible: rect.width > 0 && rect.height > 0 && getComputedStyle(el).display !== 'none'
                    }};
                    
                    for (const attr of el.attributes) {{
                        result.attributes[attr.name] = attr.value;
                    }}
                    
                    if (includeText) {{
                        result.textContent = (el.textContent || '').slice(0, 500);
                    }}
                    
                    if (includeHtml) {{
                        result.outerHtml = el.outerHTML.slice(0, 500);
                    }}
                    
                    return result;
                }}
                
                if (wait > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const result = query();
                            if (result || Date.now() - start > wait) {{
                                resolve(result || {{ exists: false }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}
                
                return query() || {{ exists: false }};
            }})()
            "#,
            params.wait_ms,
            serde_json::to_string(&params.selector)?,
            params.include_text,
            params.include_html
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Query error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "exists": false }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_query_all_dom(&self, params: QueryAllDomParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {};
                const limit = {};
                const elements = document.querySelectorAll(selector);
                const results = [];
                
                for (let i = 0; i < Math.min(elements.length, limit); i++) {{
                    const el = elements[i];
                    const rect = el.getBoundingClientRect();
                    results.push({{
                        index: i,
                        tagName: el.tagName.toLowerCase(),
                        id: el.id || null,
                        className: el.className || null,
                        textContent: (el.textContent || '').slice(0, 100),
                        boundingRect: {{ x: rect.x, y: rect.y, width: rect.width, height: rect.height }}
                    }});
                }}
                
                return {{
                    count: elements.length,
                    elements: results
                }};
            }})()
            "#,
            serde_json::to_string(&params.selector)?,
            params.limit
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Query error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "count": 0, "elements": [] }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_verify_component(&self, params: VerifyComponentParams) -> Result<ToolCallResult> {
        let assertions_json = serde_json::to_string(&params.assertions)?;

        let code = format!(
            r#"
            (() => {{
                const selector = {};
                const assertions = {};
                const timeout = {};
                
                function verify() {{
                    const el = document.querySelector(selector);
                    const results = [];
                    let allPassed = true;
                    
                    for (const assertion of assertions) {{
                        let passed = false;
                        let message = '';
                        
                        switch (assertion.type) {{
                            case 'exists':
                                passed = el !== null;
                                message = passed ? 'Element exists' : 'Element not found';
                                break;
                            case 'not_exists':
                                passed = el === null;
                                message = passed ? 'Element does not exist' : 'Element unexpectedly exists';
                                break;
                            case 'text_contains':
                                passed = el && (el.textContent || '').includes(assertion.value || '');
                                message = passed ? 'Text contains value' : `Text does not contain "${{assertion.value}}"`;
                                break;
                            case 'text_equals':
                                passed = el && (el.textContent || '').trim() === (assertion.value || '');
                                message = passed ? 'Text equals value' : `Text does not equal "${{assertion.value}}"`;
                                break;
                            case 'attribute_equals':
                                passed = el && el.getAttribute(assertion.attribute || '') === assertion.value;
                                message = passed ? 'Attribute equals value' : `Attribute "${{assertion.attribute}}" does not equal "${{assertion.value}}"`;
                                break;
                            case 'attribute_contains':
                                passed = el && (el.getAttribute(assertion.attribute || '') || '').includes(assertion.value || '');
                                message = passed ? 'Attribute contains value' : `Attribute "${{assertion.attribute}}" does not contain "${{assertion.value}}"`;
                                break;
                            case 'child_count_gte':
                                passed = el && el.children.length >= (assertion.count || 0);
                                message = passed ? `Has >= ${{assertion.count}} children` : `Has ${{el ? el.children.length : 0}} children, expected >= ${{assertion.count}}`;
                                break;
                            case 'child_count_lte':
                                passed = el && el.children.length <= (assertion.count || 0);
                                message = passed ? `Has <= ${{assertion.count}} children` : `Has ${{el ? el.children.length : 0}} children, expected <= ${{assertion.count}}`;
                                break;
                            case 'class_contains':
                                passed = el && el.classList.contains(assertion.value || '');
                                message = passed ? 'Has class' : `Does not have class "${{assertion.value}}"`;
                                break;
                            case 'visible':
                                if (el) {{
                                    const rect = el.getBoundingClientRect();
                                    const style = getComputedStyle(el);
                                    passed = rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden';
                                }}
                                message = passed ? 'Element is visible' : 'Element is not visible';
                                break;
                        }}
                        
                        results.push({{ type: assertion.type, passed, message }});
                        if (!passed) allPassed = false;
                    }}
                    
                    return {{ success: allPassed, assertions: results, elementFound: el !== null }};
                }}
                
                if (timeout > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const result = verify();
                            if (result.success || Date.now() - start > timeout) {{
                                resolve(result);
                            }} else {{
                                setTimeout(check, 100);
                            }}
                        }};
                        check();
                    }});
                }}
                
                return verify();
            }})()
            "#,
            serde_json::to_string(&params.selector)?,
            assertions_json,
            Self::clamp_timeout(params.timeout_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let clamped_timeout = Self::clamp_timeout(params.timeout_ms);
        let result = client.evaluate(&code, clamped_timeout > 0).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Verification error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "success": false, "assertions": [] }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_get_logs(&self, params: GetLogsParams) -> Result<ToolCallResult> {
        // Ensure bridge is initialized to capture logs
        if let Err(e) = self.ensure_devtools_bridge().await {
            if e.to_string().contains("Not connected") {
                return Err(e);
            }
            debug!("Warning: Failed to ensure devtools bridge for logs: {}", e);
        }

        let code = format!(
            r#"
            (() => {{
                if (!window.__DATAFUL__ || !window.__DATAFUL__.logs) {{
                    return {{
                        logs: [],
                        error: 'Devtools bridge not initialized. Logs are only captured after bridge injection.',
                        hint: 'Call connect() first, then clear_logs() before the actions you want to monitor.'
                    }};
                }}

                const level = {};
                const sinceTs = {};
                const limit = {};
                const search = {};

                let logs = window.__DATAFUL__.logs;

                if (level !== 'all') {{
                    logs = logs.filter(l => l.level === level);
                }}

                if (sinceTs) {{
                    logs = logs.filter(l => l.timestamp > sinceTs);
                }}

                if (search) {{
                    logs = logs.filter(l => l.message.includes(search));
                }}

                return {{
                    logs: logs.slice(-limit),
                    total: logs.length,
                    filtered: logs.length,
                    bridgeActive: true
                }};
            }})()
            "#,
            serde_json::to_string(&params.level)?,
            params
                .since_timestamp
                .map(|t| t.to_string())
                .unwrap_or("null".to_string()),
            params.limit,
            params
                .search
                .as_ref()
                .map(|s| serde_json::to_string(s).unwrap())
                .unwrap_or("null".to_string())
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Log retrieval error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "logs": [], "error": "Failed to get logs" }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_clear_logs(&self) -> Result<ToolCallResult> {
        let code = r#"
            (() => {
                if (window.__DATAFUL__ && window.__DATAFUL__.logs) {
                    const count = window.__DATAFUL__.logs.length;
                    window.__DATAFUL__.logs = [];
                    return { cleared: true, count };
                }
                return { cleared: false, error: 'Devtools bridge not initialized' };
            })()
        "#;

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(code, false).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Clear logs error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "cleared": false }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_get_state(&self, params: GetStateParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                if (!window.__DATAFUL__ || !window.__DATAFUL__.getState) {{
                    return {{ error: 'Devtools bridge not initialized or getState not available' }};
                }}
                
                try {{
                    const value = window.__DATAFUL__.getState({});
                    return {{ value }};
                }} catch (e) {{
                    return {{ error: e.message }};
                }}
            }})()
            "#,
            serde_json::to_string(&params.path)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Get state error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "error": "Failed to get state" }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_set_state(&self, params: SetStateParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                if (!window.__DATAFUL__ || !window.__DATAFUL__.setState) {{
                    return {{ error: 'Devtools bridge not initialized or setState not available' }};
                }}
                
                try {{
                    window.__DATAFUL__.setState({}, {});
                    return {{ success: true }};
                }} catch (e) {{
                    return {{ error: e.message }};
                }}
            }})()
            "#,
            serde_json::to_string(&params.path)?,
            serde_json::to_string(&params.value)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_dispatch(&self, params: DispatchParams) -> Result<ToolCallResult> {
        let payload_str = params
            .payload
            .map(|p| serde_json::to_string(&p).unwrap())
            .unwrap_or("undefined".to_string());

        let code = format!(
            r#"
            (() => {{
                if (!window.__DATAFUL__ || !window.__DATAFUL__.dispatch) {{
                    return {{ error: 'Devtools bridge not initialized or dispatch not available' }};
                }}
                
                try {{
                    const result = window.__DATAFUL__.dispatch({}, {});
                    if (result instanceof Promise) {{
                        return result.then(r => ({{ success: true, result: r }}))
                                     .catch(e => ({{ error: e.message }}));
                    }}
                    return {{ success: true, result }};
                }} catch (e) {{
                    return {{ error: e.message }};
                }}
            }})()
            "#,
            serde_json::to_string(&params.action)?,
            payload_str
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, true).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_navigate(&self, params: NavigateParams) -> Result<ToolCallResult> {
        // Try using Dataful's router first, fall back to window.location
        let code = format!(
            r#"
            (() => {{
                const route = {};
                
                // Try SvelteKit's goto if available
                if (window.__DATAFUL__ && window.__DATAFUL__.navigate) {{
                    return window.__DATAFUL__.navigate(route)
                        .then(() => ({{ success: true, method: 'dataful' }}))
                        .catch(e => ({{ error: e.message }}));
                }}
                
                // Fall back to history API
                try {{
                    window.history.pushState({{}}, '', route);
                    window.dispatchEvent(new PopStateEvent('popstate'));
                    return {{ success: true, method: 'history' }};
                }} catch (e) {{
                    return {{ error: e.message }};
                }}
            }})()
            "#,
            serde_json::to_string(&params.route)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, true).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_screenshot(&self, params: ScreenshotParams) -> Result<ToolCallResult> {
        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        // Build screenshot params - CDP doesn't accept quality for PNG format
        // so we must exclude the field entirely rather than setting it to null
        let screenshot_params = if params.format == "png" {
            json!({
                "format": "png",
                "captureBeyondViewport": params.full_page
            })
        } else {
            json!({
                "format": params.format,
                "quality": params.quality(),
                "captureBeyondViewport": params.full_page
            })
        };

        let result = client
            .call_method("Page.captureScreenshot", screenshot_params)
            .await?;

        let data = result
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| anyhow!("No screenshot data returned"))?;

        let mime_type = match params.format.as_str() {
            "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => "image/png",
        };

        Ok(ToolCallResult::image(
            data.to_string(),
            mime_type.to_string(),
        ))
    }

    async fn tool_get_app_info(&self) -> Result<ToolCallResult> {
        let code = r#"
            (() => {
                const info = {
                    url: window.location.href,
                    title: document.title,
                    userAgent: navigator.userAgent,
                    viewport: {
                        width: window.innerWidth,
                        height: window.innerHeight
                    },
                    devicePixelRatio: window.devicePixelRatio,
                    hasBridge: typeof window.__DATAFUL__ !== 'undefined'
                };
                
                if (window.__DATAFUL__ && window.__DATAFUL__.getAppInfo) {
                    Object.assign(info, window.__DATAFUL__.getAppInfo());
                }
                
                return info;
            })()
        "#;

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(code, false).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "App info error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({}));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_click(&self, params: ClickParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};
                const clickType = {click_type};
                const button = {button};

                function getButtonCode(btn) {{
                    switch (btn) {{
                        case 'left': return 0;
                        case 'middle': return 1;
                        case 'right': return 2;
                        default: return 0;
                    }}
                }}

                function doClick() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    const rect = el.getBoundingClientRect();
                    const x = rect.left + rect.width / 2;
                    const y = rect.top + rect.height / 2;
                    const buttonCode = getButtonCode(button);

                    // Create common event options
                    const eventOptions = {{
                        bubbles: true,
                        cancelable: true,
                        view: window,
                        clientX: x,
                        clientY: y,
                        button: buttonCode,
                        buttons: 1 << buttonCode
                    }};

                    // Dispatch mousedown
                    el.dispatchEvent(new MouseEvent('mousedown', eventOptions));

                    // Handle different click types
                    if (clickType === 'double') {{
                        // Double click
                        el.dispatchEvent(new MouseEvent('mouseup', eventOptions));
                        el.dispatchEvent(new MouseEvent('click', {{ ...eventOptions, detail: 1 }}));
                        el.dispatchEvent(new MouseEvent('mousedown', eventOptions));
                        el.dispatchEvent(new MouseEvent('mouseup', eventOptions));
                        el.dispatchEvent(new MouseEvent('click', {{ ...eventOptions, detail: 2 }}));
                        el.dispatchEvent(new MouseEvent('dblclick', {{ ...eventOptions, detail: 2 }}));
                        return {{ success: true, clickType: 'double', button }};
                    }} else if (clickType === 'triple') {{
                        // Triple click (select all text in element)
                        el.dispatchEvent(new MouseEvent('mouseup', eventOptions));
                        el.dispatchEvent(new MouseEvent('click', {{ ...eventOptions, detail: 1 }}));
                        el.dispatchEvent(new MouseEvent('mousedown', eventOptions));
                        el.dispatchEvent(new MouseEvent('mouseup', eventOptions));
                        el.dispatchEvent(new MouseEvent('click', {{ ...eventOptions, detail: 2 }}));
                        el.dispatchEvent(new MouseEvent('dblclick', {{ ...eventOptions, detail: 2 }}));
                        el.dispatchEvent(new MouseEvent('mousedown', eventOptions));
                        el.dispatchEvent(new MouseEvent('mouseup', eventOptions));
                        el.dispatchEvent(new MouseEvent('click', {{ ...eventOptions, detail: 3 }}));
                        return {{ success: true, clickType: 'triple', button }};
                    }} else {{
                        // Single click
                        el.dispatchEvent(new MouseEvent('mouseup', eventOptions));

                        if (button === 'right') {{
                            // Right click - dispatch contextmenu event
                            el.dispatchEvent(new MouseEvent('contextmenu', eventOptions));
                            return {{ success: true, clickType: 'single', button: 'right' }};
                        }} else {{
                            el.dispatchEvent(new MouseEvent('click', {{ ...eventOptions, detail: 1 }}));
                            return {{ success: true, clickType: 'single', button }};
                        }}
                    }}
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doClick());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doClick();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms),
            click_type = serde_json::to_string(&params.click_type)?,
            button = serde_json::to_string(&params.button)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_type_text(&self, params: TypeTextParams) -> Result<ToolCallResult> {
        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        // Focus element if selector provided
        if let Some(selector) = &params.selector {
            let focus_code = format!(
                r#"
                (() => {{
                    const el = document.querySelector({});
                    if (!el) return {{ success: false, error: 'Element not found' }};
                    el.focus();
                    return {{ success: true }};
                }})()
                "#,
                serde_json::to_string(selector)?
            );

            let focus_result = client.evaluate(&focus_code, false).await?;
            let focus_success = focus_result
                .get("result")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.get("success"))
                .and_then(|s| s.as_bool())
                .unwrap_or(false);

            if !focus_success {
                return Ok(ToolCallResult::json(json!({
                    "success": false,
                    "error": "Failed to focus element"
                })));
            }
        }

        // Clear if requested
        if params.clear_first {
            let clear_code = r#"
                (() => {
                    const el = document.activeElement;
                    if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) {
                        el.value = '';
                        el.dispatchEvent(new Event('input', { bubbles: true }));
                    }
                })()
            "#;
            client.evaluate(clear_code, false).await?;
        }

        // Type each character
        for ch in params.text.chars() {
            let key_params = json!({
                "type": "keyDown",
                "text": ch.to_string(),
                "unmodifiedText": ch.to_string(),
                "key": ch.to_string()
            });
            client
                .call_method("Input.dispatchKeyEvent", key_params.clone())
                .await?;

            let key_up = json!({
                "type": "keyUp",
                "key": ch.to_string()
            });
            client.call_method("Input.dispatchKeyEvent", key_up).await?;

            if params.delay_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(params.delay_ms)).await;
            }
        }

        Ok(ToolCallResult::json(json!({
            "success": true,
            "typed": params.text
        })))
    }

    async fn tool_wait_for(&self, params: WaitForParams) -> Result<ToolCallResult> {
        let code = match params.condition {
            WaitCondition::Selector => {
                let selector = params
                    .selector
                    .as_ref()
                    .ok_or_else(|| anyhow!("selector required"))?;
                format!(
                    r#"
                    new Promise((resolve) => {{
                        const timeout = {};
                        const selector = {};
                        const start = Date.now();
                        
                        const check = () => {{
                            if (document.querySelector(selector)) {{
                                resolve({{ success: true, waited: Date.now() - start }});
                            }} else if (Date.now() - start > timeout) {{
                                resolve({{ success: false, error: 'Timeout waiting for selector' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }})
                    "#,
                    params.timeout_ms(),
                    serde_json::to_string(selector)?
                )
            }
            WaitCondition::NoSelector => {
                let selector = params
                    .selector
                    .as_ref()
                    .ok_or_else(|| anyhow!("selector required"))?;
                format!(
                    r#"
                    new Promise((resolve) => {{
                        const timeout = {};
                        const selector = {};
                        const start = Date.now();

                        const check = () => {{
                            if (!document.querySelector(selector)) {{
                                resolve({{ success: true, waited: Date.now() - start }});
                            }} else if (Date.now() - start > timeout) {{
                                resolve({{ success: false, error: 'Timeout waiting for selector to disappear' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }})
                    "#,
                    params.timeout_ms(),
                    serde_json::to_string(selector)?
                )
            }
            WaitCondition::TextVisible => {
                let text = params
                    .text
                    .as_ref()
                    .ok_or_else(|| anyhow!("text required"))?;
                format!(
                    r#"
                    new Promise((resolve) => {{
                        const timeout = {};
                        const text = {};
                        const start = Date.now();

                        const check = () => {{
                            if (document.body.innerText.includes(text)) {{
                                resolve({{ success: true, waited: Date.now() - start }});
                            }} else if (Date.now() - start > timeout) {{
                                resolve({{ success: false, error: 'Timeout waiting for text' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }})
                    "#,
                    params.timeout_ms(),
                    serde_json::to_string(text)?
                )
            }
            WaitCondition::NetworkIdle => {
                format!(
                    r#"
                    new Promise((resolve) => {{
                        const timeout = {};
                        let lastActivity = Date.now();
                        const idleThreshold = 500;
                        const start = Date.now();

                        const observer = new PerformanceObserver((list) => {{
                            lastActivity = Date.now();
                        }});
                        observer.observe({{ entryTypes: ['resource'] }});

                        const check = () => {{
                            if (Date.now() - lastActivity > idleThreshold) {{
                                observer.disconnect();
                                resolve({{ success: true, waited: Date.now() - start }});
                            }} else if (Date.now() - start > timeout) {{
                                observer.disconnect();
                                resolve({{ success: false, error: 'Timeout waiting for network idle' }});
                            }} else {{
                                setTimeout(check, 100);
                            }}
                        }};
                        setTimeout(check, idleThreshold);
                    }})
                    "#,
                    params.timeout_ms()
                )
            }
            WaitCondition::AnimationComplete => {
                format!(
                    r#"
                    new Promise((resolve) => {{
                        const timeout = {};
                        const start = Date.now();

                        const check = () => {{
                            const animations = document.getAnimations();
                            const running = animations.filter(a => a.playState === 'running');

                            if (running.length === 0) {{
                                resolve({{ success: true, waited: Date.now() - start }});
                            }} else if (Date.now() - start > timeout) {{
                                resolve({{ success: false, error: 'Timeout waiting for animations', runningCount: running.length }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }})
                    "#,
                    params.timeout_ms()
                )
            }
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, true).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    // ============ New Tool Implementations (Phase 1 & 2) ============

    async fn tool_hover(&self, params: HoverParams) -> Result<ToolCallResult> {
        let position_js = params
            .position
            .as_ref()
            .map(|p| format!("{{ x: {}, y: {} }}", p.x, p.y))
            .unwrap_or_else(|| "null".to_string());

        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};
                const position = {position};

                function doHover() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    const rect = el.getBoundingClientRect();
                    let x = rect.left + rect.width / 2;
                    let y = rect.top + rect.height / 2;

                    if (position) {{
                        x = rect.left + position.x;
                        y = rect.top + position.y;
                    }}

                    // Dispatch mouse events
                    const mouseEnter = new MouseEvent('mouseenter', {{
                        bubbles: false,
                        clientX: x,
                        clientY: y,
                        view: window
                    }});
                    const mouseOver = new MouseEvent('mouseover', {{
                        bubbles: true,
                        clientX: x,
                        clientY: y,
                        view: window
                    }});
                    const mouseMove = new MouseEvent('mousemove', {{
                        bubbles: true,
                        clientX: x,
                        clientY: y,
                        view: window
                    }});

                    el.dispatchEvent(mouseEnter);
                    el.dispatchEvent(mouseOver);
                    el.dispatchEvent(mouseMove);

                    return {{ success: true, x, y, element: el.tagName }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doHover());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doHover();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms),
            position = position_js
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_press_key(&self, params: PressKeyParams) -> Result<ToolCallResult> {
        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        // Focus element if selector provided
        if let Some(selector) = &params.selector {
            let focus_code = format!(
                r#"
                (() => {{
                    const el = document.querySelector({});
                    if (el) {{ el.focus(); return true; }}
                    return false;
                }})()
                "#,
                serde_json::to_string(selector)?
            );
            client.evaluate(&focus_code, false).await?;
        }

        // Build modifier flags
        let mut modifier_flags = 0;
        for modifier in &params.modifiers {
            match modifier.as_str() {
                "Alt" => modifier_flags |= 1,
                "Control" => modifier_flags |= 2,
                "Meta" => modifier_flags |= 4,
                "Shift" => modifier_flags |= 8,
                _ => {}
            }
        }

        // Map key name to key code and text
        let (key, code, text) = map_key(&params.key);
        let key_code = get_key_code(&params.key);

        for _ in 0..params.repeat {
            // Key down
            let key_down = json!({
                "type": "keyDown",
                "key": key,
                "code": code,
                "text": text,
                "modifiers": modifier_flags,
                "windowsVirtualKeyCode": key_code,
                "nativeVirtualKeyCode": key_code
            });
            client
                .call_method("Input.dispatchKeyEvent", key_down)
                .await?;

            // Key up
            let key_up = json!({
                "type": "keyUp",
                "key": key,
                "code": code,
                "modifiers": modifier_flags,
                "windowsVirtualKeyCode": key_code,
                "nativeVirtualKeyCode": key_code
            });
            client.call_method("Input.dispatchKeyEvent", key_up).await?;
        }

        Ok(ToolCallResult::json(json!({
            "success": true,
            "key": params.key,
            "repeat": params.repeat
        })))
    }

    async fn tool_select_option(&self, params: SelectOptionParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const values = {values};
                const by = {by};
                const waitMs = {wait_ms};

                function doSelect() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};
                    if (el.tagName !== 'SELECT') return {{ success: false, error: 'Element is not a <select>' }};

                    const selected = [];

                    for (const option of el.options) {{
                        let matches = false;

                        for (const val of values) {{
                            if (by === 'value' && option.value === val) matches = true;
                            else if (by === 'label' && option.text === val) matches = true;
                            else if (by === 'index' && option.index === parseInt(val)) matches = true;
                        }}

                        if (matches) {{
                            option.selected = true;
                            selected.push({{ value: option.value, label: option.text, index: option.index }});
                        }} else if (!el.multiple) {{
                            option.selected = false;
                        }}
                    }}

                    // Dispatch change event
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));

                    return {{
                        success: selected.length > 0,
                        selected,
                        error: selected.length === 0 ? 'No matching options found' : null
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doSelect());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doSelect();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            values = serde_json::to_string(&params.values)?,
            by = serde_json::to_string(&params.by)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_go_back(&self, params: NavigationParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const timeout = {timeout};
                const beforeUrl = window.location.href;

                if (window.history.length <= 1) {{
                    return {{ success: false, error: 'No history to go back to' }};
                }}

                return new Promise((resolve) => {{
                    let resolved = false;

                    const done = (timedOut) => {{
                        if (resolved) return;
                        resolved = true;
                        const afterUrl = window.location.href;
                        const navigated = beforeUrl !== afterUrl;
                        resolve({{
                            success: navigated,
                            beforeUrl,
                            afterUrl,
                            navigated,
                            timedOut: timedOut && !navigated
                        }});
                    }};

                    // Listen for popstate (fires on history navigation)
                    window.addEventListener('popstate', () => {{
                        // Small delay to let URL update
                        setTimeout(() => done(false), 50);
                    }}, {{ once: true }});

                    // Timeout - but still check if navigation happened
                    setTimeout(() => done(true), timeout);

                    // Go back
                    window.history.back();
                }});
            }})()
            "#,
            timeout = Self::clamp_timeout(params.timeout_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, true).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_go_forward(&self, params: NavigationParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const timeout = {timeout};
                const beforeUrl = window.location.href;

                return new Promise((resolve) => {{
                    let resolved = false;

                    const done = (timedOut) => {{
                        if (resolved) return;
                        resolved = true;
                        const afterUrl = window.location.href;
                        const navigated = beforeUrl !== afterUrl;
                        resolve({{
                            success: navigated,
                            beforeUrl,
                            afterUrl,
                            navigated,
                            timedOut: timedOut && !navigated
                        }});
                    }};

                    // Listen for popstate (fires on history navigation)
                    window.addEventListener('popstate', () => {{
                        // Small delay to let URL update
                        setTimeout(() => done(false), 50);
                    }}, {{ once: true }});

                    // Timeout - but still check if navigation happened
                    setTimeout(() => done(true), timeout);

                    // Go forward
                    window.history.forward();
                }});
            }})()
            "#,
            timeout = Self::clamp_timeout(params.timeout_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, true).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_handle_dialog(&self, params: HandleDialogParams) -> Result<ToolCallResult> {
        let accept = params.action == "accept";
        let prompt_text = params.prompt_text.unwrap_or_default();

        let code = format!(
            r#"
            (() => {{
                const accept = {accept};
                const promptText = {prompt_text};

                // Store original methods
                window.__originalAlert = window.__originalAlert || window.alert;
                window.__originalConfirm = window.__originalConfirm || window.confirm;
                window.__originalPrompt = window.__originalPrompt || window.prompt;

                // Track dialogs
                window.__lastDialog = null;

                window.alert = function(message) {{
                    window.__lastDialog = {{ type: 'alert', message }};
                    // Alerts are always just acknowledged
                    return undefined;
                }};

                window.confirm = function(message) {{
                    window.__lastDialog = {{ type: 'confirm', message, result: accept }};
                    return accept;
                }};

                window.prompt = function(message, defaultValue) {{
                    window.__lastDialog = {{ type: 'prompt', message, defaultValue, result: accept ? promptText : null }};
                    return accept ? promptText : null;
                }};

                return {{
                    success: true,
                    action: accept ? 'accept' : 'dismiss',
                    message: 'Dialog handlers installed. Next dialog will be ' + (accept ? 'accepted' : 'dismissed')
                }};
            }})()
            "#,
            accept = accept,
            prompt_text = serde_json::to_string(&prompt_text)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_last_dialog(&self) -> Result<ToolCallResult> {
        let code = r#"
            (() => {
                return window.__lastDialog || { type: null, message: 'No dialog has been handled yet' };
            })()
        "#;

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(code, false).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Get last dialog error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "type": null }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_drag(&self, params: DragParams) -> Result<ToolCallResult> {
        // Validate we have either target_selector or target_position
        if params.target_selector.is_none() && params.target_position.is_none() {
            return Ok(ToolCallResult::error(
                "Either target_selector or target_position is required",
            ));
        }

        let target_selector_js = params
            .target_selector
            .as_ref()
            .map(|s| serde_json::to_string(s).unwrap())
            .unwrap_or_else(|| "null".to_string());

        let target_position_js = params
            .target_position
            .as_ref()
            .map(|p| format!("{{ x: {}, y: {} }}", p.x, p.y))
            .unwrap_or_else(|| "null".to_string());

        let code = format!(
            r#"
            (() => {{
                const sourceSelector = {source_selector};
                const targetSelector = {target_selector};
                const targetPosition = {target_position};

                const source = document.querySelector(sourceSelector);
                if (!source) return {{ success: false, error: 'Source element not found' }};

                const sourceRect = source.getBoundingClientRect();
                const startX = sourceRect.left + sourceRect.width / 2;
                const startY = sourceRect.top + sourceRect.height / 2;

                let endX, endY;

                if (targetSelector) {{
                    const target = document.querySelector(targetSelector);
                    if (!target) return {{ success: false, error: 'Target element not found' }};
                    const targetRect = target.getBoundingClientRect();
                    endX = targetRect.left + targetRect.width / 2;
                    endY = targetRect.top + targetRect.height / 2;
                }} else {{
                    endX = targetPosition.x;
                    endY = targetPosition.y;
                }}

                // Create and dispatch drag events
                const dataTransfer = new DataTransfer();

                // Mouse down on source
                source.dispatchEvent(new MouseEvent('mousedown', {{
                    bubbles: true, cancelable: true, clientX: startX, clientY: startY
                }}));

                // Drag start
                source.dispatchEvent(new DragEvent('dragstart', {{
                    bubbles: true, cancelable: true, clientX: startX, clientY: startY, dataTransfer
                }}));

                // Drag over (at target)
                const dragOverEvent = new DragEvent('dragover', {{
                    bubbles: true, cancelable: true, clientX: endX, clientY: endY, dataTransfer
                }});

                const targetEl = targetSelector ? document.querySelector(targetSelector) : document.elementFromPoint(endX, endY);
                if (targetEl) {{
                    targetEl.dispatchEvent(dragOverEvent);

                    // Drop
                    targetEl.dispatchEvent(new DragEvent('drop', {{
                        bubbles: true, cancelable: true, clientX: endX, clientY: endY, dataTransfer
                    }}));
                }}

                // Drag end
                source.dispatchEvent(new DragEvent('dragend', {{
                    bubbles: true, cancelable: true, clientX: endX, clientY: endY, dataTransfer
                }}));

                // Mouse up
                source.dispatchEvent(new MouseEvent('mouseup', {{
                    bubbles: true, cancelable: true, clientX: endX, clientY: endY
                }}));

                return {{
                    success: true,
                    from: {{ x: startX, y: startY }},
                    to: {{ x: endX, y: endY }}
                }};
            }})()
            "#,
            source_selector = serde_json::to_string(&params.source_selector)?,
            target_selector = target_selector_js,
            target_position = target_position_js
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_network_requests(
        &self,
        params: GetNetworkRequestsParams,
    ) -> Result<ToolCallResult> {
        let filter_js = params
            .filter
            .as_ref()
            .map(|f| serde_json::to_string(f).unwrap())
            .unwrap_or_else(|| "null".to_string());

        let code = format!(
            r#"
            (() => {{
                const filter = {filter};
                const limit = {limit};

                // Get from Performance API
                const entries = performance.getEntriesByType('resource');

                let requests = entries.map(e => ({{
                    url: e.name,
                    type: e.initiatorType,
                    duration: Math.round(e.duration),
                    size: e.transferSize || 0,
                    startTime: Math.round(e.startTime)
                }}));

                // Apply filters
                if (filter) {{
                    if (filter.url_contains) {{
                        requests = requests.filter(r => r.url.includes(filter.url_contains));
                    }}
                    if (filter.resource_type) {{
                        requests = requests.filter(r => r.type === filter.resource_type);
                    }}
                }}

                return {{
                    requests: requests.slice(-limit),
                    total: entries.length
                }};
            }})()
            "#,
            filter = filter_js,
            limit = params.limit
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Network requests error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "requests": [], "total": 0 }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_resize_viewport(&self, params: ResizeViewportParams) -> Result<ToolCallResult> {
        let (width, height) = if let Some(device) = &params.device {
            // Map device names to dimensions
            match device.to_lowercase().as_str() {
                "iphone 12" | "iphone 13" | "iphone 14" => (390, 844),
                "iphone 12 pro max" | "iphone 13 pro max" | "iphone 14 pro max" => (428, 926),
                "iphone se" => (375, 667),
                "ipad" | "ipad air" => (820, 1180),
                "ipad pro" => (1024, 1366),
                "ipad mini" => (768, 1024),
                "pixel 5" => (393, 851),
                "galaxy s21" => (360, 800),
                "desktop" | "desktop hd" | "1080p" => (1920, 1080),
                "desktop 2k" | "1440p" => (2560, 1440),
                "laptop" => (1366, 768),
                "macbook" | "macbook pro" => (1440, 900),
                _ => return Ok(ToolCallResult::error(format!("Unknown device: {}. Available: iPhone 12/13/14, iPhone SE, iPad, iPad Pro, iPad Mini, Pixel 5, Galaxy S21, Desktop, Laptop, MacBook", device)))
            }
        } else {
            (params.width.unwrap_or(1280), params.height.unwrap_or(720))
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        client
            .call_method(
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": width,
                    "height": height,
                    "deviceScaleFactor": 1,
                    "mobile": width < 768
                }),
            )
            .await?;

        Ok(ToolCallResult::json(json!({
            "success": true,
            "width": width,
            "height": height,
            "mobile": width < 768
        })))
    }

    // ============ Enhancement Tool Implementations ============

    async fn tool_focus(&self, params: FocusParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function doFocus() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    el.focus();

                    return {{
                        success: true,
                        element: el.tagName,
                        focused: document.activeElement === el
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doFocus());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doFocus();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_scroll(&self, params: ScrollParams) -> Result<ToolCallResult> {
        let selector_js = params
            .selector
            .as_ref()
            .map(|s| serde_json::to_string(s).unwrap())
            .unwrap_or_else(|| "null".to_string());

        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const x = {x};
                const y = {y};
                const behavior = {behavior};

                if (selector) {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    el.scrollIntoView({{ behavior, block: 'center', inline: 'center' }});

                    return {{
                        success: true,
                        action: 'scrollIntoView',
                        element: el.tagName
                    }};
                }} else {{
                    window.scrollTo({{ left: x, top: y, behavior }});

                    return {{
                        success: true,
                        action: 'scrollTo',
                        x: window.scrollX,
                        y: window.scrollY
                    }};
                }}
            }})()
            "#,
            selector = selector_js,
            x = params.x,
            y = params.y,
            behavior = serde_json::to_string(&params.behavior)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_fill(&self, params: FillParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const value = {value};
                const waitMs = {wait_ms};

                function doFill() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    // Check if it's an input-like element
                    const isInput = el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' ||
                                    el.contentEditable === 'true';
                    if (!isInput) {{
                        return {{ success: false, error: 'Element is not an input, textarea, or contenteditable' }};
                    }}

                    // Focus the element
                    el.focus();

                    // Clear existing value
                    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {{
                        el.value = '';
                    }} else {{
                        el.textContent = '';
                    }}

                    // Set new value
                    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {{
                        el.value = value;
                    }} else {{
                        el.textContent = value;
                    }}

                    // Dispatch events
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));

                    return {{
                        success: true,
                        element: el.tagName,
                        value: el.value || el.textContent
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doFill());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doFill();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            value = serde_json::to_string(&params.value)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_check(&self, params: CheckParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function doCheck() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    const isCheckable = el.type === 'checkbox' || el.type === 'radio';
                    if (!isCheckable) {{
                        return {{ success: false, error: 'Element is not a checkbox or radio button' }};
                    }}

                    if (!el.checked) {{
                        el.click();
                    }}

                    return {{
                        success: true,
                        checked: el.checked,
                        type: el.type
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doCheck());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doCheck();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_uncheck(&self, params: CheckParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function doUncheck() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    if (el.type !== 'checkbox') {{
                        return {{ success: false, error: 'Element is not a checkbox (cannot uncheck radio buttons)' }};
                    }}

                    if (el.checked) {{
                        el.click();
                    }}

                    return {{
                        success: true,
                        checked: el.checked
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doUncheck());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doUncheck();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_element_info(&self, params: GetElementInfoParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function getInfo() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    const rect = el.getBoundingClientRect();
                    const computedStyle = window.getComputedStyle(el);

                    // Get all attributes
                    const attributes = {{}};
                    for (const attr of el.attributes) {{
                        attributes[attr.name] = attr.value;
                    }}

                    // Get key computed styles
                    const styles = {{
                        display: computedStyle.display,
                        visibility: computedStyle.visibility,
                        opacity: computedStyle.opacity,
                        position: computedStyle.position,
                        width: computedStyle.width,
                        height: computedStyle.height,
                        color: computedStyle.color,
                        backgroundColor: computedStyle.backgroundColor,
                        fontSize: computedStyle.fontSize,
                        fontFamily: computedStyle.fontFamily,
                        zIndex: computedStyle.zIndex
                    }};

                    return {{
                        success: true,
                        tagName: el.tagName,
                        id: el.id || null,
                        className: el.className || null,
                        textContent: el.textContent?.substring(0, 500) || null,
                        innerText: el.innerText?.substring(0, 500) || null,
                        attributes,
                        boundingBox: {{
                            x: rect.x,
                            y: rect.y,
                            width: rect.width,
                            height: rect.height,
                            top: rect.top,
                            right: rect.right,
                            bottom: rect.bottom,
                            left: rect.left
                        }},
                        styles,
                        isVisible: rect.width > 0 && rect.height > 0 &&
                                   computedStyle.visibility !== 'hidden' &&
                                   computedStyle.display !== 'none' &&
                                   parseFloat(computedStyle.opacity) > 0,
                        childCount: el.children.length,
                        inputValue: el.value !== undefined ? el.value : null,
                        checked: el.type === 'checkbox' || el.type === 'radio' ? el.checked : null,
                        disabled: el.disabled !== undefined ? el.disabled : null
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(getInfo());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return getInfo();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    // ============ Utility Tool Implementations ============

    async fn tool_blur(&self, params: BlurParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function doBlur() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};
                    el.blur();
                    return {{ success: true, tagName: el.tagName }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doBlur());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doBlur();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_clear(&self, params: ClearParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function doClear() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    // Check if element supports value property
                    if (el.value === undefined) {{
                        return {{ success: false, error: 'Element does not have a value property' }};
                    }}

                    const oldValue = el.value;
                    el.value = '';

                    // Dispatch events to trigger any listeners
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));

                    return {{
                        success: true,
                        tagName: el.tagName,
                        oldValue: oldValue,
                        newValue: ''
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doClear());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doClear();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_reload(&self, params: ReloadParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const hard = {hard};
                const timeoutMs = {timeout_ms};

                return new Promise((resolve) => {{
                    const start = Date.now();

                    // Set up load listener before reloading
                    const onLoad = () => {{
                        window.removeEventListener('load', onLoad);
                        resolve({{
                            success: true,
                            hard: hard,
                            loadTime: Date.now() - start,
                            url: window.location.href
                        }});
                    }};

                    // Set timeout
                    setTimeout(() => {{
                        window.removeEventListener('load', onLoad);
                        resolve({{
                            success: true,
                            hard: hard,
                            timedOut: true,
                            message: 'Reload initiated but load event not received within timeout',
                            url: window.location.href
                        }});
                    }}, timeoutMs);

                    window.addEventListener('load', onLoad);

                    // Perform reload
                    if (hard) {{
                        window.location.reload(true);
                    }} else {{
                        window.location.reload();
                    }}
                }});
            }})()
            "#,
            hard = params.hard,
            timeout_ms = Self::clamp_timeout(params.timeout_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, true).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Ok(ToolCallResult::error(format!(
                "Reload error: {}",
                extract_cdp_error(exception)
            )));
        }

        let value = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!({ "success": true, "message": "Reload initiated" }));

        Ok(ToolCallResult::json(value))
    }

    async fn tool_get_url(&self) -> Result<ToolCallResult> {
        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate("window.location.href", false).await?;

        let url = result
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        Ok(ToolCallResult::json(json!({
            "success": true,
            "url": url
        })))
    }

    async fn tool_get_title(&self) -> Result<ToolCallResult> {
        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate("document.title", false).await?;

        let title = result
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        Ok(ToolCallResult::json(json!({
            "success": true,
            "title": title
        })))
    }

    // ============ Additional Enhancement Tool Implementations ============

    async fn tool_select_text(&self, params: SelectTextParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const start = {start};
                const end = {end};
                const waitMs = {wait_ms};

                function doSelect() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    // Check if element supports setSelectionRange
                    if (typeof el.setSelectionRange !== 'function') {{
                        return {{ success: false, error: 'Element does not support text selection' }};
                    }}

                    const textLength = el.value ? el.value.length : 0;
                    const actualEnd = end !== null ? Math.min(end, textLength) : textLength;
                    const actualStart = Math.min(start, actualEnd);

                    el.focus();
                    el.setSelectionRange(actualStart, actualEnd);

                    return {{
                        success: true,
                        start: actualStart,
                        end: actualEnd,
                        selectedText: el.value ? el.value.substring(actualStart, actualEnd) : ''
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(doSelect());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return doSelect();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            start = params.start,
            end = params
                .end
                .map(|e| e.to_string())
                .unwrap_or("null".to_string()),
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_attribute(&self, params: GetAttributeParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const attribute = {attribute};
                const waitMs = {wait_ms};

                function getAttr() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    const value = el.getAttribute(attribute);
                    return {{
                        success: true,
                        attribute: attribute,
                        value: value,
                        exists: value !== null
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(getAttr());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return getAttr();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            attribute = serde_json::to_string(&params.attribute)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_set_attribute(&self, params: SetAttributeParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const attribute = {attribute};
                const value = {value};
                const waitMs = {wait_ms};

                function setAttr() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    const oldValue = el.getAttribute(attribute);
                    el.setAttribute(attribute, value);

                    return {{
                        success: true,
                        attribute: attribute,
                        oldValue: oldValue,
                        newValue: value
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(setAttr());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return setAttr();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            attribute = serde_json::to_string(&params.attribute)?,
            value = serde_json::to_string(&params.value)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_local_storage(&self, params: GetStorageParams) -> Result<ToolCallResult> {
        let code = if let Some(key) = &params.key {
            format!(
                r#"
                (() => {{
                    const key = {};
                    const value = localStorage.getItem(key);
                    let parsedValue = value;
                    try {{
                        parsedValue = JSON.parse(value);
                    }} catch (e) {{}}
                    return {{
                        success: true,
                        key: key,
                        value: parsedValue,
                        exists: value !== null
                    }};
                }})()
                "#,
                serde_json::to_string(key)?
            )
        } else {
            r#"
            (() => {
                const items = {};
                for (let i = 0; i < localStorage.length; i++) {
                    const key = localStorage.key(i);
                    let value = localStorage.getItem(key);
                    try {
                        value = JSON.parse(value);
                    } catch (e) {}
                    items[key] = value;
                }
                return {
                    success: true,
                    items: items,
                    count: localStorage.length
                };
            })()
            "#
            .to_string()
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_set_local_storage(&self, params: SetStorageParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const key = {key};
                const value = {value};
                localStorage.setItem(key, value);
                return {{
                    success: true,
                    key: key,
                    value: value
                }};
            }})()
            "#,
            key = serde_json::to_string(&params.key)?,
            value = serde_json::to_string(&params.value)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_remove_local_storage(
        &self,
        params: RemoveStorageParams,
    ) -> Result<ToolCallResult> {
        let code = if let Some(key) = &params.key {
            format!(
                r#"
                (() => {{
                    const key = {};
                    const existed = localStorage.getItem(key) !== null;
                    localStorage.removeItem(key);
                    return {{
                        success: true,
                        key: key,
                        existed: existed
                    }};
                }})()
                "#,
                serde_json::to_string(key)?
            )
        } else {
            r#"
            (() => {
                const count = localStorage.length;
                localStorage.clear();
                return {
                    success: true,
                    cleared: true,
                    itemsRemoved: count
                };
            })()
            "#
            .to_string()
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_session_storage(&self, params: GetStorageParams) -> Result<ToolCallResult> {
        let code = if let Some(key) = &params.key {
            format!(
                r#"
                (() => {{
                    const key = {};
                    const value = sessionStorage.getItem(key);
                    let parsedValue = value;
                    try {{
                        parsedValue = JSON.parse(value);
                    }} catch (e) {{}}
                    return {{
                        success: true,
                        key: key,
                        value: parsedValue,
                        exists: value !== null
                    }};
                }})()
                "#,
                serde_json::to_string(key)?
            )
        } else {
            r#"
            (() => {
                const items = {};
                for (let i = 0; i < sessionStorage.length; i++) {
                    const key = sessionStorage.key(i);
                    let value = sessionStorage.getItem(key);
                    try {
                        value = JSON.parse(value);
                    } catch (e) {}
                    items[key] = value;
                }
                return {
                    success: true,
                    items: items,
                    count: sessionStorage.length
                };
            })()
            "#
            .to_string()
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_set_session_storage(&self, params: SetStorageParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const key = {key};
                const value = {value};
                sessionStorage.setItem(key, value);
                return {{
                    success: true,
                    key: key,
                    value: value
                }};
            }})()
            "#,
            key = serde_json::to_string(&params.key)?,
            value = serde_json::to_string(&params.value)?
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_remove_session_storage(
        &self,
        params: RemoveStorageParams,
    ) -> Result<ToolCallResult> {
        let code = if let Some(key) = &params.key {
            format!(
                r#"
                (() => {{
                    const key = {};
                    const existed = sessionStorage.getItem(key) !== null;
                    sessionStorage.removeItem(key);
                    return {{
                        success: true,
                        key: key,
                        existed: existed
                    }};
                }})()
                "#,
                serde_json::to_string(key)?
            )
        } else {
            r#"
            (() => {
                const count = sessionStorage.length;
                sessionStorage.clear();
                return {
                    success: true,
                    cleared: true,
                    itemsRemoved: count
                };
            })()
            "#
            .to_string()
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    // ============ Cookie and Additional Tool Implementations ============

    async fn tool_get_cookies(&self, params: GetCookiesParams) -> Result<ToolCallResult> {
        let code = if let Some(name) = &params.name {
            format!(
                r#"
                (() => {{
                    const targetName = {};
                    const cookies = document.cookie.split(';').reduce((acc, cookie) => {{
                        const [name, ...valueParts] = cookie.trim().split('=');
                        acc[name] = decodeURIComponent(valueParts.join('='));
                        return acc;
                    }}, {{}});

                    if (targetName in cookies) {{
                        return {{
                            success: true,
                            name: targetName,
                            value: cookies[targetName],
                            exists: true
                        }};
                    }} else {{
                        return {{
                            success: true,
                            name: targetName,
                            value: null,
                            exists: false
                        }};
                    }}
                }})()
                "#,
                serde_json::to_string(name)?
            )
        } else {
            r#"
            (() => {
                const cookies = document.cookie.split(';').reduce((acc, cookie) => {
                    if (cookie.trim()) {
                        const [name, ...valueParts] = cookie.trim().split('=');
                        acc[name] = decodeURIComponent(valueParts.join('='));
                    }
                    return acc;
                }, {});

                return {
                    success: true,
                    cookies: cookies,
                    count: Object.keys(cookies).length
                };
            })()
            "#
            .to_string()
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_set_cookie(&self, params: SetCookieParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const name = {name};
                const value = {value};
                const path = {path};
                const domain = {domain};
                const expires = {expires};
                const secure = {secure};
                const sameSite = {same_site};

                let cookieString = name + '=' + encodeURIComponent(value);
                cookieString += '; path=' + path;

                if (domain) {{
                    cookieString += '; domain=' + domain;
                }}

                if (expires) {{
                    const date = new Date();
                    date.setTime(date.getTime() + (expires * 1000));
                    cookieString += '; expires=' + date.toUTCString();
                }}

                if (secure) {{
                    cookieString += '; secure';
                }}

                if (sameSite) {{
                    cookieString += '; SameSite=' + sameSite;
                }}

                document.cookie = cookieString;

                return {{
                    success: true,
                    name: name,
                    value: value,
                    cookieString: cookieString
                }};
            }})()
            "#,
            name = serde_json::to_string(&params.name)?,
            value = serde_json::to_string(&params.value)?,
            path = serde_json::to_string(&params.path)?,
            domain = params
                .domain
                .as_ref()
                .map(|d| serde_json::to_string(d).unwrap_or("null".to_string()))
                .unwrap_or("null".to_string()),
            expires = params
                .expires
                .map(|e| e.to_string())
                .unwrap_or("null".to_string()),
            secure = params.secure,
            same_site = params
                .same_site
                .as_ref()
                .map(|s| serde_json::to_string(s).unwrap_or("null".to_string()))
                .unwrap_or("null".to_string())
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_remove_cookie(&self, params: RemoveCookieParams) -> Result<ToolCallResult> {
        // Build cookie removal parts with optional path and domain
        let path_str = params
            .path
            .as_ref()
            .map(|p| serde_json::to_string(p).unwrap_or_else(|_| r#""/""#.to_string()))
            .unwrap_or_else(|| r#""/""#.to_string());
        let domain_str = params
            .domain
            .as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_else(|_| "null".to_string()))
            .unwrap_or_else(|| "null".to_string());

        let code = if let Some(name) = &params.name {
            format!(
                r#"
                (() => {{
                    const name = {name};
                    const path = {path};
                    const domain = {domain};

                    // Build cookie string for removal - must match path/domain exactly
                    let cookieStr = name + '=; expires=Thu, 01 Jan 1970 00:00:00 UTC; path=' + path;
                    if (domain) {{
                        cookieStr += '; domain=' + domain;
                    }}
                    document.cookie = cookieStr;

                    return {{
                        success: true,
                        name: name,
                        path: path,
                        domain: domain,
                        removed: true
                    }};
                }})()
                "#,
                name = serde_json::to_string(name)?,
                path = path_str,
                domain = domain_str
            )
        } else {
            format!(
                r#"
                (() => {{
                    const path = {path};
                    const domain = {domain};
                    const cookies = document.cookie.split(';');
                    let removedCount = 0;

                    for (const cookie of cookies) {{
                        const name = cookie.split('=')[0].trim();
                        if (name) {{
                            let cookieStr = name + '=; expires=Thu, 01 Jan 1970 00:00:00 UTC; path=' + path;
                            if (domain) {{
                                cookieStr += '; domain=' + domain;
                            }}
                            document.cookie = cookieStr;
                            removedCount++;
                        }}
                    }}

                    return {{
                        success: true,
                        cleared: true,
                        path: path,
                        domain: domain,
                        removedCount: removedCount
                    }};
                }})()
                "#,
                path = path_str,
                domain = domain_str
            )
        };

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, false).await?;

        // Use helper for exception checking
        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_bounding_box(&self, params: GetBoundingBoxParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function getBoundingBox() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    const rect = el.getBoundingClientRect();
                    return {{
                        success: true,
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                        top: rect.top,
                        right: rect.right,
                        bottom: rect.bottom,
                        left: rect.left
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(getBoundingBox());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return getBoundingBox();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_evaluate_on_selector(
        &self,
        params: EvaluateOnSelectorParams,
    ) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const userCode = {user_code};
                const waitMs = {wait_ms};

                function evaluate() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    try {{
                        const fn = new Function('el', 'return (' + userCode + ')');
                        const result = fn(el);
                        return {{
                            success: true,
                            result: result
                        }};
                    }} catch (e) {{
                        return {{
                            success: false,
                            error: e.message
                        }};
                    }}
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(evaluate());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return evaluate();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            user_code = serde_json::to_string(&params.code)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_inner_text(&self, params: GetTextParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function getText() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    return {{
                        success: true,
                        text: el.innerText
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(getText());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return getText();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }

    async fn tool_get_inner_html(&self, params: GetTextParams) -> Result<ToolCallResult> {
        let code = format!(
            r#"
            (() => {{
                const selector = {selector};
                const waitMs = {wait_ms};

                function getHtml() {{
                    const el = document.querySelector(selector);
                    if (!el) return {{ success: false, error: 'Element not found' }};

                    return {{
                        success: true,
                        html: el.innerHTML
                    }};
                }}

                if (waitMs > 0) {{
                    return new Promise((resolve) => {{
                        const start = Date.now();
                        const check = () => {{
                            const el = document.querySelector(selector);
                            if (el) {{
                                resolve(getHtml());
                            }} else if (Date.now() - start > waitMs) {{
                                resolve({{ success: false, error: 'Element not found within timeout' }});
                            }} else {{
                                setTimeout(check, 50);
                            }}
                        }};
                        check();
                    }});
                }}

                return getHtml();
            }})()
            "#,
            selector = serde_json::to_string(&params.selector)?,
            wait_ms = Self::clamp_timeout(params.wait_ms)
        );

        let client = self.cdp_client.lock().await;
        let client = client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let result = client.evaluate(&code, params.wait_ms > 0).await?;

        match Self::extract_cdp_result(&result) {
            Ok(value) => Ok(ToolCallResult::json(value)),
            Err(err_msg) => Ok(ToolCallResult::error(err_msg)),
        }
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::McpServer;
    use serde_json::json;

    #[test]
    fn score_debug_target_prefers_app_page_over_devtools() {
        let devtools = json!({
            "type": "page",
            "title": "DevTools - 127.0.0.1:5173/",
            "url": "devtools://devtools/bundled/devtools_app.html",
        });
        let app = json!({
            "type": "page",
            "title": "Testing",
            "url": "http://127.0.0.1:5173/",
        });

        assert!(McpServer::score_debug_target(&app) > McpServer::score_debug_target(&devtools));
    }

    #[test]
    fn score_debug_target_rejects_non_page_targets() {
        let worker = json!({
            "type": "worker",
            "title": "",
            "url": "",
        });

        assert_eq!(McpServer::score_debug_target(&worker), i32::MIN);
    }
}
