import React from 'react';
import { Card, Row, Col, Button, Tag, Tooltip, Empty, Spin, Progress } from 'antd';
import {
  ProjectOutlined,
  ThunderboltOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  AuditOutlined,
  RightOutlined,
  ApiOutlined,
  CloudServerOutlined,
  RobotOutlined,
  PlusOutlined,
  RocketOutlined,
  BugOutlined,
  DashboardOutlined,
  ClockCircleOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useProjectStore, useSettingsStore } from '@/stores';
import { api } from '@/api';
import type { ProjectMeta } from '@/types';

interface StatsData {
  projectCount: number;
  taskCount: number;
  runningTasks: number;
  completedTasks: number;
  failedTasks: number;
  pendingReviews: number;
}

interface ActivityItem {
  type: string;
  taskId: string;
  projectId: string;
  pipeline: string;
  status: string;
  stage: string;
  startedAt: string;
  completedAt: string;
  error?: string;
}

interface HealthInfo {
  agentOS: { healthy: boolean; message: string };
  temporal: { healthy: boolean; message: string };
  llm: { healthy: boolean; message: string };
  overall: boolean;
}

const Dashboard: React.FC = () => {
  const navigate = useNavigate();
  const { projects, fetchProjects } = useProjectStore();
  const { agentOS, server } = useSettingsStore();
  const [stats, setStats] = React.useState<StatsData | null>(null);
  const [activities, setActivities] = React.useState<ActivityItem[]>([]);
  const [health, setHealth] = React.useState<HealthInfo | null>(null);
  const [loading, setLoading] = React.useState(true);

  React.useEffect(() => {
    loadAllData();
  }, []);

  const loadAllData = async () => {
    setLoading(true);
    try {
      await Promise.all([
        fetchProjects(),
        loadDashboardData(),
      ]);
    } catch {
    } finally {
      setLoading(false);
    }
  };

  const loadDashboardData = async () => {
    try {
      const results = await Promise.allSettled([
        api.get<StatsData>('stats'),
        api.get<{ activities: ActivityItem[] }>('activity'),
        api.get<HealthInfo>('system/health'),
      ]);

      if (results[0].status === 'fulfilled') setStats(results[0].value);
      if (results[1].status === 'fulfilled') setActivities(results[1].value?.activities || []);
      if (results[2].status === 'fulfilled') setHealth(results[2].value);
    } catch {
    }
  };

  const statusIcon = (status: string) => {
    switch (status) {
      case 'running':
        return <ThunderboltOutlined style={{ color: '#1890ff' }} />;
      case 'completed':
      case 'success':
        return <CheckCircleOutlined style={{ color: '#52c41a' }} />;
      case 'failed':
        return <CloseCircleOutlined style={{ color: '#ff4d4f' }} />;
      case 'pending':
        return <ClockCircleOutlined style={{ color: '#faad14' }} />;
      default:
        return <DashboardOutlined style={{ color: '#8c8c8c' }} />;
    }
  };

  const statusColor = (status: string) => {
    const map: Record<string, string> = {
      running: '#e6f7ff',
      completed: '#f6ffed',
      success: '#f6ffed',
      failed: '#fff2f0',
      pending: '#fffbe6',
    };
    return map[status] || '#f5f5f5';
  };

  const statusLabel = (status: string) => {
    const map: Record<string, string> = {
      running: '运行中',
      completed: '已完成',
      success: '成功',
      failed: '失败',
      pending: '等待中',
      initialized: '已初始化',
      reviewing: '审查中',
    };
    return map[status] || status;
  };

  const formatTime = (t: string) => {
    if (!t) return '';
    const d = new Date(t);
    if (isNaN(d.getTime())) return '';
    const now = new Date();
    const diff = now.getTime() - d.getTime();
    if (diff < 60000) return '刚刚';
    if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`;
    return `${Math.floor(diff / 86400000)} 天前`;
  };

  const pipelineBars = [
    { label: '运行中', count: stats?.runningTasks || 0, color: '#1890ff', bgColor: '#e6f7ff' },
    { label: '已完成', count: stats?.completedTasks || 0, color: '#52c41a', bgColor: '#f6ffed' },
    { label: '失败', count: stats?.failedTasks || 0, color: '#ff4d4f', bgColor: '#fff2f0' },
    { label: '待审查', count: stats?.pendingReviews || 0, color: '#faad14', bgColor: '#fffbe6' },
  ];

  const recentProjects = projects.slice(0, 5);

  const healthItems = [
    {
      label: '后端服务',
      icon: <CloudServerOutlined />,
      healthy: health?.overall ?? (server.apiBaseUrl ? true : false),
      detail: server.apiBaseUrl || '未配置',
    },
    {
      label: 'Agent OS',
      icon: <RobotOutlined />,
      healthy: health?.agentOS?.healthy ?? false,
      detail: health?.agentOS?.message || agentOS.grpcAddress || '未连接',
    },
    {
      label: 'LLM 服务',
      icon: <ApiOutlined />,
      healthy: health?.llm?.healthy ?? false,
      detail: health?.llm?.message || '未配置',
    },
  ];

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 24 }}>
        <div>
          <h2 style={{ margin: 0 }}>仪表盘</h2>
          <span style={{ color: '#888' }}>AgentOS Center 平台概览</span>
        </div>
        <Button icon={<ReloadOutlined />} onClick={loadAllData} loading={loading}>刷新</Button>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} style={{ marginBottom: 20 }}>
          <Col xs={12} sm={8} md={4}>
            <Card bordered={false} hoverable>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                <div style={{ width: 40, height: 40, borderRadius: 8, background: '#e6f7ff', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  <ProjectOutlined style={{ color: '#1890ff', fontSize: 20 }} />
                </div>
                <div>
                  <div style={{ fontSize: 24, fontWeight: 600, color: '#1890ff' }}>{stats?.projectCount ?? projects.length}</div>
                  <div style={{ fontSize: 12, color: '#888' }}>项目总数</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card bordered={false} hoverable>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                <div style={{ width: 40, height: 40, borderRadius: 8, background: '#f9f0ff', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  <ThunderboltOutlined style={{ color: '#722ed1', fontSize: 20 }} />
                </div>
                <div>
                  <div style={{ fontSize: 24, fontWeight: 600, color: '#722ed1' }}>{stats?.runningTasks ?? 0}</div>
                  <div style={{ fontSize: 12, color: '#888' }}>运行中任务</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card bordered={false} hoverable>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                <div style={{ width: 40, height: 40, borderRadius: 8, background: '#f6ffed', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  <CheckCircleOutlined style={{ color: '#52c41a', fontSize: 20 }} />
                </div>
                <div>
                  <div style={{ fontSize: 24, fontWeight: 600, color: '#52c41a' }}>{stats?.completedTasks ?? 0}</div>
                  <div style={{ fontSize: 12, color: '#888' }}>已完成任务</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card bordered={false} hoverable>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                <div style={{ width: 40, height: 40, borderRadius: 8, background: '#fff2f0', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  <CloseCircleOutlined style={{ color: '#ff4d4f', fontSize: 20 }} />
                </div>
                <div>
                  <div style={{ fontSize: 24, fontWeight: 600, color: '#ff4d4f' }}>{stats?.failedTasks ?? 0}</div>
                  <div style={{ fontSize: 12, color: '#888' }}>失败任务</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card bordered={false} hoverable>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                <div style={{ width: 40, height: 40, borderRadius: 8, background: '#fffbe6', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  <AuditOutlined style={{ color: '#faad14', fontSize: 20 }} />
                </div>
                <div>
                  <div style={{ fontSize: 24, fontWeight: 600, color: '#faad14' }}>{stats?.pendingReviews ?? 0}</div>
                  <div style={{ fontSize: 12, color: '#888' }}>待审查</div>
                </div>
              </div>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} style={{ marginBottom: 20 }}>
          <Col xs={24} lg={10}>
            <Card
              title="系统健康"
              bordered={false}
              extra={<Button type="link" size="small" onClick={() => navigate('/monitor')}>监控详情 <RightOutlined /></Button>}
            >
              {healthItems.map((item) => (
                <div key={item.label} style={{ display: 'flex', alignItems: 'center', padding: '8px 0', gap: 12 }}>
                  <div style={{
                    width: 36, height: 36, borderRadius: 8,
                    background: item.healthy ? '#f6ffed' : '#fff2f0',
                    color: item.healthy ? '#52c41a' : '#ff4d4f',
                    display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 18,
                  }}>
                    {item.icon}
                  </div>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontWeight: 500 }}>{item.label}</div>
                    <div style={{ fontSize: 12, color: '#888' }}>{item.detail}</div>
                  </div>
                  <Tag color={item.healthy ? 'success' : 'error'}>{item.healthy ? '正常' : '异常'}</Tag>
                </div>
              ))}
            </Card>
          </Col>

          <Col xs={24} lg={14}>
            <Card
              title="流水线统计"
              bordered={false}
              extra={<span>任务总数 <strong>{stats?.taskCount ?? 0}</strong></span>}
            >
              <div style={{ display: 'flex', gap: 24, alignItems: 'center' }}>
                <div style={{ flexShrink: 0 }}>
                  <svg viewBox="0 0 120 120" width={120} height={120}>
                    {(() => {
                      const total = stats?.taskCount || 1;
                      if (total <= 0) {
                        return <circle cx="60" cy="60" r="50" fill="none" stroke="rgba(0,0,0,0.06)" strokeWidth="16" />;
                      }
                      const circumference = 2 * Math.PI * 50;
                      let offset = 0;
                      const segments = pipelineBars.filter(b => b.count > 0);
                      if (segments.length === 0) {
                        return <circle cx="60" cy="60" r="50" fill="none" stroke="rgba(0,0,0,0.06)" strokeWidth="16" />;
                      }
                      return (
                        <>
                          {segments.map((bar) => {
                            const pct = bar.count / total;
                            const dashLen = pct * circumference;
                            const dashOffset = -offset * circumference;
                            offset += pct;
                            return (
                              <circle
                                key={bar.label}
                                cx="60" cy="60" r="50"
                                fill="none" stroke={bar.color} strokeWidth="16"
                                strokeDasharray={`${dashLen} ${circumference - dashLen}`}
                                strokeDashoffset={dashOffset}
                                strokeLinecap="butt"
                                transform="rotate(-90 60 60)"
                              />
                            );
                          })}
                          <text x="60" y="55" textAnchor="middle" fontSize={22} fontWeight={700} fill="#333">
                            {stats?.taskCount ?? 0}
                          </text>
                          <text x="60" y="72" textAnchor="middle" fontSize={12} fill="#888">任务</text>
                        </>
                      );
                    })()}
                  </svg>
                </div>
                <div style={{ flex: 1 }}>
                  {pipelineBars.map((bar) => (
                    <div key={bar.label} style={{ marginBottom: 12 }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                        <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <span style={{ width: 8, height: 8, borderRadius: '50%', background: bar.color, display: 'inline-block' }} />
                          <span>{bar.label}</span>
                        </span>
                        <span style={{ fontWeight: 600 }}>{bar.count}</span>
                      </div>
                      <div style={{ height: 6, background: 'rgba(0,0,0,0.06)', borderRadius: 3, overflow: 'hidden' }}>
                        <div style={{ height: '100%', borderRadius: 3, background: bar.color, width: `${stats?.taskCount ? (bar.count / stats.taskCount) * 100 : 0}%` }} />
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]}>
          <Col xs={24} lg={14}>
            <Card
              title="最近活动"
              bordered={false}
              extra={<Button type="link" size="small" onClick={() => navigate('/logs')}>查看全部 <RightOutlined /></Button>}
            >
              {activities.length === 0 ? (
                <Empty description="暂无活动" />
              ) : (
                <div>
                  {activities.slice(0, 8).map((act, idx) => (
                    <div key={idx} style={{ display: 'flex', gap: 12, padding: '8px 0', borderBottom: '1px solid #f0f0f0' }}>
                      <div style={{ width: 32, height: 32, borderRadius: 8, background: statusColor(act.status), display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                        {statusIcon(act.status)}
                      </div>
                      <div style={{ flex: 1 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                          <Tooltip title={act.pipeline || act.taskId}>
                            <span style={{ fontWeight: 500 }}>{act.pipeline || act.taskId.slice(0, 8)}</span>
                          </Tooltip>
                          <Tag color={act.status === 'running' ? 'processing' : act.status === 'completed' || act.status === 'success' ? 'success' : act.status === 'failed' ? 'error' : 'default'}>
                            {statusLabel(act.status)}
                          </Tag>
                        </div>
                        <div style={{ fontSize: 12, color: '#888' }}>
                          {act.stage ? `阶段: ${act.stage}` : ''}{act.error ? ` | 错误: ${act.error.slice(0, 50)}` : ''}
                        </div>
                        <div style={{ fontSize: 12, color: '#bbb' }}>{act.startedAt ? formatTime(act.startedAt) : ''}</div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </Card>
          </Col>

          <Col xs={24} lg={10}>
            <Card title="快速操作" bordered={false}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                <Button type="primary" icon={<PlusOutlined />} block onClick={() => navigate('/projects')}>新建项目</Button>
                <Button icon={<RocketOutlined />} block onClick={() => navigate('/projects')}>项目列表</Button>
                <Button icon={<AuditOutlined />} block onClick={() => navigate('/reviews')}>
                  待审查任务{stats?.pendingReviews ? <Tag color="orange" style={{ marginLeft: 8 }}>{stats.pendingReviews}</Tag> : null}
                </Button>
                <Button icon={<BugOutlined />} block onClick={() => navigate('/logs')}>系统日志</Button>
              </div>
            </Card>

            <Card title="最近项目" bordered={false} style={{ marginTop: 16 }}
              extra={<Button type="link" size="small" onClick={() => navigate('/projects')}>查看全部 <RightOutlined /></Button>}>
              {recentProjects.length === 0 ? (
                <Empty description="暂无项目" image={Empty.PRESENTED_IMAGE_SIMPLE} />
              ) : (
                recentProjects.map((p: ProjectMeta) => (
                  <div key={p.projectId}
                    style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '8px 0', cursor: 'pointer', borderBottom: '1px solid #f0f0f0' }}
                    onClick={() => navigate(`/projects/${p.projectId}`)}>
                    <div>
                      <div style={{ fontWeight: 500 }}>{p.projectName}</div>
                      <div style={{ fontSize: 12, color: '#888' }}>{p.description || '无描述'}</div>
                    </div>
                    <Tag color={p.status === 'running' ? 'processing' : p.status === 'completed' ? 'success' : p.status === 'failed' ? 'error' : 'default'}>
                      {statusLabel(p.status)}
                    </Tag>
                  </div>
                ))
              )}
            </Card>
          </Col>
        </Row>
      </Spin>
    </div>
  );
};

export default Dashboard;