import * as vscode from 'vscode';
import { AgentClient } from './agentClient';

interface TaskItem {
    task_id: string;
    stage_type: string;
    description: string;
}

export class TaskPanel implements vscode.TreeDataProvider<TaskTreeItem> {
    private _onDidChangeTreeData: vscode.EventEmitter<TaskTreeItem | undefined | null> =
        new vscode.EventEmitter<TaskTreeItem | undefined | null>();
    readonly onDidChangeTreeData: vscode.Event<TaskTreeItem | undefined | null> =
        this._onDidChangeTreeData.event;

    private tasks: TaskItem[] = [];

    constructor(private agentClient: AgentClient) {}

    refresh(): void {
        this.fetchTasks();
    }

    private async fetchTasks(): Promise<void> {
        try {
            const apiUrl = `${this.agentClient.getDaemonUrl().replace(/\/+$/, '')}/api/tasks`;
            const response = await fetch(apiUrl, {
                method: 'GET',
                signal: AbortSignal.timeout(5000),
            });
            if (response.ok) {
                const data = await response.json();
                this.tasks = data.tasks || data || [];
            }
        } catch {
            this.tasks = [];
        }
        this._onDidChangeTreeData.fire(null);
    }

    getTreeItem(element: TaskTreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(element?: TaskTreeItem): Thenable<TaskTreeItem[]> {
        if (element) {
            return Promise.resolve([]);
        }
        return Promise.resolve(
            this.tasks.map(
                (t) =>
                    new TaskTreeItem(
                        t,
                        vscode.TreeItemCollapsibleState.None
                    )
            )
        );
    }

    async claimTask(taskId: string): Promise<void> {
        try {
            const apiUrl = `${this.agentClient.getDaemonUrl().replace(/\/+$/, '')}/api/tasks/${taskId}/claim`;
            const response = await fetch(apiUrl, {
                method: 'POST',
                signal: AbortSignal.timeout(5000),
            });
            if (response.ok) {
                vscode.window.showInformationMessage(`Task ${taskId} claimed successfully`);
                this.refresh();
            } else {
                vscode.window.showErrorMessage(`Failed to claim task ${taskId}: ${response.statusText}`);
            }
        } catch (err: any) {
            vscode.window.showErrorMessage(`Failed to claim task ${taskId}: ${err.message}`);
        }
    }
}

class TaskTreeItem extends vscode.TreeItem {
    constructor(
        public readonly task: TaskItem,
        collapsibleState: vscode.TreeItemCollapsibleState
    ) {
        super(`${task.task_id} [${task.stage_type}]`, collapsibleState);
        this.tooltip = `${task.task_id}\nStage: ${task.stage_type}\n${task.description || ''}`;
        this.description = task.description || '';
        this.contextValue = 'taskItem';
        this.command = {
            command: 'agentos.claimTask',
            title: 'Claim Task',
            arguments: [task.task_id],
        };
    }
}