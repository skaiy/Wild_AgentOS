import React, { useRef, useEffect } from 'react';
import ReactMarkdown from 'react-markdown';
import { Card, Tag, Space, Collapse, Empty } from 'antd';
import { CheckCircleOutlined, CloseCircleOutlined, LoadingOutlined, ToolOutlined } from '@ant-design/icons';
import type { ChatMessage, MessageContent, CodeContent, DiffContent, TerminalContent, ToolCallContent } from '@/types';
import { MermaidRenderer } from '@/components';

interface CodeBlockProps {
  code: string;
  language: string;
  showLineNumbers?: boolean;
}

const CodeBlock: React.FC<CodeBlockProps> = ({ code, language, showLineNumbers }) => {
  const [copied, setCopied] = React.useState(false);
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {}
  };

  return (
    <div style={{ margin: '8px 0', borderRadius: 6, overflow: 'hidden', border: '1px solid #333' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '4px 12px', background: '#1e1e1e' }}>
        <span style={{ color: '#999', fontSize: 12 }}>{language}</span>
        <button onClick={handleCopy} style={{ background: 'none', border: 'none', color: copied ? '#52c41a' : '#888', cursor: 'pointer', fontSize: 12 }}>
          {copied ? '已复制' : '复制'}
        </button>
      </div>
      <pre style={{ margin: 0, padding: 12, background: '#1e1e1e', color: '#d4d4d4', overflow: 'auto', fontSize: 13, lineHeight: 1.5 }}>
        <code>{code}</code>
      </pre>
    </div>
  );
};

const CodeDiff: React.FC<{ oldCode: string; newCode: string; language?: string }> = ({ oldCode, newCode }) => (
  <div style={{ margin: '8px 0', borderRadius: 6, overflow: 'hidden', border: '1px solid #333' }}>
    <div style={{ display: 'flex', padding: '4px 12px', background: '#252526', gap: 16 }}>
      <span style={{ color: '#888', fontSize: 12 }}>旧版本</span>
      <span style={{ color: '#888', fontSize: 12 }}>新版本</span>
    </div>
    <pre style={{ margin: 0, padding: 12, background: '#1e1e1e', fontSize: 13, overflow: 'auto' }}>
      {oldCode.split('\n').map((line, i) => (
        <div key={i} style={{ color: '#f85149' }}><span style={{ color: '#666', marginRight: 8, userSelect: 'none' }}>{i + 1}</span>{line}</div>
      ))}
      <div style={{ borderTop: '1px solid #333', margin: '4px 0' }} />
      {newCode.split('\n').map((line, i) => (
        <div key={i} style={{ color: '#3fb950' }}><span style={{ color: '#666', marginRight: 8, userSelect: 'none' }}>{i + 1}</span>{line}</div>
      ))}
    </pre>
  </div>
);

const TerminalLog: React.FC<{ logStream: string }> = ({ logStream }) => (
  <pre style={{ margin: '8px 0', padding: 12, background: '#1e1e1e', color: '#d4d4d4', borderRadius: 6, fontSize: 13, overflow: 'auto', maxHeight: 200, fontFamily: 'Consolas, Monaco, monospace' }}>
    {logStream}
  </pre>
);

interface MessageContentRendererProps {
  content: MessageContent[];
  role: 'user' | 'assistant' | 'system';
}

