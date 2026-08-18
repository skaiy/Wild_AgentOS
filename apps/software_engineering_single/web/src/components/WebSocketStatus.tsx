import React from 'react';
import { Badge, Tooltip } from 'antd';
import { CheckCircleOutlined, CloseCircleOutlined, LoadingOutlined } from '@ant-design/icons';
import styles from './WebSocketStatus.module.css';

interface WebSocketStatusProps {
  connected: boolean;
}

const WebSocketStatus: React.FC<WebSocketStatusProps> = ({ connected }) => {
  return (
    <Tooltip title={connected ? 'WebSocket 已连接' : 'WebSocket 未连接'}>
      <div className={styles.container}>
        {connected ? (
          <CheckCircleOutlined className={styles.connected} />
        ) : (
          <CloseCircleOutlined className={styles.disconnected} />
        )}
        <span className={styles.text}>
          {connected ? '已连接' : '未连接'}
        </span>
      </div>
    </Tooltip>
  );
};

export default WebSocketStatus;
