import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { AgentClient, ChatMessage } from './agentClient';

export class ChatPanel {
    private panel: vscode.WebviewPanel | undefined;
    private messages: ChatMessage[] = [];
    private disposables: vscode.Disposable[] = [];

    constructor(private context: vscode.ExtensionContext, private agentClient: AgentClient) {
        this.agentClient.onMessage((data: any) => {
            if (data.type === 'stream_chunk') {
                this.postStreamChunk(data.chunk || data.content || '');
            } else if (data.type === 'chat' || data.type === 'message') {
                this.postCompleteMessage(data.role || 'assistant', data.content || '');
            }
        });
    }

    show(): void {
        const column = vscode.ViewColumn.Beside;
        if (this.panel) {
            this.panel.reveal(column);
            return;
        }

        this.panel = vscode.window.createWebviewPanel(
            'agentosChat',
            'AgentOS Chat',
            column,
            {
                enableScripts: true,
                retainContextWhenHidden: true,
                localResourceRoots: [
                    vscode.Uri.file(path.join(this.context.extensionPath, 'webview'))
                ]
            }
        );

        const htmlPath = path.join(this.context.extensionPath, 'webview', 'chatPanel.html');
        let htmlContent = '';
        try {
            htmlContent = fs.readFileSync(htmlPath, 'utf-8');

            const bundlePath = vscode.Uri.file(
                path.join(this.context.extensionPath, 'webview', 'chatPanel.bundle.js')
            );
            const bundleUri = this.panel.webview.asWebviewUri(bundlePath);
            htmlContent = htmlContent.replace('{{BUNDLE_SCRIPT_SRC}}', bundleUri.toString());
        } catch {
            htmlContent = this.getFallbackHtml();
        }
        this.panel.webview.html = htmlContent;

        this.panel.webview.onDidReceiveMessage(
            (message: any) => {
                if (message.type === 'sendMessage') {
                    this.sendMessage(message.text);
                } else if (message.type === 'clearMessages') {
                    this.messages = [];
                } else if (message.type === 'ready') {
                    this.restoreMessages();
                }
            }
        );

        this.panel.onDidDispose(() => {
            this.panel = undefined;
        });
    }

    private restoreMessages(): void {
        if (!this.panel) return;
        this.panel.webview.postMessage({ type: 'setMessages', messages: this.messages.map(m => ({
            id: Date.now() + Math.random(),
            role: m.role,
            content: m.content,
            time: new Date().toLocaleTimeString()
        }))});
    }

    postCompleteMessage(role: string, content: string): void {
        if (role === 'user') {
            this.messages.push({ role, content });
        }
        if (this.panel) {
            this.panel.webview.postMessage({
                type: 'addMessage',
                role,
                content,
            });
        }
    }

    postStreamChunk(chunk: string): void {
        if (!this.panel) return;
        this.panel.webview.postMessage({
            type: 'streamChunk',
            chunk,
        });
    }

    async sendMessage(text: string): Promise<void> {
        if (!this.panel) return;

        this.messages.push({ role: 'user', content: text });
        this.panel.webview.postMessage({
            type: 'addMessage',
            role: 'user',
            content: text,
        });

        this.panel.webview.postMessage({ type: 'streamStart' });

        try {
            const allMessages = [
                ...this.messages.slice(0, -1),
                { role: 'user' as const, content: text },
            ];

            const response = await this.agentClient.sendMessage(allMessages);

            const fullText = response || '';
            const chunkSize = 3;
            let index = 0;

            const streamInterval = setInterval(() => {
                if (!this.panel) {
                    clearInterval(streamInterval);
                    return;
                }
                if (index >= fullText.length) {
                    clearInterval(streamInterval);
                    this.messages.push({ role: 'assistant', content: fullText });
                    this.panel.webview.postMessage({
                        type: 'streamEnd',
                        fullText,
                    });
                    return;
                }
                const end = Math.min(index + chunkSize, fullText.length);
                this.panel.webview.postMessage({
                    type: 'streamChunk',
                    chunk: fullText.slice(index, end),
                });
                index = end;
            }, 15);
        } catch (err: any) {
            if (this.panel) {
                this.messages.push({ role: 'system', content: `Error: ${err.message}` });
                this.panel.webview.postMessage({
                    type: 'addMessage',
                    role: 'system',
                    content: `Error: ${err.message}`,
                });
                this.panel.webview.postMessage({ type: 'streamEnd', fullText: '' });
            }
        }
    }

    private getFallbackHtml(): string {
        return `<!DOCTYPE html><html><body><h2>Chat Panel</h2><p>Failed to load chat panel HTML.</p></body></html>`;
    }

    dispose(): void {
        if (this.panel) {
            this.panel.dispose();
            this.panel = undefined;
        }
        for (const d of this.disposables) {
            d.dispose();
        }
        this.disposables = [];
    }
}