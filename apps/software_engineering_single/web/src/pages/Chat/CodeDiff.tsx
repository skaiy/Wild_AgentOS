import React, { useState } from 'react';
import ReactDiffViewer, { DiffMethod } from 'react-diff-viewer-continued';
import { Button, Space, Segmented } from 'antd';
import { SplitCellsOutlined, OrderedListOutlined } from '@ant-design/icons';
import styles from './CodeDiff.module.css';

interface CodeDiffProps {
  oldCode: string;
  newCode: string;
  language?: string;
  splitView?: boolean;
  leftTitle?: string;
  rightTitle?: string;
  showLineNumbers?: boolean;
}

const CodeDiff: React.FC<CodeDiffProps> = ({
  oldCode,
  newCode,
  language = 'text',
  splitView: initialSplitView = true,
  leftTitle = '旧版本',
  rightTitle = '新版本',
  showLineNumbers = true,
}) => {
  const [splitView, setSplitView] = useState(initialSplitView);

  const darkTheme = {
    variables: {
      dark: {
        diffViewerBackground: '#1e1e1e',
        diffViewerColor: '#d4d4d4',
        addedBackground: 'rgba(46, 160, 67, 0.15)',
        addedColor: '#3fb950',
        removedBackground: 'rgba(248, 81, 73, 0.15)',
        removedColor: '#f85149',
        wordAddedBackground: 'rgba(46, 160, 67, 0.3)',
        wordRemovedBackground: 'rgba(248, 81, 73, 0.3)',
        addedGutterBackground: 'rgba(46, 160, 67, 0.2)',
        removedGutterBackground: 'rgba(248, 81, 73, 0.2)',
        gutterBackground: '#252526',
        gutterBackgroundDark: '#1e1e1e',
        highlightBackground: 'rgba(255, 255, 0, 0.1)',
        highlightGutterBackground: 'rgba(255, 255, 0, 0.2)',
        codeFoldGutterBackground: '#2d2d2d',
        codeFoldBackground: '#2d2d2d',
        emptyLineBackground: '#252526',
        gutterColor: '#606060',
        addedGutterColor: '#3fb950',
        removedGutterColor: '#f85149',
        codeFoldContentColor: '#d4d4d4',
        diffViewerTitleBackground: '#2d2d2d',
        diffViewerTitleColor: '#888',
        diffViewerTitleBorderColor: '#3d3d3d',
      },
    },
    line: {
      padding: '4px 8px',
      fontSize: '13px',
    },
    gutter: {
      minWidth: '45px',
      padding: '0 8px',
    },
  };

  return (
    <div className={styles.container}>
      <div className={styles.toolbar}>
        <Space>
          <Segmented
            value={splitView ? 'split' : 'unified'}
            onChange={(value) => setSplitView(value === 'split')}
            options={[
              { value: 'split', label: <><SplitCellsOutlined /> 并排对比</> },
              { value: 'unified', label: <><OrderedListOutlined /> 合并视图</> },
            ]}
          />
        </Space>
      </div>
      <div className={styles.diffContainer}>
        <ReactDiffViewer
          oldValue={oldCode}
          newValue={newCode}
          splitView={splitView}
          leftTitle={leftTitle}
          rightTitle={rightTitle}
          showDiffOnly={false}
          useDarkTheme={true}
          styles={darkTheme}
          compareMethod={DiffMethod.WORDS}
        />
      </div>
    </div>
  );
};

export default CodeDiff;
