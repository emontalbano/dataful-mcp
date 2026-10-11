use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::protocol::ToolDefinition;

/// All available tools in the Dataful MCP
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    let tools = vec![
        // Connection tools
        ToolDefinition {
            name: "connect".to_string(),
            description: "Connect to a running Dataful application instance. Must be called before using other tools.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "port": {
                        "type": "integer",
                        "description": "Debug port to connect to (default: 9222)",
                        "default": 9222
                    },
                    "host": {
                        "type": "string", 
                        "description": "Host to connect to (default: 127.0.0.1)",
                        "default": "127.0.0.1"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "disconnect".to_string(),
            description: "Disconnect from the Dataful application".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "ping".to_string(),
            description: "Check if connection to Dataful is alive".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },

        // JavaScript execution
        ToolDefinition {
            name: "execute_js".to_string(),
            description: "Execute JavaScript code in Dataful's webview context. Has access to window.__DATAFUL__ devtools bridge. Accepts expressions (e.g., 'document.title') or statements with return (e.g., 'return document.title').".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "JavaScript code to execute. Can be an expression like 'document.title' or use return statements like 'return document.title'"
                    },
                    "await_promise": {
                        "type": "boolean",
                        "description": "If the code returns a Promise, wait for it to resolve",
                        "default": true
                    }
                },
                "required": ["code"]
            }),
        },

        // DOM queries
        ToolDefinition {
            name: "query_dom".to_string(),
            description: "Query DOM elements using CSS selector and return their state".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector to query"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait up to N milliseconds for element to appear",
                        "default": 0
                    },
                    "include_text": {
                        "type": "boolean",
                        "description": "Include text content in response",
                        "default": true
                    },
                    "include_html": {
                        "type": "boolean",
                        "description": "Include outer HTML in response (truncated to 500 chars)",
                        "default": false
                    }
                },
                "required": ["selector"]
            }),
        },
        ToolDefinition {
            name: "query_all_dom".to_string(),
            description: "Query all matching DOM elements using CSS selector".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector to query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of elements to return",
                        "default": 10
                    }
                },
                "required": ["selector"]
            }),
        },
        ToolDefinition {
            name: "verify_component".to_string(),
            description: "High-level verification that a component rendered correctly with assertions".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector for the component root"
                    },
                    "assertions": {
                        "type": "array",
                        "description": "List of assertions to verify",
                        "items": {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["exists", "not_exists", "text_contains", "text_equals", "attribute_equals", "attribute_contains", "child_count_gte", "child_count_lte", "class_contains", "visible"]
                                },
                                "attribute": {
                                    "type": "string",
                                    "description": "Attribute name for attribute assertions"
                                },
                                "value": {
                                    "type": "string",
                                    "description": "Expected value"
                                },
                                "count": {
                                    "type": "integer",
                                    "description": "Expected count for child_count assertions"
                                }
                            },
                            "required": ["type"]
                        }
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Timeout in milliseconds to wait for assertions to pass",
                        "default": 2000
                    }
                },
                "required": ["selector"]
            }),
        },

        // Console logs
        ToolDefinition {
            name: "get_logs".to_string(),
            description: "Retrieve console logs captured from the Dataful application".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "level": {
                        "type": "string",
                        "enum": ["all", "error", "warn", "info", "log", "debug"],
                        "description": "Filter logs by level",
                        "default": "all"
                    },
                    "since_timestamp": {
                        "type": "integer",
                        "description": "Only return logs after this Unix timestamp (milliseconds)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of logs to return",
                        "default": 100
                    },
                    "search": {
                        "type": "string",
                        "description": "Filter logs containing this substring"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "clear_logs".to_string(),
            description: "Clear the captured console log buffer".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },

        // State management
        ToolDefinition {
            name: "get_state".to_string(),
            description: "Get application state from the Dataful devtools bridge. Use dot notation for nested paths (e.g., 'user.settings.theme').".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Dot-notation path to state (e.g., 'user.settings'). Empty string returns root state."
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "set_state".to_string(),
            description: "Set application state via the Dataful devtools bridge".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Dot-notation path to state"
                    },
                    "value": {
                        "description": "Value to set (any JSON-serializable value)"
                    }
                },
                "required": ["path", "value"]
            }),
        },

        // Actions/Navigation
        ToolDefinition {
            name: "dispatch".to_string(),
            description: "Dispatch an action through the Dataful devtools bridge".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action name to dispatch"
                    },
                    "payload": {
                        "description": "Optional payload for the action"
                    }
                },
                "required": ["action"]
            }),
        },
        ToolDefinition {
            name: "navigate".to_string(),
            description: "Navigate to a route in the Dataful application".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "route": {
                        "type": "string",
                        "description": "Route path to navigate to (e.g., '/settings', '/cards')"
                    }
                },
                "required": ["route"]
            }),
        },

        // Screenshots
        ToolDefinition {
            name: "screenshot".to_string(),
            description: "Capture a screenshot of the Dataful application window".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "enum": ["png", "jpeg", "webp"],
                        "description": "Image format",
                        "default": "png"
                    },
                    "quality": {
                        "type": "integer",
                        "description": "Quality for jpeg/webp (1-100)",
                        "default": 80
                    },
                    "full_page": {
                        "type": "boolean",
                        "description": "Capture full scrollable page",
                        "default": false
                    }
                }
            }),
        },

        // App info
        ToolDefinition {
            name: "get_app_info".to_string(),
            description: "Get information about the connected Dataful application".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },

        // Click/Input simulation
        ToolDefinition {
            name: "click".to_string(),
            description: "Click on an element. Supports single/double/triple click and left/right/middle button.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of element to click"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    },
                    "click_type": {
                        "type": "string",
                        "enum": ["single", "double", "triple"],
                        "description": "Type of click",
                        "default": "single"
                    },
                    "button": {
                        "type": "string",
                        "enum": ["left", "right", "middle"],
                        "description": "Mouse button to use",
                        "default": "left"
                    }
                },
                "required": ["selector"]
            }),
        },
        ToolDefinition {
            name: "type_text".to_string(),
            description: "Type text into a focused element or element matching selector".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to type"
                    },
                    "selector": {
                        "type": "string",
                        "description": "Optional CSS selector to focus first"
                    },
                    "clear_first": {
                        "type": "boolean",
                        "description": "Clear existing text before typing",
                        "default": false
                    },
                    "delay_ms": {
                        "type": "integer",
                        "description": "Delay between keystrokes in milliseconds",
                        "default": 0
                    }
                },
                "required": ["text"]
            }),
        },

        // Wait utilities
        ToolDefinition {
            name: "wait_for".to_string(),
            description: "Wait for a condition to be true".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "condition": {
                        "type": "string",
                        "enum": ["selector", "no_selector", "text_visible", "network_idle", "animation_complete"],
                        "description": "Type of condition to wait for"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector (for selector conditions)"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to look for (for text_visible)"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Maximum time to wait",
                        "default": 5000
                    }
                },
                "required": ["condition"]
            }),
        },

        // ============ New Tools (Phase 1 & 2) ============

        // Hover tool
        ToolDefinition {
            name: "hover".to_string(),
            description: "Hover the mouse over an element to trigger hover states, tooltips, or menus".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of element to hover over"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear before hovering",
                        "default": 0
                    },
                    "position": {
                        "type": "object",
                        "description": "Optional position within element to hover",
                        "properties": {
                            "x": { "type": "number" },
                            "y": { "type": "number" }
                        }
                    }
                },
                "required": ["selector"]
            }),
        },

        // Press key tool
        ToolDefinition {
            name: "press_key".to_string(),
            description: "Press a keyboard key or key combination".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key to press. Examples: 'Enter', 'Escape', 'Tab', 'ArrowDown', 'a', 'A', 'Backspace', 'Delete'"
                    },
                    "modifiers": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["Alt", "Control", "Meta", "Shift"]
                        },
                        "description": "Modifier keys to hold while pressing"
                    },
                    "selector": {
                        "type": "string",
                        "description": "Optional: focus this element before pressing key"
                    },
                    "repeat": {
                        "type": "integer",
                        "description": "Number of times to press the key",
                        "default": 1
                    }
                },
                "required": ["key"]
            }),
        },

        // Select option tool
        ToolDefinition {
            name: "select_option".to_string(),
            description: "Select one or more options in a <select> dropdown element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector for the <select> element"
                    },
                    "values": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Option value(s) to select. For single-select, provide one value. For multi-select, provide multiple."
                    },
                    "by": {
                        "type": "string",
                        "enum": ["value", "label", "index"],
                        "description": "How to match options: by value attribute, visible label text, or numeric index",
                        "default": "value"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector", "values"]
            }),
        },

        // Go back tool
        ToolDefinition {
            name: "go_back".to_string(),
            description: "Navigate back in browser history".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "wait_until": {
                        "type": "string",
                        "enum": ["load", "domcontentloaded", "networkidle"],
                        "description": "When to consider navigation complete",
                        "default": "load"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Navigation timeout in milliseconds",
                        "default": 5000
                    }
                }
            }),
        },

        // Go forward tool
        ToolDefinition {
            name: "go_forward".to_string(),
            description: "Navigate forward in browser history".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "wait_until": {
                        "type": "string",
                        "enum": ["load", "domcontentloaded", "networkidle"],
                        "description": "When to consider navigation complete",
                        "default": "load"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Navigation timeout in milliseconds",
                        "default": 5000
                    }
                }
            }),
        },

        // Handle dialog tool
        ToolDefinition {
            name: "handle_dialog".to_string(),
            description: "Handle JavaScript dialogs (alert, confirm, prompt). Call this to set up a handler before triggering an action that shows a dialog.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["accept", "dismiss"],
                        "description": "Whether to accept or dismiss the dialog"
                    },
                    "prompt_text": {
                        "type": "string",
                        "description": "Text to enter into prompt dialog (only used for prompt dialogs)"
                    }
                },
                "required": ["action"]
            }),
        },

        // Get last dialog tool
        ToolDefinition {
            name: "get_last_dialog".to_string(),
            description: "Get information about the last JavaScript dialog that was handled".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },

        // Drag tool
        ToolDefinition {
            name: "drag".to_string(),
            description: "Drag an element to another element or position".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source_selector": {
                        "type": "string",
                        "description": "CSS selector for the element to drag"
                    },
                    "target_selector": {
                        "type": "string",
                        "description": "CSS selector for the drop target element"
                    },
                    "target_position": {
                        "type": "object",
                        "description": "Alternative: drop at specific coordinates instead of target element",
                        "properties": {
                            "x": { "type": "number" },
                            "y": { "type": "number" }
                        }
                    }
                },
                "required": ["source_selector"]
            }),
        },

        // Get network requests tool
        ToolDefinition {
            name: "get_network_requests".to_string(),
            description: "Get network requests made by the page using the Performance API".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "object",
                        "properties": {
                            "url_contains": { "type": "string" },
                            "method": { "type": "string", "enum": ["GET", "POST", "PUT", "DELETE", "PATCH"] },
                            "status_code": { "type": "integer" },
                            "resource_type": {
                                "type": "string",
                                "enum": ["document", "stylesheet", "script", "image", "font", "xhr", "fetch", "other"]
                            }
                        }
                    },
                    "limit": {
                        "type": "integer",
                        "default": 50
                    }
                }
            }),
        },

        // Resize viewport tool
        ToolDefinition {
            name: "resize_viewport".to_string(),
            description: "Resize the browser viewport".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "width": {
                        "type": "integer",
                        "description": "Viewport width in pixels"
                    },
                    "height": {
                        "type": "integer",
                        "description": "Viewport height in pixels"
                    },
                    "device": {
                        "type": "string",
                        "description": "Preset device name (overrides width/height). Examples: 'iPhone 12', 'iPad', 'Desktop'"
                    }
                }
            }),
        },

        // ============ Additional Enhancement Tools ============

        // Focus tool
        ToolDefinition {
            name: "focus".to_string(),
            description: "Focus an element (useful before typing or pressing keys)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of element to focus"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Scroll tool
        ToolDefinition {
            name: "scroll".to_string(),
            description: "Scroll the page or an element into view".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of element to scroll into view. If omitted, scrolls the page."
                    },
                    "x": {
                        "type": "integer",
                        "description": "Horizontal scroll offset in pixels (when no selector)",
                        "default": 0
                    },
                    "y": {
                        "type": "integer",
                        "description": "Vertical scroll offset in pixels (when no selector)",
                        "default": 0
                    },
                    "behavior": {
                        "type": "string",
                        "enum": ["smooth", "instant", "auto"],
                        "description": "Scroll behavior",
                        "default": "smooth"
                    }
                }
            }),
        },

        // Fill tool (clear + type)
        ToolDefinition {
            name: "fill".to_string(),
            description: "Clear an input field and type new text (combines clear and type_text)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of input element"
                    },
                    "value": {
                        "type": "string",
                        "description": "Text to fill into the input"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector", "value"]
            }),
        },

        // Check tool (for checkboxes)
        ToolDefinition {
            name: "check".to_string(),
            description: "Check a checkbox or radio button (ensures it is checked)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of checkbox/radio element"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Uncheck tool (for checkboxes)
        ToolDefinition {
            name: "uncheck".to_string(),
            description: "Uncheck a checkbox (ensures it is unchecked)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of checkbox element"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Get element info tool
        ToolDefinition {
            name: "get_element_info".to_string(),
            description: "Get detailed information about an element including attributes, computed styles, and bounding box".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of element"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // ============ Utility Tools ============

        // Blur tool
        ToolDefinition {
            name: "blur".to_string(),
            description: "Remove focus from an element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of element to blur"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Clear tool
        ToolDefinition {
            name: "clear".to_string(),
            description: "Clear the value of an input or textarea element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of input element to clear"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Reload tool
        ToolDefinition {
            name: "reload".to_string(),
            description: "Reload the current page".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hard": {
                        "type": "boolean",
                        "description": "If true, bypass cache (hard reload)",
                        "default": false
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Wait for page to load",
                        "default": 5000
                    }
                }
            }),
        },

        // Get URL tool
        ToolDefinition {
            name: "get_url".to_string(),
            description: "Get the current page URL".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },

        // Get title tool
        ToolDefinition {
            name: "get_title".to_string(),
            description: "Get the current page title".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },

        // Select text tool
        ToolDefinition {
            name: "select_text".to_string(),
            description: "Select text within an input or textarea element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of the input/textarea element"
                    },
                    "start": {
                        "type": "integer",
                        "description": "Start position of selection (0-indexed). Defaults to 0",
                        "default": 0
                    },
                    "end": {
                        "type": "integer",
                        "description": "End position of selection. Defaults to end of text"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Get attribute tool
        ToolDefinition {
            name: "get_attribute".to_string(),
            description: "Get a specific attribute value from an element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of the element"
                    },
                    "attribute": {
                        "type": "string",
                        "description": "Name of the attribute to get"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector", "attribute"]
            }),
        },

        // Set attribute tool
        ToolDefinition {
            name: "set_attribute".to_string(),
            description: "Set an attribute value on an element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of the element"
                    },
                    "attribute": {
                        "type": "string",
                        "description": "Name of the attribute to set"
                    },
                    "value": {
                        "type": "string",
                        "description": "Value to set the attribute to"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector", "attribute", "value"]
            }),
        },

        // Get local storage tool
        ToolDefinition {
            name: "get_local_storage".to_string(),
            description: "Get a value from localStorage".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The localStorage key to get. If not provided, returns all keys and values"
                    }
                }
            }),
        },

        // Set local storage tool
        ToolDefinition {
            name: "set_local_storage".to_string(),
            description: "Set a value in localStorage".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The localStorage key to set"
                    },
                    "value": {
                        "type": "string",
                        "description": "The value to store (will be JSON stringified if not a string)"
                    }
                },
                "required": ["key", "value"]
            }),
        },

        // Remove local storage tool
        ToolDefinition {
            name: "remove_local_storage".to_string(),
            description: "Remove a value from localStorage".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The localStorage key to remove. If not provided, clears all localStorage"
                    }
                }
            }),
        },

        // Get session storage tool
        ToolDefinition {
            name: "get_session_storage".to_string(),
            description: "Get a value from sessionStorage".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The sessionStorage key to get. If not provided, returns all keys and values"
                    }
                }
            }),
        },

        // Set session storage tool
        ToolDefinition {
            name: "set_session_storage".to_string(),
            description: "Set a value in sessionStorage".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The sessionStorage key to set"
                    },
                    "value": {
                        "type": "string",
                        "description": "The value to store (will be JSON stringified if not a string)"
                    }
                },
                "required": ["key", "value"]
            }),
        },

        // Remove session storage tool
        ToolDefinition {
            name: "remove_session_storage".to_string(),
            description: "Remove a value from sessionStorage".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The sessionStorage key to remove. If not provided, clears all sessionStorage"
                    }
                }
            }),
        },

        // Get cookies tool
        ToolDefinition {
            name: "get_cookies".to_string(),
            description: "Get cookies from the current page".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of specific cookie to get. If not provided, returns all cookies"
                    }
                }
            }),
        },

        // Set cookie tool
        ToolDefinition {
            name: "set_cookie".to_string(),
            description: "Set a cookie on the current page".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Cookie name"
                    },
                    "value": {
                        "type": "string",
                        "description": "Cookie value"
                    },
                    "path": {
                        "type": "string",
                        "description": "Cookie path (default: /)",
                        "default": "/"
                    },
                    "domain": {
                        "type": "string",
                        "description": "Cookie domain (default: current domain)"
                    },
                    "expires": {
                        "type": "integer",
                        "description": "Expiration time in seconds from now. If not set, creates session cookie"
                    },
                    "secure": {
                        "type": "boolean",
                        "description": "Set Secure flag",
                        "default": false
                    },
                    "same_site": {
                        "type": "string",
                        "description": "SameSite attribute: Strict, Lax, or None",
                        "enum": ["Strict", "Lax", "None"]
                    }
                },
                "required": ["name", "value"]
            }),
        },

        // Remove cookie tool
        ToolDefinition {
            name: "remove_cookie".to_string(),
            description: "Remove a cookie from the current page. Must specify path/domain if the cookie was set with specific values.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the cookie to remove. If not provided, removes all cookies"
                    },
                    "path": {
                        "type": "string",
                        "description": "Path the cookie was set for (e.g., '/admin'). Required to remove cookies set with specific paths"
                    },
                    "domain": {
                        "type": "string",
                        "description": "Domain the cookie was set for (e.g., '.example.com'). Required to remove cookies set with specific domains"
                    }
                }
            }),
        },

        // Bounding box tool
        ToolDefinition {
            name: "get_bounding_box".to_string(),
            description: "Get the bounding box (position and dimensions) of an element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of the element"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Evaluate on selector tool
        ToolDefinition {
            name: "evaluate_on_selector".to_string(),
            description: "Evaluate JavaScript on a specific element (element available as 'el' in the code)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of the element"
                    },
                    "code": {
                        "type": "string",
                        "description": "JavaScript code to evaluate. The element is available as 'el'"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector", "code"]
            }),
        },

        // Inner text tool
        ToolDefinition {
            name: "get_inner_text".to_string(),
            description: "Get the visible text content of an element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of the element"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },

        // Inner HTML tool
        ToolDefinition {
            name: "get_inner_html".to_string(),
            description: "Get the inner HTML content of an element".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector of the element"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait for element to appear",
                        "default": 0
                    }
                },
                "required": ["selector"]
            }),
        },
    ];
    tools
}

