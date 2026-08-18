import React, { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import { createRoot } from 'react-dom/client';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Highlight, themes } from 'prism-react-renderer';
import { renderMermaidSVG } from 'beautiful-mermaid';
import ReactDiffViewer, { DiffMethod } from 'react-diff-viewer-continued';
import htm from 'htm';

const html = htm.bind(React.createElement);

const VSCODE_API = typeof acquireVsCodeApi !== 'undefined' ? acquireVsCodeApi() : null;

const isDark = document.body.classList.contains('vscode-dark');
const prismTheme = isDark ? themes.vsDark : themes.vsLight;

const languageLabels = {
  go: 'Go', typescript: 'TypeScript', javascript: 'JavaScript',
  python: 'Python', rust: 'Rust', java: 'Java', cpp: 'C++', c: 'C',
  bash: 'Bash', shell: 'Shell', json: 'JSON', yaml: 'YAML',
  markdown: 'Markdown', sql: 'SQL', html: 'HTML', css: 'CSS',
};

function CodeBlock({ code, language }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [code]);
  const normalizedLanguage = (language || 'text').toLowerCase().replace(/[-_]/g, '');
  const prismLanguage = normalizedLanguage === 'dockerfile' ? 'docker' : normalizedLanguage;
  return html`<div className="code-block">
    <div className="code-block-header">
      <span className="lang">${languageLabels[language] || language || 'text'}</span>
      <button className="copy-btn" onClick=${handleCopy}>${copied ? 'Copied!' : 'Copy'}</button>
    </div>
    <${Highlight} theme=${prismTheme} code=${code} language=${prismLanguage}>
      ${({ className, style, tokens, getLineProps, getTokenProps }) => html`
        <pre className=${className} style=${{ ...style, margin: 0, padding: '12px' }}>
          ${tokens.map((line, i) => html`
            <div key=${i} ...${getLineProps({ line })} style=${{ display: 'flex' }}>
              <span className="line-number">${i + 1}</span>
              <span>${line.map((token, key) => html`<span key=${key} ...${getTokenProps({ token })} />`)}</span>
            </div>
          `)}
        </pre>
      `}
    <//>
  </div>`;
}

function MermaidBlock({ code }) {
  const [error, setError] = useState(null);
  const svgHtml = useMemo(() => {
    const colors = isDark
      ? { bg: '#1e1e1e', fg: '#d4d4d4', accent: '#7aa2f7', muted: '#565f89', line: '#3d59a1', surface: '#292e42', border: '#3d59a1' }
      : { bg: '#ffffff', fg: '#333333', accent: '#0969da', muted: '#656d76', line: '#d0d7de', surface: '#f6f8fa', border: '#d0d7de' };
    try {
      return renderMermaidSVG(code, { ...colors, transparent: false });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return null;
    }
  }, [code]);
  if (error) {
    return html`<div className="mermaid-container"><div className="mermaid-error">${error}</div></div>`;
  }
  return html`<div className="mermaid-container"><div dangerouslySetInnerHTML=${{ __html: svgHtml }} /></div>`;
}

