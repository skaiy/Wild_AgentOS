import React from 'react';
import { ConfigProvider, theme, Layout, App } from 'antd';
import Sidebar from './Sidebar';
import Header from './Header';
import styles from './AppLayout.module.css';

const { Content } = Layout;

interface AppLayoutProps {
  children: React.ReactNode;
}

const AppLayout: React.FC<AppLayoutProps> = ({ children }) => {
  const [isDark, setIsDark] = React.useState(false);

  const toggleTheme = () => {
    setIsDark(!isDark);
  };

  return (
    <ConfigProvider
      theme={{
        algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: {
          colorPrimary: '#1890ff',
          borderRadius: 6,
        },
      }}
    >
      <App>
        <Layout className={styles.layout}>
          <Sidebar />
          <Layout>
            <Header isDark={isDark} onToggleTheme={toggleTheme} />
            <Content className={styles.content}>{children}</Content>
          </Layout>
        </Layout>
      </App>
    </ConfigProvider>
  );
};

export default AppLayout;
