import * as vscode from 'vscode';

export interface ChatMessage {
    role: string;
    content: string;
}

export class AgentClient {
    private ws: WebSocket | null = null;
    private messageCallbacks: Array<(data: any) => void> = [];
    private reconnectTimer: ReturnType<typeof setInterval> | null = null;
    private connected: boolean = false;

    constructor(private daemonUrl: string) {}

    async connect(): Promise<boolean> {
        try {
            const healthUrl = `${this.daemonUrl.replace(/\/+$/, '')}/api/health`;
            const response = await fetch(healthUrl, {
                method: 'GET',
                signal: AbortSignal.timeout(5000),
            });
            if (!response.ok) {
                return false;
            }
            this.startWebSocket();
            this.startReconnect();
            this.connected = true;
            return true;
        } catch {
            this.connected = false;
            return false;
        }
    }

    disconnect(): void {
        this.stopReconnect();
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
        this.connected = false;
    }

    isConnected(): boolean {
        return this.connected;
    }

    getDaemonUrl(): string {
        return this.daemonUrl;
    }

    async sendMessage(messages: ChatMessage[]): Promise<string> {
        const config = vscode.workspace.getConfiguration('agentos');
        const apiKey = config.get<string>('llm.apiKey') || undefined;
        const baseUrl = config.get<string>('llm.baseUrl') || undefined;
        const model = config.get<string>('llm.model') || undefined;

        const apiUrl = `${this.daemonUrl.replace(/\/+$/, '')}/api/chat`;
        const response = await fetch(apiUrl, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ messages, api_key: apiKey, base_url: baseUrl, model }),
            // No timeout: complex tasks can take minutes.
            // Connectivity is monitored via WebSocket heartbeat (30s ping)
            // and reconnect timer (5s check). As long as heartbeat exists,
            // Agent OS is alive and processing.
        });
        if (!response.ok) {
            throw new Error(`Chat request failed: ${response.statusText}`);
        }
        const data = await response.json();
        return data.content || data.response || JSON.stringify(data);
    }

    onMessage(callback: (data: any) => void): void {
        this.messageCallbacks.push(callback);
    }

    private startWebSocket(): void {
        const wsUrl = this.daemonUrl.replace(/^http/, 'ws').replace(/\/+$/, '') + '/ws/events';
        try {
            this.ws = new WebSocket(wsUrl);
            this.ws.onopen = () => {
                this.connected = true;
            };
            this.ws.onmessage = (event: MessageEvent) => {
                try {
                    const data = JSON.parse(event.data);
                    for (const cb of this.messageCallbacks) {
                        try {
                            cb(data);
                        } catch { }
                    }
                } catch { }
            };
            this.ws.onclose = () => {
                this.connected = false;
                this.ws = null;
            };
            this.ws.onerror = () => {
                this.connected = false;
            };
        } catch {
            this.connected = false;
        }
    }

    private startReconnect(): void {
        if (this.reconnectTimer) return;
        this.reconnectTimer = setInterval(() => {
            if (!this.connected || !this.ws) {
                this.disconnect();
                this.startWebSocket();
            }
        }, 5000);
    }

    private stopReconnect(): void {
        if (this.reconnectTimer) {
            clearInterval(this.reconnectTimer);
            this.reconnectTimer = null;
        }
    }
}