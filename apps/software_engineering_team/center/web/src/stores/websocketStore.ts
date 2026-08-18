import { create } from 'zustand';
import type { WSEvent } from '@/types';
import { wsManager } from '@/api';

interface WebSocketState {
  connected: boolean;
  projectId: string | null;
  lastEvent: WSEvent | null;

  connect: (projectId: string) => void;
  disconnect: () => void;
  setConnected: (connected: boolean) => void;
}

export const useWebSocketStore = create<WebSocketState>((set) => ({
  connected: false,
  projectId: null,
  lastEvent: null,

  connect: (projectId: string) => {
    set({ projectId });
    wsManager.connect(projectId);
    wsManager.subscribe((event) => {
      set({ lastEvent: event });
    });

    const checkConnection = setInterval(() => {
      set({ connected: wsManager.connected });
    }, 1000);

    return () => clearInterval(checkConnection);
  },

  disconnect: () => {
    wsManager.disconnect();
    set({ connected: false, projectId: null, lastEvent: null });
  },

  setConnected: (connected: boolean) => set({ connected }),
}));