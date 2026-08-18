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
      style={{
        background: '#fff',
        borderRight: '1px solid #f0f0f0',
        height: '100vh',
        position: 'fixed',
        left: 0,
        top: 0,
        overflow: 'auto',
      }}
    >
      <div
        style={{
          height: 64,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          borderBottom: '1px solid #f0f0f0',
        }}
      >
        <span style={{ fontSize: 18, fontWeight: 700, color: '#1890ff' }}>AgentOS Center</span>
      </div>
      <Menu
        mode="inline"
        selectedKeys={[getSelectedKey()]}
        items={menuItems}
        onClick={handleMenuClick}
        style={{ borderRight: 0 }}
      />
    </Sider>
  );
};

export default Sidebar;