function DiffViewer({ oldCode, newCode }) {
  const [splitView, setSplitView] = useState(true);
  const diffViewerStyles = useMemo(() => {
    if (isDark) {
      return {
        variables: {
          dark: {
            diffViewerBackground: '#1e1e1e', diffViewerColor: '#d4d4d4',
            addedBackground: 'rgba(46, 160, 67, 0.15)', addedColor: '#3fb950',
            removedBackground: 'rgba(248, 81, 73, 0.15)', removedColor: '#f85149',
            wordAddedBackground: 'rgba(46, 160, 67, 0.3)', wordRemovedBackground: 'rgba(248, 81, 73, 0.3)',
            addedGutterBackground: 'rgba(46, 160, 67, 0.2)', removedGutterBackground: 'rgba(248, 81, 73, 0.2)',
            gutterBackground: '#252526', gutterBackgroundDark: '#1e1e1e',
            highlightBackground: 'rgba(255, 255, 0, 0.1)', highlightGutterBackground: 'rgba(255, 255, 0, 0.2)',
            codeFoldGutterBackground: '#2d2d2d', codeFoldBackground: '#2d2d2d',
            emptyLineBackground: '#252526', gutterColor: '#606060',
            addedGutterColor: '#3fb950', removedGutterColor: '#f85149',
            codeFoldContentColor: '#d4d4d4', diffViewerTitleBackground: '#2d2d2d',
            diffViewerTitleColor: '#888', diffViewerTitleBorderColor: '#3d3d3d',
          },
        },
        line: { padding: '4px 8px', fontSize: '13px' },
        gutter: { minWidth: '45px', padding: '0 8px' },
      };
    } else {
      return {
        variables: {
          light: {
            diffViewerBackground: '#ffffff', diffViewerColor: '#24292f',
            addedBackground: 'rgba(46, 160, 67, 0.1)', addedColor: '#1a7f37',
            removedBackground: 'rgba(248, 81, 73, 0.1)', removedColor: '#cf222e',
            wordAddedBackground: 'rgba(46, 160, 67, 0.2)', wordRemovedBackground: 'rgba(248, 81, 73, 0.2)',
            addedGutterBackground: 'rgba(46, 160, 67, 0.15)', removedGutterBackground: 'rgba(248, 81, 73, 0.15)',
            gutterBackground: '#f6f8fa', gutterBackgroundDark: '#eee',
            highlightBackground: 'rgba(255, 255, 0, 0.15)', highlightGutterBackground: 'rgba(255, 255, 0, 0.2)',
            codeFoldGutterBackground: '#f6f8fa', codeFoldBackground: '#f6f8fa',
            emptyLineBackground: '#f6f8fa', gutterColor: '#656d76',
            addedGutterColor: '#1a7f37', removedGutterColor: '#cf222e',
            codeFoldContentColor: '#656d76', diffViewerTitleBackground: '#f6f8fa',
            diffViewerTitleColor: '#656d76', diffViewerTitleBorderColor: '#d0d7de',
          },
        },
        line: { padding: '4px 8px', fontSize: '13px' },
        gutter: { minWidth: '45px', padding: '0 8px' },
      };
    }
  }, []);
  return html`<div className="diff-container">
    <div style=${{ display: 'flex', gap: '4px', padding: '6px 12px', background: 'var(--surface)', borderBottom: '1px solid var(--border)' }}>
      <button style=${{
        background: splitView ? 'var(--primary)' : 'none',
        border: '1px solid ' + (splitView ? 'var(--primary)' : 'var(--border)'),
        color: splitView ? '#fff' : 'var(--text-muted)',
        padding: '2px 10px', borderRadius: '4px', fontSize: '12px', cursor: 'pointer',
      }} onClick=${() => setSplitView(true)}>Split</button>
      <button style=${{
        background: !splitView ? 'var(--primary)' : 'none',
        border: '1px solid ' + (!splitView ? 'var(--primary)' : 'var(--border)'),
        color: !splitView ? '#fff' : 'var(--text-muted)',
        padding: '2px 10px', borderRadius: '4px', fontSize: '12px', cursor: 'pointer',
      }} onClick=${() => setSplitView(false)}>Unified</button>
    </div>
    <${ReactDiffViewer}
      oldValue=${oldCode}
      newValue=${newCode}
      splitView=${splitView}
      showDiffOnly=${false}
      useDarkTheme=${isDark}
      styles=${diffViewerStyles}
      compareMethod=${DiffMethod.WORDS}
    />
  </div>`;
}

