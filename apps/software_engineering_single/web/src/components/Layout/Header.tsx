import React from 'react';
import { Layout, Button, Space, Badge, Typography } from 'antd';
import {
  BulbOutlined,
  BulbFilled,
  WifiOutlined,
  UserOutlined,
} from '@ant-design/icons';
import { useWebSocketStore } from '@/stores';
import styles from './Header.module.css';

const { Header: AntHeader } = Layout;
const { Text } = Typography;

interface HeaderProps {
  isDark: boolean;
  onToggleTheme: () => void;
}

const Header: React.FC<HeaderProps> = ({ isDark, onToggleTheme }) => {
  const { connected } = useWebSocketStore();

  return (
    <AntHeader className={styles.header}>
      <div className={styles.left}>
        <Text strong style={{ fontSize: 16 }}>
          软件工程智能体应用
        </Text>
      </div>
      <div className={styles.right}>
        <Space size="middle">
          <Badge
            status={connected ? 'success' : 'error'}
            text={
              <Space size={4}>
                <WifiOutlined style={{ color: connected ? '#52c41a' : '#ff4d4f' }} />
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {connected ? '已连接' : '未连接'}
                </Text>
              </Space>
            }
          />
          <Button
            type="text"
            icon={isDark ? <BulbFilled /> : <BulbOutlined />}
            onClick={onToggleTheme}
            title={isDark ? '切换到亮色模式' : '切换到暗色模式'}
          />
          <Button type="text" icon={<UserOutlined />} title="用户" />
        </Space>
      </div>
    </AntHeader>
  );
};

export default Header;
