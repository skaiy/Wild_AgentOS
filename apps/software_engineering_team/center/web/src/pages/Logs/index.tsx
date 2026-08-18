import React from 'react';
import { Card, Tabs, Select, Input, Button, Space } from 'antd';
import { SearchOutlined, DownloadOutlined } from '@ant-design/icons';
import { logsApi } from '@/api';
import type { LogEntry, LogLevel } from '@/types';

const Logs: React.FC = () => {
  const [logs, setLogs] = React.useState<LogEntry[]>([]);
  const [loading, setLoading] = React.useState(false);
  const [level, setLevel] = React.useState<LogLevel | undefined>();
  const [keyword, setKeyword] = React.useState('');
  const [activeTab, setActiveTab] = React.useState('system');

  const fetchLogs = async () => {
    setLoading(true);
    try {
      const result = await logsApi.getSystemLogs({ level, keyword });
      setLogs(result);
    } catch (error) {
      console.error('Failed to fetch logs:', error);
    }
    setLoading(false);
  };

  React.useEffect(() => { fetchLogs(); }, [activeTab, level]);

  const handleExport = () => {
    const content = logs.map((log) => `[${log.timestamp}] [${log.level}] ${log.message}`).join('\n');
    const blob = new Blob([content], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `logs-${new Date().toISOString()}.txt`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const getLevelColor = (level: LogLevel) => {
    switch (level) {
      case 'DEBUG': return '#999';
      case 'INFO': return '#1890ff';
      case 'WARN': return '#faad14';
      case 'ERROR': return '#ff4d4f';
    }
  };

  const tabItems = [
    {
      key: 'system', label: '系统日志',
      children: (
        <div style={{ maxHeight: 'calc(100vh - 280px)', overflow: 'auto', fontFamily: 'Consolas, Monaco, monospace', fontSize: 13 }}>
          {logs.map((log) => (
            <div key={log.id} style={{ padding: '4px 8px', display: 'flex', gap: 12, borderBottom: '1px solid #f5f5f5' }}>
              <span style={{ color: '#bbb', whiteSpace: 'nowrap' }}>{log.timestamp}</span>
              <span style={{ color: getLevelColor(log.level), fontWeight: 600, whiteSpace: 'nowrap' }}>[{log.level}]</span>
              <span style={{ flex: 1 }}>{log.message}</span>
            </div>
          ))}
        </div>
      ),
    },
    {
      key: 'agent-os', label: 'Agent OS 日志',
      children: <div style={{ textAlign: 'center', padding: 40, color: '#888' }}>Agent OS 日志（开发中）</div>,
    },
  ];

  return (
    <div>
      <Card
        title="日志查看"
        extra={
          <Space>
            <Select placeholder="日志级别" allowClear style={{ width: 120 }} value={level} onChange={setLevel}
              options={[
                { value: 'DEBUG', label: 'DEBUG' },
                { value: 'INFO', label: 'INFO' },
                { value: 'WARN', label: 'WARN' },
                { value: 'ERROR', label: 'ERROR' },
              ]}
            />
            <Input placeholder="关键词搜索" prefix={<SearchOutlined />} value={keyword}
              onChange={(e) => setKeyword(e.target.value)} onPressEnter={fetchLogs} style={{ width: 200 }} />
            <Button onClick={fetchLogs}>搜索</Button>
            <Button icon={<DownloadOutlined />} onClick={handleExport}>导出</Button>
          </Space>
        }
      >
        <Tabs activeKey={activeTab} onChange={setActiveTab} items={tabItems} />
      </Card>
    </div>
  );
};

export default Logs;