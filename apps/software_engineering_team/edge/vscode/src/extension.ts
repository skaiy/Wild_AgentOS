import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';
import { StatusBarManager } from './statusBar';
import { AgentClient } from './agentClient';
import { TaskPanel } from './taskPanel';
import { ChatPanel } from './chatPanel';
import { GraphPanel } from './graphPanel';

let agentClient: AgentClient;
let statusBarManager: StatusBarManager;
let taskPanel: TaskPanel;
let chatPanel: ChatPanel;
let graphPanel: GraphPanel;

class StatusProvider implements vscode.TreeDataProvider<vscode.TreeItem> {
    private _onDidChangeTreeData = new vscode.EventEmitter<vscode.TreeItem | undefined | null>();
    readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

    private connected = false;
    private daemonUrl = '';

    setStatus(connected: boolean, url: string): void {
        this.connected = connected;
        this.daemonUrl = url;
        this._onDidChangeTreeData.fire(null);
    }

    getTreeItem(element: vscode.TreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(): vscode.TreeItem[] {
        const items: vscode.TreeItem[] = [];

        const chatItem = new vscode.TreeItem('💬 Open Chat', vscode.TreeItemCollapsibleState.None);
        chatItem.command = { command: 'agentos.openChat', title: 'Open Chat', arguments: [] };
        chatItem.tooltip = 'Free-form AI chat (no task required)';
        items.push(chatItem);

        const statusItem = new vscode.TreeItem(
            this.connected ? '🟢 Connected' : '🔴 Disconnected',
            vscode.TreeItemCollapsibleState.None
        );
        statusItem.description = this.daemonUrl;
        statusItem.tooltip = this.connected ? 'Daemon is connected' : 'Daemon is not connected';
        items.push(statusItem);

        const daemonItem = new vscode.TreeItem('Edge Daemon', vscode.TreeItemCollapsibleState.None);
        daemonItem.description = this.daemonUrl;
        daemonItem.tooltip = 'Edge Daemon URL';
        items.push(daemonItem);

        return items;
    }
}

function getEnvDir(): string {
    return path.join(os.homedir(), '.agentos');
}

function getEnvPath(): string {
    return path.join(getEnvDir(), '.env');
}

function writeEnvFile(): void {
    const config = vscode.workspace.getConfiguration('agentos');
    const llmApiKey: string = config.get<string>('llm.apiKey') || '';
    const llmBaseUrl: string = config.get<string>('llm.baseUrl') || '';
    const llmModel: string = config.get<string>('llm.model') || '';

    const envDir = getEnvDir();
    if (!fs.existsSync(envDir)) {
        fs.mkdirSync(envDir, { recursive: true });
    }

    const envLines: string[] = [];
    if (llmApiKey) envLines.push(`LLM_API_KEY=${llmApiKey}`);
    if (llmBaseUrl) envLines.push(`LLM_BASE_URL=${llmBaseUrl}`);
    if (llmModel) envLines.push(`LLM_MODEL=${llmModel}`);

    fs.writeFileSync(getEnvPath(), envLines.join('\n') + '\n', 'utf-8');
}

export function activate(context: vscode.ExtensionContext): void {
    const config = vscode.workspace.getConfiguration('agentos');
    const daemonUrl: string = config.get('daemonUrl') || 'http://localhost:7890';
    const autoConnect: boolean = config.get('autoConnect') ?? true;

    writeEnvFile();

    agentClient = new AgentClient(daemonUrl);
    statusBarManager = new StatusBarManager(context);
    taskPanel = new TaskPanel(agentClient);
    chatPanel = new ChatPanel(context, agentClient);
    graphPanel = new GraphPanel(context);

    const statusProvider = new StatusProvider();
    vscode.window.registerTreeDataProvider('agentos.tasks', taskPanel);
    vscode.window.registerTreeDataProvider('agentos.status', statusProvider);

    context.subscriptions.push(
        vscode.commands.registerCommand('agentos.connect', async () => {
            const connected = await agentClient.connect();
            statusBarManager.updateStatus(connected);
            statusProvider.setStatus(connected, daemonUrl);
            if (connected) {
                vscode.window.showInformationMessage('AgentOS: Connected to daemon');
                taskPanel.refresh();
            } else {
                vscode.window.showErrorMessage('AgentOS: Failed to connect to daemon');
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentos.disconnect', () => {
            agentClient.disconnect();
            statusBarManager.updateStatus(false);
            statusProvider.setStatus(false, daemonUrl);
            vscode.window.showInformationMessage('AgentOS: Disconnected from daemon');
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentos.refreshTasks', () => {
            taskPanel.refresh();
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentos.openChat', () => {
            chatPanel.show();
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentos.openGraph', () => {
            graphPanel.show();
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentos.claimTask', (taskId: string) => {
            taskPanel.claimTask(taskId);
        })
    );

    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration('agentos.llm') || e.affectsConfiguration('agentos.daemonUrl')) {
                writeEnvFile();
                const newUrl = vscode.workspace.getConfiguration('agentos').get<string>('daemonUrl') || 'http://localhost:7890';
                if (newUrl !== daemonUrl) {
                    vscode.window.showInformationMessage(
                        'AgentOS: Daemon URL changed. Please reconnect using the "AgentOS: Connect" command.',
                        'Connect'
                    ).then(selection => {
                        if (selection === 'Connect') {
                            vscode.commands.executeCommand('agentos.connect');
                        }
                    });
                } else {
                    vscode.window.showInformationMessage(
                        'AgentOS: LLM settings updated. Restart the Edge Daemon to apply changes.',
                        'Restart Daemon'
                    ).then(selection => {
                        if (selection === 'Restart Daemon') {
                            vscode.commands.executeCommand('workbench.action.terminal.sendSequence', {
                                text: 'pkill -f "edge-daemon" && cd ' + daemonUrl.replace('http://', '').split(':')[0] + ' && ./edge-daemon &\r'
                            });
                        }
                    });
                }
            }
        })
    );

    if (autoConnect) {
        vscode.commands.executeCommand('agentos.connect');
    }
}

export function deactivate(): void {
    if (agentClient) {
        agentClient.disconnect();
    }
    if (statusBarManager) {
        statusBarManager.dispose();
    }
    if (chatPanel) {
        chatPanel.dispose();
    }
    if (graphPanel) {
        graphPanel.dispose();
    }
}