# Codex Instructions for Dataful MCP

This document explains how to use the Dataful MCP to verify code changes in the running application.

## Quick Reference

### Before Making Changes
```
# Connect to running Dataful
mcp__dataful__connect()

# Clear logs so we only see new errors
mcp__dataful__clear_logs()
```

### After Making Changes
```
# Wait for HMR/rebuild, then verify
mcp__dataful__wait_for({ condition: "selector", selector: "[data-app-ready]", timeout_ms: 5000 })

# Check for errors
mcp__dataful__get_logs({ level: "error" })

# Verify specific component
mcp__dataful__verify_component({
  selector: "[data-testid='my-component']",
  assertions: [{ type: "exists" }],
  timeout_ms: 3000
})
```

## Workflow Patterns

### Pattern 1: Verify Component Renders
Use when adding or modifying a component:

```
mcp__dataful__navigate({ route: "/target-page" })
mcp__dataful__verify_component({
  selector: "[data-testid='new-component']",
  assertions: [
    { type: "exists" },
    { type: "visible" }
  ],
  timeout_ms: 3000
})
```

### Pattern 2: Verify No Errors After Change
Use after any code change:

```
mcp__dataful__clear_logs()
// ... make change, wait for HMR ...
mcp__dataful__get_logs({ level: "error" })
// If logs.length === 0, change is safe
```

### Pattern 3: Test User Interaction
Use when testing interactive features:

```
mcp__dataful__click({ selector: "button[data-action='submit']" })
mcp__dataful__wait_for({ condition: "selector", selector: ".success-message" })
mcp__dataful__query_dom({ selector: ".success-message" })
```

### Pattern 4: Verify State Changes
Use when testing state management:

```
mcp__dataful__dispatch({ action: "addItem", payload: { name: "test" } })
mcp__dataful__get_state({ path: "items" })
// Verify the item was added
```

### Pattern 5: Visual Verification
Use when layout matters:

```
mcp__dataful__screenshot({ format: "png" })
// Returns base64 image for visual inspection
```

## Best Practices

1. **Always connect first**: Call `connect()` at the start of a session
2. **Clear logs before changes**: Helps isolate new errors from existing ones
3. **Use data-testid**: Add `data-testid` attributes to components for reliable selection
4. **Add timeouts**: Use `wait_ms` and `timeout_ms` for async content
5. **Check for errors**: Always call `get_logs({ level: "error" })` after changes
6. **Disconnect when done**: Call `disconnect()` to clean up

## Selector Tips

Good selectors:
- `[data-testid='card-grid']` - Test IDs (most reliable)
- `#unique-id` - IDs (if stable)
- `.specific-class` - Classes (if unique)
- `button[type='submit']` - Attribute combinations

Avoid:
- Deeply nested selectors
- Index-based selectors (`:nth-child`)
- Class names that might change (`.svelte-abc123`)

## Error Handling

If a tool returns an error:

1. **Not connected**: Call `connect()` first
2. **Element not found**: Add `wait_ms` or check selector
3. **Timeout**: Increase timeout or check if page loaded
4. **JavaScript error**: Check the code syntax

## Verification Assertions

Available assertion types for `verify_component`:

| Type | Params | Description |
|------|--------|-------------|
| `exists` | - | Element is in DOM |
| `not_exists` | - | Element is NOT in DOM |
| `visible` | - | Element is visible |
| `text_contains` | `value` | Text includes value |
| `text_equals` | `value` | Text exactly matches |
| `attribute_equals` | `attribute`, `value` | Attr equals value |
| `attribute_contains` | `attribute`, `value` | Attr contains value |
| `class_contains` | `value` | Has CSS class |
| `child_count_gte` | `count` | Has >= N children |
| `child_count_lte` | `count` | Has <= N children |

## Example: Full Verification Flow

```
// 1. Connect
mcp__dataful__connect()

// 2. Navigate to target page
mcp__dataful__navigate({ route: "/dashboard" })

// 3. Clear logs
mcp__dataful__clear_logs()

// 4. Wait for page to load
mcp__dataful__wait_for({ 
  condition: "selector", 
  selector: "[data-page='dashboard']",
  timeout_ms: 5000 
})

// 5. Verify component structure
mcp__dataful__verify_component({
  selector: "[data-testid='dashboard-grid']",
  assertions: [
    { type: "exists" },
    { type: "visible" },
    { type: "child_count_gte", count: 3 }
  ]
})

// 6. Check for console errors
mcp__dataful__get_logs({ level: "error" })

// 7. Take screenshot for visual verification
mcp__dataful__screenshot()
```

## Debugging Tips

- Use `execute_js` to run arbitrary diagnostics:
  ```
  mcp__dataful__execute_js({ code: "document.querySelectorAll('*').length" })
  ```

- Get detailed element info:
  ```
  mcp__dataful__query_dom({ 
    selector: ".problem-element", 
    include_html: true 
  })
  ```

- Check app state:
  ```
  mcp__dataful__get_app_info()
  mcp__dataful__get_state({ path: "" })  // Get all state
  ```
