import React from 'react';
import { Card, Row, Col, Statistic, Progress, Table, Tag, Button, Space, Spin } from 'antd';
import { SyncOutlined, DesktopOutlined, CloudServerOutlined } from '@ant-design/icons';
import { useSettingsStore } from '@/stores';
import { api } from '@/api';

interface HealthResult {
  agentOS: { healthy: boolean; message: string };
  temporal: { healthy: boolean; message: string };
  llm: { healthy: boolean; message: string };
  overall: boolean;
}

interface ResourceData {
  cpuPercent: number;
  memoryUsedMB: number;
  memoryTotalMB: number;
  diskUsedGB: number;
  diskTotalGB: number;
}

const Monitor: React.FC = () => {
  const { agentOS } = useSettingsStore();
  const [resources, setResources] = React.useState<ResourceData | null>(null);
  const [activeTasks, setActiveTasks] = React.useState<any[]>([]);
  const [health, setHealth] = React.useState<HealthResult | null>(null);
  const [loading, setLoading] = React.useState(true);

  const fetchAll = async () => {
    setLoading(true);
    try {
      const results = await Promise.allSettled([
        api.get<ResourceData>('system/resources'),
        api.get<{ activeTasks: any[] }>('system/active-tasks'),
        api.get<HealthResult>('system/health'),
      ]);
      if (results[0].status === 'fulfilled') setResources(results[0].value);
      if (results[1].status === 'fulfilled') setActiveTasks(results[1].value.activeTasks || []);
      if (results[2].status === 'fulfilled') setHealth(results[2].value);
    } catch {}
    setLoading(false);
  };

  React.useEffect(() => {
    fetchAll();
    const interval = setInterval(fetchAll, 30000);
    return () => clearInterval(interval);
  }, []);

  const taskColumns = [
    { title: '管线', dataIndex: 'pipeline', key: 'pipeline' },
    { title: '阶段', dataIndex: 'stage', key: 'stage' },
    { title: '状态', dataIndex: 'status', key: 'status' },
    { title: '开始时间', dataIndex: 'startedAt', key: 'startedAt', render: (date: string) => date ? new Date(date).toLocaleString() : '-' },
  ];

  const resourceData = resources || { cpuPercent: 0, memoryUsedMB: 0, memoryTotalMB: 0, diskUsedGB: 0, diskTotalGB: 0 };

  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col span={12}>
          <Card title={<Space><DesktopOutlined />Agent OS 状态</Space>}
            extra={<Tag color={agentOS.grpcAddress ? 'success' : 'error'}>{agentOS.grpcAddress ? '已配置' : '未配置'}</Tag>}>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>
              <Statistic title="gRPC 地址" value={agentOS.grpcAddress || '未配置'} valueStyle={{ fontSize: 14 }} />
              <Statistic title="超时" value={`${agentOS.grpcTimeout}s`} />
            </div>
          </Card>
        </Col>
        <Col span={12}>
          <Card title={<Space><CloudServerOutlined />系统健康</Space>}
            extra={<Tag color={health?.overall ? 'success' : 'error'}>{health?.overall ? '正常' : '异常'}</Tag>}>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>
              <Statistic title="后端服务" value={health?.agentOS?.healthy ? '正常' : '异常'} valueStyle={{ color: health?.agentOS?.healthy ? '#52c41a' : '#ff4d4f' }} />
              <Statistic title="LLM 服务" value={health?.llm?.healthy ? '正常' : '未配置'} valueStyle={{ color: health?.llm?.healthy ? '#52c41a' : '#faad14' }} />
            </div>
          </Card>
        </Col>
        <Col span={24}>
          <Card title="资源使用">
            <Row gutter={24}>
              <Col span={8}>
                <Statistic title="CPU 使用率" value={resourceData.cpuPercent} suffix="%" />
                <Progress percent={resourceData.cpuPercent} showInfo={false} />
              </Col>
              <Col span={8}>
                <Statistic title="内存使用" value={`${resourceData.memoryUsedMB} / ${resourceData.memoryTotalMB} MB`} />
                <Progress percent={resourceData.memoryTotalMB > 0 ? Math.round((resourceData.memoryUsedMB / resourceData.memoryTotalMB) * 100) : 0} showInfo={false} />
              </Col>
              <Col span={8}>
                <Statistic title="磁盘使用" value={`${resourceData.diskUsedGB} / ${resourceData.diskTotalGB} GB`} />
                <Progress percent={resourceData.diskTotalGB > 0 ? Math.round((resourceData.diskUsedGB / resourceData.diskTotalGB) * 100) : 0} showInfo={false} />
              </Col>
            </Row>
          </Card>
        </Col>
        <Col span={24}>
          <Card title="活跃任务" extra={<Button icon={<SyncOutlined />} onClick={fetchAll}>刷新</Button>}>
            <Table columns={taskColumns} dataSource={activeTasks} rowKey="taskId" pagination={false} locale={{ emptyText: '暂无活跃任务' }} />
          </Card>
        </Col>
      </Row>
    </div>
  );
};

export default Monitor;