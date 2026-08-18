import * as vscode from 'vscode';

export class StatusBarManager {
    private statusBarItem: vscode.StatusBarItem;

    constructor(context: vscode.ExtensionContext) {
        this.statusBarItem = vscode.window.createStatusBarItem(
            vscode.StatusBarAlignment.Right,
            100
        );
        this.statusBarItem.tooltip = 'AgentOS Edge Daemon Status';
        this.statusBarItem.command = 'agentos.connect';
        context.subscriptions.push(this.statusBarItem);
        this.updateStatus(false);
    }

    updateStatus(connected: boolean): void {
        if (connected) {
            this.statusBarItem.text = '$(radio-tower) AgentOS: Connected';
            this.statusBarItem.backgroundColor = undefined;
            this.statusBarItem.color = '#4CAF50';
            this.statusBarItem.tooltip = 'AgentOS Edge Daemon - Connected';
        } else {
            this.statusBarItem.text = '$(circle-slash) AgentOS: Disconnected';
            this.statusBarItem.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
            this.statusBarItem.color = '#F44336';
            this.statusBarItem.tooltip = 'AgentOS Edge Daemon - Disconnected';
        }
        this.statusBarItem.show();
    }

    dispose(): void {
        this.statusBarItem.dispose();
    }
}