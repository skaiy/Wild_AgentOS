import React, { useMemo } from 'react';
import { renderMermaidSVG } from 'beautiful-mermaid';
import { Alert, Spin } from 'antd';
import styles from './MermaidRenderer.module.css';

interface MermaidRendererProps {
  code: string;
  theme?: 'light' | 'dark';
}

const MermaidRenderer: React.FC<MermaidRendererProps> = ({ code, theme = 'dark' }) => {
  const result = useMemo(() => {
    try {
      const svg = renderMermaidSVG(code, {
        bg: theme === 'dark' ? '#1e1e1e' : '#ffffff',
        fg: theme === 'dark' ? '#d4d4d4' : '#1e1e1e',
        accent: theme === 'dark' ? '#7aa2f7' : '#0969da',
        muted: theme === 'dark' ? '#565f89' : '#6e7781',
        line: theme === 'dark' ? '#3d59a1' : '#8b949e',
        surface: theme === 'dark' ? '#292e42' : '#f6f8fa',
        border: theme === 'dark' ? '#3d59a1' : '#d0d7de',
        transparent: false,
      });
      return { svg, error: null };
    } catch (err) {
      return {
        svg: null,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }, [code, theme]);

  if (result.error) {
    return (
      <div className={styles.container}>
        <div className={styles.header}>
          <span className={styles.languageTag}>Mermaid</span>
        </div>
        <div className={styles.errorContent}>
          <Alert
            type="error"
            message="Mermaid 渲染失败"
            description={result.error}
            showIcon
          />
          <pre className={styles.fallbackCode}>{code}</pre>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <span className={styles.languageTag}>Mermaid</span>
      </div>
      <div
        className={styles.svgWrapper}
        dangerouslySetInnerHTML={{ __html: result.svg! }}
      />
    </div>
  );
};

export default MermaidRenderer;