const MessageContentRenderer: React.FC<MessageContentRendererProps> = ({ content, role }) => {
  const renderContent = (item: MessageContent, index: number) => {
    switch (item.type) {
      case 'text':
        return (
          <div key={index}>
            <ReactMarkdown
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || '');
                  const codeString = String(children).replace(/\n$/, '');
                  if (!match) return <code style={{ background: 'rgba(0,0,0,0.1)', padding: '2px 4px', borderRadius: 3, fontSize: '0.9em' }} {...props}>{children}</code>;
                  if (match[1] === 'mermaid') return <MermaidRenderer code={codeString} theme="dark" />;
                  return <CodeBlock code={codeString} language={match[1]} showLineNumbers={codeString.split('\n').length > 3} />;
                },
                pre({ children }) { return <>{children}</>; },
                p({ children }) { return <p style={{ margin: '4px 0' }}>{children}</p>; },
                ul({ children }) { return <ul style={{ paddingLeft: 20 }}>{children}</ul>; },
                ol({ children }) { return <ol style={{ paddingLeft: 20 }}>{children}</ol>; },
                li({ children }) { return <li style={{ margin: '2px 0' }}>{children}</li>; },
                h1({ children }) { return <h1 style={{ fontSize: 20, margin: '12px 0 8px' }}>{children}</h1>; },
                h2({ children }) { return <h2 style={{ fontSize: 18, margin: '10px 0 6px' }}>{children}</h2>; },
                h3({ children }) { return <h3 style={{ fontSize: 16, margin: '8px 0 4px' }}>{children}</h3>; },
                blockquote({ children }) { return <blockquote style={{ borderLeft: '3px solid #1890ff', paddingLeft: 12, margin: '8px 0', color: '#888' }}>{children}</blockquote>; },
                table({ children }) { return <div style={{ overflow: 'auto' }}><table style={{ borderCollapse: 'collapse', width: '100%' }}>{children}</table></div>; },
              }}
            >
              {item.data as string}
            </ReactMarkdown>
          </div>
        );
      case 'code': {
        const codeData = item.data as CodeContent;
        if (codeData.language === 'mermaid') return <MermaidRenderer key={index} code={codeData.code} theme="dark" />;
        return <CodeBlock key={index} code={codeData.code} language={codeData.language} showLineNumbers />;
      }
      case 'diff': {
        const diffData = item.data as DiffContent;
        return <CodeDiff key={index} oldCode={diffData.oldCode} newCode={diffData.newCode} language={diffData.language} />;
      }
      case 'terminal': {
        const terminalData = item.data as TerminalContent;
        return <TerminalLog key={index} logStream={terminalData.log} />;
      }
      case 'tool_call': {
        const toolData = item.data as ToolCallContent;
        return (
          <Card key={index} size="small" style={{ margin: '8px 0' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <ToolOutlined />
              <span style={{ fontWeight: 500 }}>{toolData.toolName}</span>
              {toolData.status === 'pending' && <LoadingOutlined spin />}
              {toolData.status === 'success' && <CheckCircleOutlined style={{ color: '#52c41a' }} />}
              {toolData.status === 'error' && <CloseCircleOutlined style={{ color: '#ff4d4f' }} />}
            </div>
            <Collapse ghost items={[
              { key: 'args', label: '参数', children: <pre style={{ fontSize: 12 }}>{JSON.stringify(toolData.arguments, null, 2)}</pre> },
              ...(toolData.result ? [{ key: 'result', label: '结果', children: <pre style={{ fontSize: 12 }}>{JSON.stringify(toolData.result, null, 2)}</pre> }] : []),
            ]} />
          </Card>
        );
      }
      default:
        return null;
    }
  };

  if (!content || content.length === 0) return <Empty description="暂无内容" image={Empty.PRESENTED_IMAGE_SIMPLE} />;
  return <div>{content.map((item, index) => renderContent(item, index))}</div>;
};

