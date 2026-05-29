import React from 'react';
import { Card, Row, Col, Statistic, Progress, Table, Tag, Button, Space, Spin } from 'antd';
import {
  CheckCircleOutlined,
  CloseCircleOutlined,
  SyncOutlined,
  CloudServerOutlined,
  DesktopOutlined,
} from '@ant-design/icons';
import { useMonitorStore } from '@/stores';
import type { HealthCheckResult } from '@/types';
import styles from './Monitor.module.css';

const Monitor: React.FC = () => {
  const {
    agentOSStatus,
    temporalStatus,
    resourceUsage,
    activeTasks,
    loading,
    fetchAll,
    healthCheck,
  } = useMonitorStore();

  const [healthResult, setHealthResult] = React.useState<HealthCheckResult | null>(null);

  React.useEffect(() => {
    fetchAll();
    const interval = setInterval(fetchAll, 30000);
    return () => clearInterval(interval);
  }, [fetchAll]);

  const handleHealthCheck = async () => {
    const result = await healthCheck();
    setHealthResult(result);
  };

  if (loading && !agentOSStatus) {
    return <Spin />;
  }

  const taskColumns = [
    { title: '管线', dataIndex: 'pipeline', key: 'pipeline' },
    { title: '阶段', dataIndex: 'stage', key: 'stage' },
    { title: '状态', dataIndex: 'status', key: 'status' },
    {
      title: '开始时间',
      dataIndex: 'startedAt',
      key: 'startedAt',
      render: (date: string) => new Date(date).toLocaleString(),
    },
  ];

  return (
    <div className={styles.container}>
      <Row gutter={[16, 16]}>
        <Col span={12}>
          <Card
            title={
              <Space>
                <DesktopOutlined />
                Agent OS 状态
              </Space>
            }
            extra={
              agentOSStatus?.running ? (
                <Tag color="success">运行中</Tag>
              ) : (
                <Tag color="error">已停止</Tag>
              )
            }
          >
            {agentOSStatus && (
              <div className={styles.statusGrid}>
                <Statistic title="版本" value={agentOSStatus.version || '-'} />
                <Statistic
                  title="gRPC 连接"
                  value={agentOSStatus.grpcConnected ? '已连接' : '未连接'}
                  valueStyle={{ color: agentOSStatus.grpcConnected ? '#52c41a' : '#ff4d4f' }}
                />
                <Statistic title="运行时长" value={`${Math.floor(agentOSStatus.uptime / 60)} 分钟`} />
                <Statistic title="任务数" value={agentOSStatus.taskCount} />
              </div>
            )}
          </Card>
        </Col>

        <Col span={12}>
          <Card
            title={
              <Space>
                <CloudServerOutlined />
                Temporal 状态
              </Space>
            }
            extra={
              temporalStatus?.connected ? (
                <Tag color="success">已连接</Tag>
              ) : (
                <Tag color="error">未连接</Tag>
              )
            }
          >
            {temporalStatus && (
              <div className={styles.statusGrid}>
                <Statistic title="命名空间" value={temporalStatus.namespace || '-'} />
                <Statistic title="Worker 数量" value={temporalStatus.workerCount} />
                <Statistic title="任务队列" value={temporalStatus.taskQueue || '-'} />
                <Statistic title="待处理工作流" value={temporalStatus.pendingWorkflows} />
              </div>
            )}
          </Card>
        </Col>

        <Col span={24}>
          <Card title="资源使用">
            {resourceUsage && (
              <Row gutter={24}>
                <Col span={8}>
                  <Statistic
                    title="CPU 使用率"
                    value={resourceUsage.cpuPercent}
                    suffix="%"
                  />
                  <Progress percent={resourceUsage.cpuPercent} showInfo={false} />
                </Col>
                <Col span={8}>
                  <Statistic
                    title="内存使用"
                    value={`${resourceUsage.memoryUsedMB} / ${resourceUsage.memoryTotalMB} MB`}
                  />
                  <Progress
                    percent={Math.round((resourceUsage.memoryUsedMB / resourceUsage.memoryTotalMB) * 100)}
                    showInfo={false}
                  />
                </Col>
                <Col span={8}>
                  <Statistic
                    title="磁盘使用"
                    value={`${resourceUsage.diskUsedGB} / ${resourceUsage.diskTotalGB} GB`}
                  />
                  <Progress
                    percent={Math.round((resourceUsage.diskUsedGB / resourceUsage.diskTotalGB) * 100)}
                    showInfo={false}
                  />
                </Col>
              </Row>
            )}
          </Card>
        </Col>

        <Col span={24}>
          <Card
            title="活跃任务"
            extra={
              <Space>
                <Button icon={<SyncOutlined />} onClick={fetchAll}>
                  刷新
                </Button>
                <Button type="primary" onClick={handleHealthCheck}>
                  健康检查
                </Button>
              </Space>
            }
          >
            <Table
              columns={taskColumns}
              dataSource={activeTasks}
              rowKey="taskId"
              pagination={false}
              locale={{ emptyText: '暂无活跃任务' }}
            />
          </Card>
        </Col>
      </Row>
    </div>
  );
};

export default Monitor;
