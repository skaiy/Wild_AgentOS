import React from 'react';
import ReactMarkdown from 'react-markdown';
import { Card, Tag, Space, Collapse, Empty } from 'antd';
import {
  CheckCircleOutlined,
  CloseCircleOutlined,
  LoadingOutlined,
  ToolOutlined,
} from '@ant-design/icons';
import type { ChatMessage, MessageContent, CodeContent, DiffContent, TerminalContent, ToolCallContent } from '@/types';
import CodeBlock from './CodeBlock';
import CodeDiff from './CodeDiff';
import TerminalLog from './TerminalLog';
import { MermaidRenderer } from '@/components';
import styles from './MessageContentRenderer.module.css';

interface MessageContentRendererProps {
  content: MessageContent[];
  role: 'user' | 'assistant' | 'system';
}

const MessageContentRenderer: React.FC<MessageContentRendererProps> = ({ content, role }) => {
  const renderContent = (item: MessageContent, index: number) => {
    switch (item.type) {
      case 'text':
        return (
          <div key={index} className={styles.textContent}>
            <ReactMarkdown
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || '');
                  const codeString = String(children).replace(/\n$/, '');
                  
                  if (!match) {
                    return <code className={styles.inlineCode} {...props}>{children}</code>;
                  }

                  if (match[1] === 'mermaid') {
                    return <MermaidRenderer code={codeString} theme="dark" />;
                  }
                  
                  return (
                    <CodeBlock
                      code={codeString}
                      language={match[1]}
                      showLineNumbers={codeString.split('\n').length > 3}
                    />
                  );
                },
                pre({ children }) {
                  return <>{children}</>;
                },
                p({ children }) {
                  return <p className={styles.paragraph}>{children}</p>;
                },
                ul({ children }) {
                  return <ul className={styles.list}>{children}</ul>;
                },
                ol({ children }) {
                  return <ol className={styles.orderedList}>{children}</ol>;
                },
                li({ children }) {
                  return <li className={styles.listItem}>{children}</li>;
                },
                h1({ children }) {
                  return <h1 className={styles.heading}>{children}</h1>;
                },
                h2({ children }) {
                  return <h2 className={styles.heading}>{children}</h2>;
                },
                h3({ children }) {
                  return <h3 className={styles.heading}>{children}</h3>;
                },
                blockquote({ children }) {
                  return <blockquote className={styles.blockquote}>{children}</blockquote>;
                },
                table({ children }) {
                  return <div className={styles.tableWrapper}><table className={styles.table}>{children}</table></div>;
                },
              }}
            >
              {item.data as string}
            </ReactMarkdown>
          </div>
        );

      case 'code':
        const codeData = item.data as CodeContent;
        if (codeData.language === 'mermaid') {
          return (
            <MermaidRenderer
              key={index}
              code={codeData.code}
              theme="dark"
            />
          );
        }
        return (
          <CodeBlock
            key={index}
            code={codeData.code}
            language={codeData.language}
            showLineNumbers={true}
          />
        );

      case 'diff':
        const diffData = item.data as DiffContent;
        return (
          <CodeDiff
            key={index}
            oldCode={diffData.oldCode}
            newCode={diffData.newCode}
            language={diffData.language}
          />
        );

      case 'terminal':
        const terminalData = item.data as TerminalContent;
        return (
          <TerminalLog
            key={index}
            logStream={terminalData.log}
            height={200}
          />
        );

      case 'tool_call':
        const toolData = item.data as ToolCallContent;
        return (
          <Card key={index} size="small" className={styles.toolCallCard}>
            <div className={styles.toolCallHeader}>
              <ToolOutlined />
              <span className={styles.toolName}>{toolData.toolName}</span>
              {toolData.status === 'pending' && <LoadingOutlined spin />}
              {toolData.status === 'success' && <CheckCircleOutlined style={{ color: '#52c41a' }} />}
              {toolData.status === 'error' && <CloseCircleOutlined style={{ color: '#ff4d4f' }} />}
            </div>
            <Collapse
              ghost
              items={[
                {
                  key: 'args',
                  label: '参数',
                  children: (
                    <pre className={styles.toolCallPre}>
                      {JSON.stringify(toolData.arguments, null, 2)}
                    </pre>
                  ),
                },
                ...(toolData.result
                  ? [{
                      key: 'result',
                      label: '结果',
                      children: (
                        <pre className={styles.toolCallPre}>
                          {JSON.stringify(toolData.result, null, 2)}
                        </pre>
                      ),
                    }]
                  : []),
              ]}
            />
          </Card>
        );

      default:
        return null;
    }
  };

  if (!content || content.length === 0) {
    return <Empty description="暂无内容" image={Empty.PRESENTED_IMAGE_SIMPLE} />;
  }

  return (
    <div className={styles.container}>
      {content.map((item, index) => renderContent(item, index))}
    </div>
  );
};

export default MessageContentRenderer;