const ChatPage: React.FC = () => {
  const [messages, setMessages] = React.useState<ChatMessage[]>([]);
  const [streaming, setStreaming] = React.useState(false);
  const [inputValue, setInputValue] = React.useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<any>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const sendMessage = async () => {
    if (!inputValue.trim() || streaming) return;
    const text = inputValue.trim();
    setInputValue('');

    const userMsg: ChatMessage = {
      id: `msg-${Date.now()}`,
      role: 'user',
      content: [{ type: 'text', data: text }],
      createdAt: new Date().toISOString(),
    };

    const assistantId = `msg-${Date.now() + 1}`;
    const assistantMsg: ChatMessage = {
      id: assistantId,
      role: 'assistant',
      content: [{ type: 'text', data: '' }],
      createdAt: new Date().toISOString(),
    };

    setMessages((prev) => [...prev, userMsg, assistantMsg]);
    setStreaming(true);

    try {
      const response = await fetch('/api/v1/chat/sync', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          messages: [{ role: 'user', content: text }],
        }),
      });
      const data = await response.json();
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId ? { ...m, content: [{ type: 'text', data: data.content }] } : m
        )
      );
    } catch (error) {
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId ? { ...m, content: [{ type: 'text', data: `请求失败: ${(error as Error).message}` }] } : m
        )
      );
    } finally {
      setStreaming(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  const renderMessage = (msg: ChatMessage) => {
    const isUser = msg.role === 'user';
    const isSystem = msg.role === 'system';

    return (
      <div key={msg.id} style={{ display: 'flex', gap: 12, marginBottom: 16, flexDirection: isUser ? 'row-reverse' : 'row' }}>
        <div style={{ width: 32, height: 32, borderRadius: '50%', background: isUser ? '#1890ff' : '#52c41a', display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#fff', fontSize: 14, flexShrink: 0 }}>
          {isUser ? 'U' : isSystem ? 'S' : 'A'}
        </div>
        <div style={{ maxWidth: '80%' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
            <span style={{ fontWeight: 600, fontSize: 13 }}>{isUser ? '我' : isSystem ? '系统' : 'Agent'}</span>
            <span style={{ fontSize: 11, color: '#bbb' }}>{new Date(msg.createdAt).toLocaleTimeString()}</span>
          </div>
          <div style={{ background: isUser ? '#e6f7ff' : '#f5f5f5', padding: '8px 12px', borderRadius: 8, fontSize: 14, lineHeight: 1.6 }}>
            <MessageContentRenderer content={msg.content} role={msg.role} />
          </div>
        </div>
      </div>
    );
  };

  const clearMessages = () => setMessages([]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: 'calc(100vh - 120px)' }}>
      <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
        {messages.length === 0 ? (
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%' }}>
            <Empty description={<span>开始与 Agent 对话</span>} />
          </div>
        ) : (
          <>
            {messages.map(renderMessage)}
            <div ref={messagesEndRef} />
          </>
        )}
      </div>

      <div style={{ padding: '12px 16px', borderTop: '1px solid #f0f0f0', background: '#fff' }}>
        <div style={{ display: 'flex', gap: 8 }}>
          <textarea
            ref={inputRef as any}
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="输入消息... (Enter 发送, Shift+Enter 换行)"
            rows={1}
            disabled={streaming}
            style={{
              flex: 1, resize: 'none', padding: '8px 12px', borderRadius: 6,
              border: '1px solid #d9d9d9', fontSize: 14, outline: 'none',
              minHeight: 38, maxHeight: 120,
            }}
          />
          <button
            onClick={clearMessages}
            disabled={messages.length === 0 || streaming}
            style={{
              padding: '8px 12px', borderRadius: 6, border: '1px solid #d9d9d9',
              background: '#fff', cursor: 'pointer', fontSize: 13,
            }}
          >
            清空
          </button>
          <button
            onClick={sendMessage}
            disabled={!inputValue.trim() || streaming}
            style={{
              padding: '8px 16px', borderRadius: 6, border: 'none',
              background: !inputValue.trim() || streaming ? '#d9d9d9' : '#1890ff',
              color: '#fff', cursor: !inputValue.trim() || streaming ? 'not-allowed' : 'pointer',
              fontSize: 13, fontWeight: 500,
            }}
          >
            {streaming ? '发送中...' : '发送'}
          </button>
        </div>
      </div>
    </div>
  );
};

export default ChatPage;