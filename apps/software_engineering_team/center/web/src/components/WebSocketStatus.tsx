import React from 'react';
import { Tooltip } from 'antd';
import { CheckCircleOutlined, CloseCircleOutlined } from '@ant-design/icons';

interface WebSocketStatusProps {
  connected: boolean;
}

const WebSocketStatus: React.FC<WebSocketStatusProps> = ({ connected }) => {
  return (
    <Tooltip title={connected ? 'WebSocket 已连接' : 'WebSocket 未连接'}>
      <div style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
        {connected ? (
          <CheckCircleOutlined style={{ color: '#52c41a' }} />
        ) : (
          <CloseCircleOutlined style={{ color: '#ff4d4f' }} />
        )}
        <span style={{ fontSize: 12, color: '#888' }}>
          {connected ? '已连接' : '未连接'}
        </span>
      </div>
    </Tooltip>
  );
};

export default WebSocketStatus;