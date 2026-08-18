import React, { useState } from 'react';
import { Highlight, themes } from 'prism-react-renderer';
import { Button, Tooltip } from 'antd';
import { CopyOutlined, CheckOutlined } from '@ant-design/icons';
import styles from './CodeBlock.module.css';

interface CodeBlockProps {
  code: string;
  language: string;
  showLineNumbers?: boolean;
  maxHeight?: number;
  showCopy?: boolean;
}

const languageLabels: Record<string, string> = {
  go: 'Go',
  typescript: 'TypeScript',
  javascript: 'JavaScript',
  python: 'Python',
  rust: 'Rust',
  java: 'Java',
  cpp: 'C++',
  c: 'C',
  bash: 'Bash',
  shell: 'Shell',
  json: 'JSON',
  yaml: 'YAML',
  markdown: 'Markdown',
  sql: 'SQL',
  html: 'HTML',
  css: 'CSS',
  scss: 'SCSS',
  xml: 'XML',
  docker: 'Dockerfile',
};

const CodeBlock: React.FC<CodeBlockProps> = ({
  code,
  language,
  showLineNumbers = true,
  maxHeight,
  showCopy = true,
}) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  };

  const normalizedLanguage = language.toLowerCase().replace(/[-_]/g, '');
  const prismLanguage = normalizedLanguage === 'dockerfile' ? 'docker' : normalizedLanguage;

  return (
    <div className={styles.container} style={{ maxHeight: maxHeight ? `${maxHeight}px` : undefined }}>
      <div className={styles.header}>
        <span className={styles.languageTag}>{languageLabels[language] || language}</span>
        {showCopy && (
          <Tooltip title={copied ? '已复制' : '复制代码'}>
            <Button
              type="text"
              size="small"
              icon={copied ? <CheckOutlined style={{ color: '#52c41a' }} /> : <CopyOutlined />}
              onClick={handleCopy}
              className={styles.copyButton}
            />
          </Tooltip>
        )}
      </div>
      <Highlight
        theme={themes.vsDark}
        code={code}
        language={prismLanguage as any}
      >
        {({ className, style, tokens, getLineProps, getTokenProps }) => (
          <pre className={`${className} ${styles.pre}`} style={{ ...style, margin: 0 }}>
            {tokens.map((line, i) => (
              <div key={i} {...getLineProps({ line })} className={styles.line}>
                {showLineNumbers && (
                  <span className={styles.lineNumber}>{i + 1}</span>
                )}
                <span className={styles.lineContent}>
                  {line.map((token, key) => (
                    <span key={key} {...getTokenProps({ token })} />
                  ))}
                </span>
              </div>
            ))}
          </pre>
        )}
      </Highlight>
    </div>
  );
};

export default CodeBlock;
