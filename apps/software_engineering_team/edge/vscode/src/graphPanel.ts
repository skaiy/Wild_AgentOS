import * as vscode from 'vscode';

export class GraphPanel {
    private panel: vscode.WebviewPanel | undefined;

    constructor(private context: vscode.ExtensionContext) {}

    show(): void {
        const column = vscode.ViewColumn.Beside;
        if (this.panel) {
            this.panel.reveal(column);
            return;
        }

        this.panel = vscode.window.createWebviewPanel(
            'agentosGraph',
            'AgentOS Graph',
            column,
            {
                enableScripts: true,
                retainContextWhenHidden: true,
            }
        );

        this.panel.webview.html = this.getHtml();

        this.renderGraph(`graph TD
    A[iri://system/agent_os] --> B[iri://task/init]
    A --> C[iri://memory/l2]
    B --> D[iri://skill/registry]
    C --> E[iri://projection/l3]
    D --> E`);

        this.panel.onDidDispose(() => {
            this.panel = undefined;
        });
    }

    renderGraph(mermaidCode: string): void {
        if (this.panel) {
            this.panel.webview.postMessage({
                command: 'renderGraph',
                mermaidCode,
            });
        }
    }

    private getHtml(): string {
        return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>AgentOS Graph</title>
<script src="https://cdn.jsdelivr.net/npm/mermaid/dist/mermaid.min.js"></script>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { background: #1e1e1e; color: #d4d4d4; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; height: 100vh; display: flex; flex-direction: column; }
#toolbar { padding: 8px 12px; background: #252526; border-bottom: 1px solid #3c3c3c; display: flex; gap: 8px; align-items: center; }
#toolbar button { padding: 4px 12px; background: #0e639c; color: #fff; border: none; border-radius: 4px; cursor: pointer; font-size: 12px; }
#toolbar button:hover { background: #1177bb; }
#mermaid-container { flex: 1; overflow: auto; padding: 16px; display: flex; justify-content: center; align-items: flex-start; }
#mermaid-container svg { max-width: 100%; }
</style>
</head>
<body>
<div id="toolbar">
<button id="zoom-in">Zoom In</button>
<button id="zoom-out">Zoom Out</button>
<button id="reset-zoom">Reset</button>
</div>
<div id="mermaid-container">
<div class="mermaid" id="graphContainer"></div>
</div>
<script>
mermaid.initialize({
    startOnLoad: true,
    theme: 'dark',
    themeVariables: {
        primaryColor: '#0e639c',
        primaryTextColor: '#fff',
        primaryBorderColor: '#1177bb',
        lineColor: '#4CAF50',
        secondaryColor: '#2d2d2d',
        tertiaryColor: '#252526',
    },
});

let currentCode = '';

function renderMermaid(code) {
    currentCode = code;
    const container = document.getElementById('graphContainer');
    container.innerHTML = code;
    try {
        mermaid.run({ nodes: [container] });
    } catch (e) {
        container.innerHTML = '<pre style="color:#f48771;">Failed to render graph: ' + e.message + '</pre>';
    }
}

const vscode = acquireVsCodeApi();

window.addEventListener('message', function(event) {
    const msg = event.data;
    if (msg.command === 'renderGraph') {
        renderMermaid(msg.mermaidCode);
    }
});

document.getElementById('zoom-in').addEventListener('click', function() {
    const svg = document.querySelector('#mermaid-container svg');
    if (svg) {
        const w = parseFloat(svg.getAttribute('width') || svg.clientWidth);
        const h = parseFloat(svg.getAttribute('height') || svg.clientHeight);
        svg.setAttribute('width', (w * 1.2) + 'px');
        svg.setAttribute('height', (h * 1.2) + 'px');
    }
});

document.getElementById('zoom-out').addEventListener('click', function() {
    const svg = document.querySelector('#mermaid-container svg');
    if (svg) {
        const w = parseFloat(svg.getAttribute('width') || svg.clientWidth);
        const h = parseFloat(svg.getAttribute('height') || svg.clientHeight);
        svg.setAttribute('width', Math.max(w / 1.2, 100) + 'px');
        svg.setAttribute('height', Math.max(h / 1.2, 100) + 'px');
    }
});

document.getElementById('reset-zoom').addEventListener('click', function() {
    if (currentCode) {
        renderMermaid(currentCode);
    }
});
</script>
</body>
</html>`;
    }

    dispose(): void {
        if (this.panel) {
            this.panel.dispose();
            this.panel = undefined;
        }
    }
}