# Dataful MCP

An MCP (Model Context Protocol) server for testing and verifying the Dataful application. This enables AI coding assistants like Claude Code to verify that code changes actually work in the running application.

## Features

- **Connection Management**: Connect to a running Dataful instance via Chrome DevTools Protocol
- **JavaScript Execution**: Run arbitrary JS in the webview context
- **DOM Queries**: Query elements, verify components rendered correctly
- **Console Log Capture**: Retrieve and filter console output
- **State Management**: Read/write application state
- **Action Dispatch**: Trigger app actions and navigation
- **Screenshots**: Capture the current view
- **Input Simulation**: Click elements, type text

## Setup

### 1. Build the MCP Server

```bash
cd dataful-mcp
cargo build --release
```

The binary will be at `target/release/dataful-mcp`.

### 2. Configure Dataful for Debugging

When running Dataful in development, enable the WebKit inspector:

```bash
# On macOS/Linux
WEBKIT_INSPECTOR_SERVER=127.0.0.1:9222 cargo tauri dev

# Or add to your dev script in package.json
```

For Windows, set the environment variable before running:
```powershell
$env:WEBKIT_INSPECTOR_SERVER="127.0.0.1:9222"
cargo tauri dev
```

### 3. Configure Claude Code

Add to your Claude Code MCP settings (`.claude/mcp.json` or global config):

```json
{
  "mcpServers": {
    "dataful": {
      "command": "/path/to/dataful-mcp/target/release/dataful-mcp",
      "env": {
        "RUST_LOG": "dataful_mcp=info"
      }
    }
  }
}
```

## Usage

### Basic Workflow

1. Start Dataful in dev mode with inspector enabled
2. Claude Code connects via the MCP
3. Make code changes
4. Verify changes work by querying the app

### Example: Verify a Component Renders

```
// Claude Code would call:
connect()
navigate({ route: "/cards" })
verify_component({
  selector: "[data-testid='card-grid']",
  assertions: [
    { type: "exists" },
    { type: "child_count_gte", count: 1 }
  ]
})
get_logs({ level: "error" })
```

### Available Tools

| Tool | Description |
|------|-------------|
| `connect` | Connect to running Dataful (default port 9222) |
| `disconnect` | Disconnect from app |
| `ping` | Check connection status |
| `execute_js` | Run JavaScript in webview |
| `query_dom` | Query single element by selector |
| `query_all_dom` | Query multiple elements |
| `verify_component` | Verify component with assertions |
| `get_logs` | Get captured console logs |
| `clear_logs` | Clear log buffer |
| `get_state` | Read app state by path |
| `set_state` | Write app state |
| `dispatch` | Dispatch an action |
| `navigate` | Navigate to route |
| `screenshot` | Capture screenshot |
| `get_app_info` | Get app version/info |
| `click` | Click an element |
| `type_text` | Type text into focused element |
| `wait_for` | Wait for condition |

## Dataful Integration

For full functionality, add the devtools bridge to Dataful's frontend:

```typescript
// src/lib/devtools/init.ts
import { appState } from '$lib/stores';
import { goto } from '$app/navigation';

export function initDevtools() {
  if (typeof window === 'undefined') return;
  
  // Wait for MCP bridge to be injected
  const init = () => {
    if (!window.__DATAFUL__) {
      setTimeout(init, 100);
      return;
    }
    
    // Register state access
    window.__DATAFUL__.registerStateGetter((path) => {
      // Return state from your Svelte stores
      return getStateByPath(appState, path);
    });
    
    window.__DATAFUL__.registerStateSetter((path, value) => {
      // Update your Svelte stores
      setStateByPath(appState, path, value);
    });
    
    // Register navigation
    window.__DATAFUL__.registerNavigator(async (route) => {
      await goto(route);
    });
    
    // Register app info
    window.__DATAFUL__.registerAppInfo(() => ({
      version: __APP_VERSION__,
      environment: import.meta.env.MODE,
      // Add micro-app info, profile state, etc.
    }));
    
    console.log('[Dataful] Devtools integration ready');
  };
  
  init();
}
```

Then call `initDevtools()` in your app's entry point.

## Verification Assertions

The `verify_component` tool supports these assertion types:

| Type | Description |
|------|-------------|
| `exists` | Element exists in DOM |
| `not_exists` | Element does not exist |
| `text_contains` | Text content contains value |
| `text_equals` | Text content equals value |
| `attribute_equals` | Attribute equals value |
| `attribute_contains` | Attribute contains value |
| `child_count_gte` | Has >= N children |
| `child_count_lte` | Has <= N children |
| `class_contains` | Has CSS class |
| `visible` | Element is visible |

## Troubleshooting

### "Failed to connect to debug port"

- Ensure Dataful is running with `WEBKIT_INSPECTOR_SERVER=127.0.0.1:9222`
- Check that port 9222 is not in use by another process
- Verify firewall isn't blocking localhost connections

### "Element not found"

- Add `wait_ms` parameter to wait for element to appear
- Check the selector is correct using browser devtools
- Verify you're on the correct route

### Console logs not appearing

- The MCP injects a minimal bridge if Dataful doesn't provide one
- For full integration, implement the Dataful-side bridge
- Logs are captured after connection, not retroactively

## Development

```bash
# Run with debug logging
RUST_LOG=dataful_mcp=debug cargo run

# Run tests
cargo test

# Build release
cargo build --release
```

## License

MIT
