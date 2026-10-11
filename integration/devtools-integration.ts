/**
 * Dataful Devtools Bridge Integration
 * 
 * Add this to your Dataful application to enable full MCP integration.
 * Call initDevtoolsBridge() in your app's entry point.
 * 
 * Example usage in +layout.svelte or App.svelte:
 * 
 * ```svelte
 * <script>
 *   import { initDevtoolsBridge } from '$lib/devtools';
 *   import { onMount } from 'svelte';
 *   
 *   onMount(() => {
 *     initDevtoolsBridge();
 *   });
 * </script>
 * ```
 */

// Type declarations for the global devtools bridge
declare global {
  interface Window {
    __DATAFUL__?: DatafulDevtools;
    __DATAFUL_STATE_GETTER__?: (path: string) => unknown;
    __DATAFUL_STATE_SETTER__?: (path: string, value: unknown) => void;
    __DATAFUL_DISPATCHER__?: (action: string, payload?: unknown) => Promise<unknown>;
    __DATAFUL_NAVIGATOR__?: (route: string) => Promise<void>;
    __DATAFUL_APP_INFO__?: () => AppInfo;
  }
}

interface DatafulDevtools {
  logs: LogEntry[];
  getState: (path: string) => unknown;
  setState: (path: string, value: unknown) => void;
  dispatch: (action: string, payload?: unknown) => Promise<unknown>;
  navigate: (route: string) => Promise<void>;
  queryComponent: (selector: string) => ComponentInfo | null;
  getAppInfo: () => AppInfo;
  registerStateGetter: (fn: (path: string) => unknown) => void;
  registerStateSetter: (fn: (path: string, value: unknown) => void) => void;
  registerDispatcher: (fn: (action: string, payload?: unknown) => Promise<unknown>) => void;
  registerNavigator: (fn: (route: string) => Promise<void>) => void;
  registerAppInfo: (fn: () => AppInfo) => void;
}

interface LogEntry {
  level: 'log' | 'info' | 'warn' | 'error' | 'debug';
  message: string;
  timestamp: number;
}

interface ComponentInfo {
  exists: boolean;
  tagName: string;
  id: string | null;
  className: string | null;
  attributes: Record<string, string>;
  textContent: string;
  boundingRect: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  childCount: number;
  visible: boolean;
}

interface AppInfo {
  version: string;
  environment: string;
  [key: string]: unknown;
}

// Configuration
export interface DevtoolsConfig {
  /**
   * Function to get state by dot-notation path
   * @example getState('user.settings.theme')
   */
  getState?: (path: string) => unknown;
  
  /**
   * Function to set state by dot-notation path
   */
  setState?: (path: string, value: unknown) => void;
  
  /**
   * Function to dispatch actions
   */
  dispatch?: (action: string, payload?: unknown) => Promise<unknown>;
  
  /**
   * Function to navigate to routes
   */
  navigate?: (route: string) => Promise<void>;
  
  /**
   * Function to get app info
   */
  getAppInfo?: () => AppInfo;
}

/**
 * Default implementation that uses Svelte stores
 * Customize this based on your state management approach
 */
export function createDefaultConfig(): DevtoolsConfig {
  return {
    getState: (path: string) => {
      console.warn('[Devtools] getState not configured, returning undefined for:', path);
      return undefined;
    },
    setState: (path: string, value: unknown) => {
      console.warn('[Devtools] setState not configured, ignoring:', path, value);
    },
    dispatch: async (action: string, payload?: unknown) => {
      console.warn('[Devtools] dispatch not configured, dispatching event:', action);
      window.dispatchEvent(new CustomEvent('dataful:action', {
        detail: { action, payload }
      }));
      return { dispatched: true, action };
    },
    navigate: async (route: string) => {
      console.warn('[Devtools] navigate not configured, using history API:', route);
      window.history.pushState({}, '', route);
      window.dispatchEvent(new PopStateEvent('popstate'));
    },
    getAppInfo: () => ({
      version: '0.0.0',
      environment: 'development'
    })
  };
}

/**
 * Initialize the devtools bridge with custom configuration
 */
