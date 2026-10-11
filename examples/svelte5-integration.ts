/**
 * Example Svelte 5 Integration for Dataful Devtools
 * 
 * This file shows how to integrate the MCP devtools bridge with a
 * Svelte 5 application using $state runes.
 * 
 * Copy and adapt this to your Dataful project structure.
 */

// Import your navigation function (SvelteKit example)
// import { goto } from '$app/navigation';

// ============ Types ============

interface AppState {
  user: User | null;
  settings: Settings;
  items: Item[];
  ui: UIState;
}

interface User {
  id: string;
  name: string;
  email: string;
}

interface Settings {
  theme: 'light' | 'dark' | 'system';
  language: string;
  notifications: boolean;
}

interface Item {
  id: string;
  name: string;
  data: unknown;
}

interface UIState {
  sidebarOpen: boolean;
  currentRoute: string;
  loading: boolean;
}

// ============ Global State (using Svelte 5 $state) ============

// In a real app, this would be in your stores file
// let appState = $state<AppState>({
//   user: null,
//   settings: { theme: 'dark', language: 'en', notifications: true },
//   items: [],
//   ui: { sidebarOpen: true, currentRoute: '/', loading: false }
// });

// For this example, we'll use a plain object
let appState: AppState = {
  user: null,
  settings: { theme: 'dark', language: 'en', notifications: true },
  items: [],
  ui: { sidebarOpen: true, currentRoute: '/', loading: false }
};

// ============ Action Handlers ============

const actionHandlers: Record<string, (payload?: unknown) => Promise<unknown> | unknown> = {
  // User actions
  'user.login': async (payload) => {
    const { email, password } = payload as { email: string; password: string };
    // Simulate login
    appState.user = { id: '1', name: 'Test User', email };
    return { success: true, user: appState.user };
  },
  
  'user.logout': () => {
    appState.user = null;
    return { success: true };
  },
  
  // Settings actions
  'settings.setTheme': (payload) => {
    const { theme } = payload as { theme: 'light' | 'dark' | 'system' };
    appState.settings.theme = theme;
    return { success: true, theme };
  },
  
  // Item actions
  'items.add': (payload) => {
    const item = payload as Item;
    appState.items.push(item);
    return { success: true, count: appState.items.length };
  },
  
  'items.remove': (payload) => {
    const { id } = payload as { id: string };
    appState.items = appState.items.filter(i => i.id !== id);
    return { success: true };
  },
  
  'items.clear': () => {
    appState.items = [];
    return { success: true };
  },
  
  // UI actions
  'ui.toggleSidebar': () => {
    appState.ui.sidebarOpen = !appState.ui.sidebarOpen;
    return { sidebarOpen: appState.ui.sidebarOpen };
  },
  
  'ui.setLoading': (payload) => {
    const { loading } = payload as { loading: boolean };
    appState.ui.loading = loading;
    return { loading };
  }
};

// ============ Path Utilities ============

function getByPath(obj: unknown, path: string): unknown {
  if (!path) return obj;
  
  const parts = path.split('.');
  let current: unknown = obj;
  
  for (const part of parts) {
    if (current === null || current === undefined) return undefined;
    current = (current as Record<string, unknown>)[part];
  }
  
  return current;
}

function setByPath(obj: Record<string, unknown>, path: string, value: unknown): void {
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

// ============ Devtools Bridge Initialization ============

export function initDevtoolsBridge(): void {
  if (typeof window === 'undefined') return;
  
  const waitForBridge = () => {
    // Wait for MCP to inject the bridge
    if (!window.__DATAFUL__) {
      setTimeout(waitForBridge, 100);
      return;
    }
    
    console.log('[Dataful] Registering devtools handlers');
    
    // Register state getter
    window.__DATAFUL__.registerStateGetter((path: string) => {
      return getByPath(appState, path);
    });
    
    // Register state setter
    window.__DATAFUL__.registerStateSetter((path: string, value: unknown) => {
      setByPath(appState as unknown as Record<string, unknown>, path, value);
    });
    
    // Register action dispatcher
    window.__DATAFUL__.registerDispatcher(async (action: string, payload?: unknown) => {
      const handler = actionHandlers[action];
      
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
    });
    
    // Register navigator
    window.__DATAFUL__.registerNavigator(async (route: string) => {
      // Update UI state
      appState.ui.currentRoute = route;
      
      // Use SvelteKit's goto if available
      // await goto(route);
      
      // Fallback to history API
      window.history.pushState({}, '', route);
      window.dispatchEvent(new PopStateEvent('popstate'));
    });
    
    // Register app info
    window.__DATAFUL__.registerAppInfo(() => ({
      version: '0.1.0', // Replace with your app version
      environment: 'development',
      user: appState.user?.email ?? null,
      theme: appState.settings.theme,
      itemCount: appState.items.length,
      // Add any other relevant info
    }));
    
    console.log('[Dataful] Devtools bridge ready');
  };
  
  waitForBridge();
}

// ============ Type Declarations ============

declare global {
  interface Window {
    __DATAFUL__?: {
      logs: Array<{ level: string; message: string; timestamp: number }>;
      registerStateGetter: (fn: (path: string) => unknown) => void;
      registerStateSetter: (fn: (path: string, value: unknown) => void) => void;
      registerDispatcher: (fn: (action: string, payload?: unknown) => Promise<unknown>) => void;
      registerNavigator: (fn: (route: string) => Promise<void>) => void;
      registerAppInfo: (fn: () => Record<string, unknown>) => void;
    };
  }
}

// ============ Export for use in App ============

export { appState, actionHandlers };