#[cfg(test)]
mod retired_ux_tools_tests {
    use super::*;

    #[test]
    fn ordinary_runtime_tools_remain_without_formal_ux_tools() {
        let tools = get_tool_definitions();
        for name in ["connect", "execute_js", "query_dom", "get_logs"] {
            assert!(tools.iter().any(|tool| tool.name == name), "{name}");
        }
        assert!(tools.iter().all(|tool| !tool.name.starts_with("ux_")));
    }
}

// ============ Tool parameter types ============

#[derive(Debug, Deserialize)]
pub struct ConnectParams {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
}

fn default_port() -> u16 {
    9222
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

#[derive(Debug, Deserialize)]
pub struct ExecuteJsParams {
    pub code: String,
    #[serde(default = "default_true")]
    pub await_promise: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct QueryDomParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
    #[serde(default = "default_true")]
    pub include_text: bool,
    #[serde(default)]
    pub include_html: bool,
}

#[derive(Debug, Deserialize)]
pub struct QueryAllDomParams {
    pub selector: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Deserialize)]
pub struct VerifyComponentParams {
    pub selector: String,
    #[serde(default)]
    pub assertions: Vec<Assertion>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    2000
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Assertion {
    #[serde(rename = "type")]
    pub assertion_type: AssertionType,
    pub attribute: Option<String>,
    pub value: Option<String>,
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AssertionType {
    Exists,
    NotExists,
    TextContains,
    TextEquals,
    AttributeEquals,
    AttributeContains,
    ChildCountGte,
    ChildCountLte,
    ClassContains,
    Visible,
}

#[derive(Debug, Deserialize)]
pub struct GetLogsParams {
    #[serde(default = "default_log_level")]
    pub level: String,
    pub since_timestamp: Option<u64>,
    #[serde(default = "default_log_limit")]
    pub limit: usize,
    pub search: Option<String>,
}

fn default_log_level() -> String {
    "all".to_string()
}

fn default_log_limit() -> usize {
    100
}

#[derive(Debug, Deserialize)]
pub struct GetStateParams {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct SetStateParams {
    pub path: String,
    pub value: Value,
}

#[derive(Debug, Deserialize)]
pub struct DispatchParams {
    pub action: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct NavigateParams {
    pub route: String,
}

#[derive(Debug, Deserialize)]
pub struct ScreenshotParams {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_quality")]
    quality_raw: u8,
    #[serde(default)]
    pub full_page: bool,
}

impl ScreenshotParams {
    /// Get quality clamped to valid range 1-100
    pub fn quality(&self) -> u8 {
        self.quality_raw.clamp(1, 100)
    }
}

fn default_format() -> String {
    "png".to_string()
}

fn default_quality() -> u8 {
    80
}

#[derive(Debug, Deserialize)]
pub struct ClickParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
    #[serde(default = "default_click_type")]
    pub click_type: String,
    #[serde(default = "default_click_button")]
    pub button: String,
}

fn default_click_type() -> String {
    "single".to_string()
}

fn default_click_button() -> String {
    "left".to_string()
}

#[derive(Debug, Deserialize)]
pub struct BlurParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ClearParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ReloadParams {
    #[serde(default)]
    pub hard: bool,
    #[serde(default = "default_reload_timeout")]
    pub timeout_ms: u64,
}

fn default_reload_timeout() -> u64 {
    5000
}

#[derive(Debug, Deserialize)]
pub struct TypeTextParams {
    pub text: String,
    pub selector: Option<String>,
    #[serde(default)]
    pub clear_first: bool,
    #[serde(default)]
    pub delay_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct WaitForParams {
    pub condition: WaitCondition,
    pub selector: Option<String>,
    pub text: Option<String>,
    #[serde(default = "default_wait_timeout")]
    timeout_ms_raw: u64,
}

impl WaitForParams {
    /// Get timeout clamped to reasonable range (100ms - 60s)
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms_raw.clamp(100, 60000)
    }
}

fn default_wait_timeout() -> u64 {
    5000
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    Selector,
    NoSelector,
    TextVisible,
    NetworkIdle,
    AnimationComplete,
}

// ============ New Tool Parameters (Phase 1 & 2) ============

/// Position within an element (used by hover and drag)
#[derive(Debug, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
pub struct HoverParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
    pub position: Option<Position>,
}

#[derive(Debug, Deserialize)]
pub struct PressKeyParams {
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<String>,
    pub selector: Option<String>,
    #[serde(default = "default_one")]
    pub repeat: u32,
}

fn default_one() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
pub struct SelectOptionParams {
    pub selector: String,
    pub values: Vec<String>,
    #[serde(default = "default_select_by")]
    pub by: String,
    #[serde(default)]
    pub wait_ms: u64,
}

fn default_select_by() -> String {
    "value".to_string()
}

#[derive(Debug, Deserialize)]
pub struct NavigationParams {
    /// Wait until condition - kept for API compatibility even though simplified impl uses popstate
    #[allow(dead_code)]
    #[serde(default = "default_wait_until")]
    pub wait_until: String,
    #[serde(default = "default_nav_timeout")]
    pub timeout_ms: u64,
}

fn default_wait_until() -> String {
    "load".to_string()
}

fn default_nav_timeout() -> u64 {
    5000
}

#[derive(Debug, Deserialize)]
pub struct HandleDialogParams {
    pub action: String,
    pub prompt_text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DragParams {
    pub source_selector: String,
    pub target_selector: Option<String>,
    pub target_position: Option<Position>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NetworkFilter {
    pub url_contains: Option<String>,
    pub method: Option<String>,
    pub status_code: Option<u16>,
    pub resource_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetNetworkRequestsParams {
    pub filter: Option<NetworkFilter>,
    #[serde(default = "default_network_limit")]
    pub limit: usize,
}

fn default_network_limit() -> usize {
    50
}

#[derive(Debug, Deserialize)]
pub struct ResizeViewportParams {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub device: Option<String>,
}

// ============ Additional Enhancement Parameters ============

#[derive(Debug, Deserialize)]
pub struct FocusParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ScrollParams {
    pub selector: Option<String>,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "default_scroll_behavior")]
    pub behavior: String,
}

fn default_scroll_behavior() -> String {
    "smooth".to_string()
}

#[derive(Debug, Deserialize)]
pub struct FillParams {
    pub selector: String,
    pub value: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct CheckParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct GetElementInfoParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
}

// ============ Additional Enhancement Tool Parameters ============

#[derive(Debug, Deserialize)]
pub struct SelectTextParams {
    pub selector: String,
    #[serde(default)]
    pub start: u32,
    pub end: Option<u32>,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct GetAttributeParams {
    pub selector: String,
    pub attribute: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct SetAttributeParams {
    pub selector: String,
    pub attribute: String,
    pub value: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct GetStorageParams {
    pub key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetStorageParams {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct RemoveStorageParams {
    pub key: Option<String>,
}

// ============ Cookie and Additional Tool Parameters ============

#[derive(Debug, Deserialize)]
pub struct GetCookiesParams {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetCookieParams {
    pub name: String,
    pub value: String,
    #[serde(default = "default_cookie_path")]
    pub path: String,
    pub domain: Option<String>,
    pub expires: Option<u64>,
    #[serde(default)]
    pub secure: bool,
    pub same_site: Option<String>,
}

fn default_cookie_path() -> String {
    "/".to_string()
}

#[derive(Debug, Deserialize)]
pub struct RemoveCookieParams {
    pub name: Option<String>,
    /// Path the cookie was set for (needed to properly remove cookies set with specific paths)
    pub path: Option<String>,
    /// Domain the cookie was set for (needed to properly remove cookies set with specific domains)
    pub domain: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetBoundingBoxParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct EvaluateOnSelectorParams {
    pub selector: String,
    pub code: String,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct GetTextParams {
    pub selector: String,
    #[serde(default)]
    pub wait_ms: u64,
}