export function initDevtoolsBridge(config: DevtoolsConfig = {}): void {
  if (typeof window === 'undefined') {
    return; // SSR guard
  }

  const finalConfig = { ...createDefaultConfig(), ...config };

  // Wait for MCP-injected bridge
  const init = () => {
    if (!window.__DATAFUL__) {
      // Bridge not injected yet, retry
      setTimeout(init, 100);
      return;
    }

    // Register our implementations
    if (finalConfig.getState) {
      window.__DATAFUL__.registerStateGetter(finalConfig.getState);
    }

    if (finalConfig.setState) {
      window.__DATAFUL__.registerStateSetter(finalConfig.setState);
    }

    if (finalConfig.dispatch) {
      window.__DATAFUL__.registerDispatcher(finalConfig.dispatch);
    }

    if (finalConfig.navigate) {
      window.__DATAFUL__.registerNavigator(finalConfig.navigate);
    }

    if (finalConfig.getAppInfo) {
      window.__DATAFUL__.registerAppInfo(finalConfig.getAppInfo);
    }

    console.log('[Dataful] Devtools bridge integration complete');
  };

  // Start initialization
  init();
}

// ============ Utility Functions ============

/**
 * Helper to get nested property by dot-notation path
 */
export function getByPath(obj: unknown, path: string): unknown {
  if (!path) return obj;
  
  const parts = path.split('.');
  let current: unknown = obj;
  
  for (const part of parts) {
    if (current === null || current === undefined) return undefined;
    current = (current as Record<string, unknown>)[part];
  }
  
  return current;
}

/**
 * Helper to set nested property by dot-notation path
 */
export function setByPath(obj: Record<string, unknown>, path: string, value: unknown): void {
  if (!path) return;
  
  const parts = path.split('.');
  let current: Record<string, unknown> = obj;
  
  for (let i = 0; i < parts.length - 1; i++) {
    if (current[parts[i]] === undefined) {
      current[parts[i]] = {};
    }
    current = current[parts[i]] as Record<string, unknown>;
  }
  
  current[parts[parts.length - 1]] = value;
}

// ============ Svelte 5 Integration Example ============

/**
 * Example integration with Svelte 5 $state runes
 * 
 * Usage:
 * ```typescript
 * // In your store file
 * import { createSvelteDevtoolsConfig } from './devtools-integration';
 * 
 * let appState = $state({
 *   user: null,
 *   settings: { theme: 'dark' },
 *   items: []
 * });
 * 
 * // In your entry point
 * initDevtoolsBridge(createSvelteDevtoolsConfig(
 *   () => appState,
 *   (newState) => { appState = newState }
 * ));
 * ```
 */
export function createSvelteDevtoolsConfig(
  getAppState: () => Record<string, unknown>,
  setAppState: (state: Record<string, unknown>) => void,
  options: {
    navigate?: (route: string) => Promise<void>;
    dispatch?: (action: string, payload?: unknown) => Promise<unknown>;
    appInfo?: () => AppInfo;
  } = {}
): DevtoolsConfig {
  return {
    getState: (path: string) => {
      return getByPath(getAppState(), path);
    },
    
    setState: (path: string, value: unknown) => {
      const state = { ...getAppState() };
      setByPath(state, path, value);
      setAppState(state);
    },
    
    navigate: options.navigate,
    dispatch: options.dispatch,
    getAppInfo: options.appInfo
  };
}

// ============ Action System Integration ============

/**
 * Create a dispatch function that integrates with a centralized action system
 */
export function createActionDispatcher(
  handlers: Record<string, (payload?: unknown) => Promise<unknown> | unknown>
): (action: string, payload?: unknown) => Promise<unknown> {
  return async (action: string, payload?: unknown) => {
    const handler = handlers[action];
    
    if (!handler) {
      console.warn(`[Devtools] Unknown action: ${action}`);
      return { error: `Unknown action: ${action}` };
    }
    
    try {
      const result = await handler(payload);
      return { success: true, result };
    } catch (error) {
      return { 
        success: false, 
        error: error instanceof Error ? error.message : String(error) 
      };
    }
  };
}

export default initDevtoolsBridge;
