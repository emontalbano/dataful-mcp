// Dataful Devtools Bridge
// This script is injected by the MCP server to enable communication
// For full functionality, Dataful should provide its own bridge implementation

(function() {
    if (window.__DATAFUL__) {
        console.log('[MCP] Devtools bridge already initialized');
        return;
    }

    const MAX_LOGS = 500;

    // Store for captured console logs
    const logs = [];

    // Capture console methods
    const originalConsole = {};
    ['log', 'warn', 'error', 'info', 'debug'].forEach(level => {
        originalConsole[level] = console[level];
        console[level] = function(...args) {
            logs.push({
                level,
                message: args.map(arg => {
                    if (arg === null) return 'null';
                    if (arg === undefined) return 'undefined';
                    if (typeof arg === 'object') {
                        try {
                            return JSON.stringify(arg);
                        } catch (e) {
                            return String(arg);
                        }
                    }
                    return String(arg);
                }).join(' '),
                timestamp: Date.now()
            });
            
            // Keep log buffer bounded
            while (logs.length > MAX_LOGS) {
                logs.shift();
            }
            
            // Call original
            originalConsole[level].apply(console, args);
        };
    });

    // Helper to get nested property by path
    function getByPath(obj, path) {
        if (!path) return obj;
        const parts = path.split('.');
        let current = obj;
        for (const part of parts) {
            if (current === null || current === undefined) return undefined;
            current = current[part];
        }
        return current;
    }

    // Helper to set nested property by path
    function setByPath(obj, path, value) {
        if (!path) return;
        const parts = path.split('.');
        let current = obj;
        for (let i = 0; i < parts.length - 1; i++) {
            if (current[parts[i]] === undefined) {
                current[parts[i]] = {};
            }
            current = current[parts[i]];
        }
        current[parts[parts.length - 1]] = value;
    }

    // Basic state store (Dataful should override with real implementation)
    let appState = {};

    window.__DATAFUL__ = {
        // Console log buffer
        logs,

        // State management (stub - Dataful should provide real implementation)
        getState(path) {
            // Check if Dataful has registered a custom state getter
            if (window.__DATAFUL_STATE_GETTER__) {
                return window.__DATAFUL_STATE_GETTER__(path);
            }
            return getByPath(appState, path);
        },

        setState(path, value) {
            // Check if Dataful has registered a custom state setter
            if (window.__DATAFUL_STATE_SETTER__) {
                return window.__DATAFUL_STATE_SETTER__(path, value);
            }
            setByPath(appState, path, value);
        },

        // Action dispatch (stub - Dataful should provide real implementation)
        dispatch(action, payload) {
            // Check if Dataful has registered a custom dispatcher
            if (window.__DATAFUL_DISPATCHER__) {
                return window.__DATAFUL_DISPATCHER__(action, payload);
            }
            
            // Default: dispatch custom event
            const event = new CustomEvent('dataful:action', {
                detail: { action, payload }
            });
            window.dispatchEvent(event);
            return Promise.resolve({ dispatched: true, action });
        },

        // Navigation (stub - Dataful should provide real implementation)
        navigate(route) {
            // Check if Dataful has registered a custom navigator
            if (window.__DATAFUL_NAVIGATOR__) {
                return window.__DATAFUL_NAVIGATOR__(route);
            }
            
            // Default: use history API
            window.history.pushState({}, '', route);
            window.dispatchEvent(new PopStateEvent('popstate'));
            return Promise.resolve();
        },

        // Component query helper
        queryComponent(selector) {
            const el = document.querySelector(selector);
            if (!el) return null;
            
            const rect = el.getBoundingClientRect();
            return {
                exists: true,
                tagName: el.tagName.toLowerCase(),
                id: el.id || null,
                className: el.className || null,
                attributes: Object.fromEntries(
                    Array.from(el.attributes).map(a => [a.name, a.value])
                ),
                textContent: (el.textContent || '').slice(0, 500),
                boundingRect: {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height
                },
                childCount: el.children.length,
                visible: rect.width > 0 && rect.height > 0 && 
                         getComputedStyle(el).display !== 'none' &&
                         getComputedStyle(el).visibility !== 'hidden'
            };
        },

        // App info (stub - Dataful should provide real implementation)
        getAppInfo() {
            // Check if Dataful has registered custom app info
            if (window.__DATAFUL_APP_INFO__) {
                return window.__DATAFUL_APP_INFO__();
            }
            
            return {
                version: 'unknown',
                environment: 'development',
                bridgeVersion: '1.0.0'
            };
        },

        // Register custom handlers (called by Dataful to provide real implementations)
        registerStateGetter(fn) {
            window.__DATAFUL_STATE_GETTER__ = fn;
        },
        
        registerStateSetter(fn) {
            window.__DATAFUL_STATE_SETTER__ = fn;
        },
        
        registerDispatcher(fn) {
            window.__DATAFUL_DISPATCHER__ = fn;
        },
        
        registerNavigator(fn) {
            window.__DATAFUL_NAVIGATOR__ = fn;
        },
        
        registerAppInfo(fn) {
            window.__DATAFUL_APP_INFO__ = fn;
        }
    };

    console.log('[MCP] Devtools bridge initialized');
})();
