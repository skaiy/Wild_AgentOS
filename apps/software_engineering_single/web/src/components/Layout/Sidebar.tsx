import React from 'react';
import { Layout, Menu } from 'antd';
import {
  DashboardOutlined,
  FolderOutlined,
  MessageOutlined,
  CheckCircleOutlined,
  ApartmentOutlined,
  SettingOutlined,
  MonitorOutlined,
  FileTextOutlined,
  PartitionOutlined,
} from '@ant-design/icons';
import { useNavigate, useLocation } from 'react-router-dom';
import styles from './Sidebar.module.css';

const { Sider } = Layout;

const menuItems = [
  {
    key: '/',
    icon: <DashboardOutlined />,
    label: '仪表盘',
  },
  {
    key: '/projects',
    icon: <FolderOutlined />,
    label: '项目',
  },
  {
    key: '/pipeline-config',
    icon: <PartitionOutlined />,
    label: '管线配置',
  },
  {
    key: '/chat',
    icon: <MessageOutlined />,
    label: '对话',
  },
  {
    key: '/reviews',
    icon: <CheckCircleOutlined />,
    label: '审查',
  },
  {
    key: '/graph',
    icon: <ApartmentOutlined />,
    label: '图谱',
  },
  { type: 'divider' as const },
  {
    key: '/settings',
    icon: <SettingOutlined />,
    label: '设置',
  },
  {
    key: '/monitor',
    icon: <MonitorOutlined />,
    label: '监控',
  },
  {
    key: '/logs',
    icon: <FileTextOutlined />,
    label: '日志',
  },
];

const Sidebar: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();

  const getSelectedKey = () => {
    const path = location.pathname;
    if (path === '/') return '/';
    if (path.startsWith('/projects/')) return '/projects';
    const firstSegment = '/' + path.split('/')[1];
    return firstSegment;
  };

  const handleMenuClick = ({ key }: { key: string }) => {
    navigate(key);
  };

  return (
    <Sider
      width={200}
      className={styles.sider}
      theme="light"
    >
      <div className={styles.logo}>
        <span className={styles.logoText}>SDLC Agent</span>
      </div>
      <Menu
        mode="inline"
        selectedKeys={[getSelectedKey()]}
        items={menuItems}
        onClick={handleMenuClick}
        className={styles.menu}
      />
    </Sider>
  );
};

export default Sidebar;
