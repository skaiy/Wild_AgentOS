import React, { useMemo } from 'react';
import { renderMermaidSVG } from 'beautiful-mermaid';
import { Alert } from 'antd';

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
      <div style={{ margin: '8px 0' }}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            padding: '4px 12px',
            background: '#1e1e1e',
            borderTopLeftRadius: 6,
            borderTopRightRadius: 6,
          }}
        >
          <span style={{ color: '#999', fontSize: 12 }}>Mermaid</span>
        </div>
        <div style={{ padding: 12, background: '#1e1e1e', borderBottomLeftRadius: 6, borderBottomRightRadius: 6 }}>
          <Alert
            type="error"
            message="Mermaid 渲染失败"
            description={result.error}
            showIcon
          />
          <pre style={{ color: '#d4d4d4', marginTop: 8, fontSize: 13 }}>{code}</pre>
        </div>
      </div>
    );
  }

  return (
    <div style={{ margin: '8px 0' }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          padding: '4px 12px',
          background: '#1e1e1e',
          borderTopLeftRadius: 6,
          borderTopRightRadius: 6,
        }}
      >
        <span style={{ color: '#999', fontSize: 12 }}>Mermaid</span>
      </div>
      <div
        style={{
          padding: 16,
          background: theme === 'dark' ? '#1e1e1e' : '#ffffff',
          borderBottomLeftRadius: 6,
          borderBottomRightRadius: 6,
          overflow: 'auto',
        }}
        dangerouslySetInnerHTML={{ __html: result.svg! }}
      />
    </div>
  );
};

export default MermaidRenderer;