function parseContent(text) {
  if (!text) return [];
  const parts = [];
  const codeBlockRegex = /```(\w*)\n([\s\S]*?)```/g;
  let lastIndex = 0;
  let match;
  const blocks = [];
  while ((match = codeBlockRegex.exec(text)) !== null) {
    blocks.push({ lang: match[1], content: match[2], index: match.index, end: match.index + match[0].length });
  }
  let blockIdx = 0;
  let pos = 0;
  while (blockIdx < blocks.length) {
    const block = blocks[blockIdx];
    if (block.index > pos) {
      const textSeg = text.slice(pos, block.index);
      const mdSegments = textSeg.split(/(?=```diff\n)/);
      for (const seg of mdSegments) {
        const diffRegex = /```diff\n([\s\S]*?)```/g;
        let dm;
        let dlast = 0;
        let tempSeg = seg;
        const diffBlocks = [];
        while ((dm = diffRegex.exec(tempSeg)) !== null) {
          diffBlocks.push({ content: dm[1], index: dm.index, end: dm.index + dm[0].length });
        }
        let di = 0;
        let dp = 0;
        while (di < diffBlocks.length) {
          const db = diffBlocks[di];
          if (db.index > dp) parts.push({ type: 'text', content: tempSeg.slice(dp, db.index) });
          const lines = db.content.split('\n');
          let oldCode = '', newCode = '';
          let isOld = true;
          for (const line of lines) {
            if (line.startsWith('---')) { isOld = false; continue; }
            if (line.startsWith('+++')) continue;
            if (line.startsWith('-')) oldCode += line.slice(1) + '\n';
            else if (line.startsWith('+')) newCode += line.slice(1) + '\n';
            else { oldCode += line + '\n'; newCode += line + '\n'; }
          }
          parts.push({ type: 'diff', oldCode: oldCode.trim(), newCode: newCode.trim() });
          dp = db.end;
          di++;
        }
        if (dp < tempSeg.length) parts.push({ type: 'text', content: tempSeg.slice(dp) });
      }
    }
    if (block.lang === 'mermaid') {
      parts.push({ type: 'mermaid', content: block.content });
    } else {
      parts.push({ type: 'code', content: block.content, language: block.lang });
    }
    pos = block.end;
    blockIdx++;
  }
  if (pos < text.length) {
    parts.push({ type: 'text', content: text.slice(pos) });
  }
  return parts;
}

function MessageBubble({ role, content }) {
  if (role === 'user') {
    return html`<div className="bubble user">${content}</div>`;
  }
  const parts = useMemo(() => parseContent(content), [content]);
  return html`<div className="bubble assistant">
    ${parts.map((part, i) => {
      switch (part.type) {
        case 'code':
          return html`<${CodeBlock} key=${i} code=${part.content} language=${part.language || 'text'} />`;
        case 'mermaid':
          return html`<${MermaidBlock} key=${i} code=${part.content} />`;
        case 'diff':
          return html`<${DiffViewer} key=${i} oldCode=${part.oldCode} newCode=${part.newCode} />`;
        case 'text':
          return html`<div key=${i} className="markdown-content"><${ReactMarkdown} remarkPlugins=${[remarkGfm]} components=${{
            code({ className, children, ...props }) {
              const match = /language-(\w+)/.exec(className || '');
              const codeString = String(children).replace(/\n$/, '');
              if (!match) return html`<code style=${{
                background: 'var(--inline-code-bg)', padding: '2px 6px',
                borderRadius: '3px', fontSize: '0.9em',
                fontFamily: "'Cascadia Code','Fira Code','Consolas',monospace",
              }} ...${props}>${children}</code>`;
              return html`<${CodeBlock} code=${codeString} language=${match[1]} />`;
            },
            pre({ children }) { return html`<${React.Fragment}>${children}<//>`; },
          }}>${part.content}</${ReactMarkdown}></div>`;
        default:
          return null;
      }
    })}
  </div>`;
}

function App() {
  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState('');
  const [streaming, setStreaming] = useState(false);
  const [currentStreamText, setCurrentStreamText] = useState('');
  const chatRef = useRef(null);
  const inputRef = useRef(null);

  useEffect(() => {
    if (chatRef.current) {
      chatRef.current.scrollTop = chatRef.current.scrollHeight;
    }
  }, [messages, currentStreamText]);

  useEffect(() => {
    if (!VSCODE_API) return;
    const handler = (event) => {
      const msg = event.data;
      switch (msg.type) {
        case 'addMessage':
          setMessages(prev => [...prev, { id: Date.now(), role: msg.role, content: msg.content, time: new Date().toLocaleTimeString() }]);
          break;
        case 'streamStart':
          setStreaming(true);
          setCurrentStreamText('');
          break;
        case 'streamChunk':
          setCurrentStreamText(prev => prev + msg.chunk);
          break;
        case 'streamEnd':
          setStreaming(false);
          if (msg.fullText || currentStreamText) {
            const fullText = msg.fullText || currentStreamText;
            setMessages(prev => [...prev, { id: Date.now(), role: 'assistant', content: fullText, time: new Date().toLocaleTimeString() }]);
            setCurrentStreamText('');
          }
          break;
        case 'setMessages':
          setMessages(msg.messages || []);
          break;
        case 'clearMessages':
          setMessages([]);
          break;
      }
    };
    window.addEventListener('message', handler);
    return () => window.removeEventListener('message', handler);
  }, [currentStreamText]);

  const sendMessage = useCallback(() => {
    const text = input.trim();
    if (!text || streaming) return;
    setInput('');
    setMessages(prev => [...prev, { id: Date.now(), role: 'user', content: text, time: new Date().toLocaleTimeString() }]);
    if (VSCODE_API) {
      VSCODE_API.postMessage({ type: 'sendMessage', text });
    }
  }, [input, streaming]);

  const handleKeyDown = useCallback((e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }, [sendMessage]);

  const handleInputChange = useCallback((e) => {
    setInput(e.target.value);
    if (e.target.scrollHeight > 36) {
      e.target.style.height = 'auto';
      e.target.style.height = Math.min(e.target.scrollHeight, 120) + 'px';
    }
  }, []);

  const renderMessage = (msg) => html`
    <div key=${msg.id} className={"message " + msg.role}>
      <div className={"avatar " + msg.role}>
        ${msg.role === 'user' ? '\u{1F464}' : msg.role === 'system' ? '\u2699' : '\u{1F916}'}
      </div>
      <div>
        <div className="header-info">
          <span>${msg.role === 'user' ? 'You' : msg.role === 'system' ? 'System' : 'Agent'}</span>
          <span>${msg.time}</span>
        </div>
        <${MessageBubble} role=${msg.role} content=${msg.content} />
      </div>
    </div>
  `;

  return html`
    <div className="chat-container" ref=${chatRef}>
      ${messages.length === 0 && !streaming ? html`
        <div className="empty-state">
          <div className="icon">\u{1F916}</div>
          <div className="title">AgentOS Chat</div>
          <div className="subtitle">Start a conversation with the Agent</div>
        </div>
      ` : messages.map(renderMessage)}
      ${streaming && currentStreamText ? html`
        <div className="message assistant">
          <div className="avatar assistant">\u{1F916}</div>
          <div>
            <div className="header-info"><span>Agent</span><span className="typing"><span/><span/><span/></span></div>
            <${MessageBubble} role="assistant" content=${currentStreamText} />
          </div>
        </div>
      ` : null}
      ${streaming && !currentStreamText ? html`
        <div className="message assistant">
          <div className="avatar assistant">\u{1F916}</div>
          <div>
            <div className="header-info"><span>Agent</span></div>
            <div className="bubble assistant"><div className="typing"><span/><span/><span/></div></div>
          </div>
        </div>
      ` : null}
    </div>
    <div className="input-area">
      <textarea
        ref=${inputRef}
        value=${input}
        onChange=${handleInputChange}
        onKeyDown=${handleKeyDown}
        placeholder="Type a message... (Enter to send, Shift+Enter for new line)"
        disabled=${streaming}
        rows=${1}
      />
      <button onClick=${sendMessage} disabled=${!input.trim() || streaming}>
        ${streaming ? '\u26A1' : '\u27A4'} Send
      </button>
    </div>
  `;
}

window.__AGENTOS_CHAT_READY = true;
const root = createRoot(document.getElementById('root'));
root.render(html`<${App} />`);