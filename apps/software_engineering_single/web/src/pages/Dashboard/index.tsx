import React from 'react';
import {
  Card,
  Row,
  Col,
  Button,
  Tag,
  Tooltip,
  Empty,
  Spin,
  Progress,
} from 'antd';
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
import { useProjectStore } from '@/stores';
import { useMonitorStore } from '@/stores';
import { api } from '@/api';
import type { ProjectMeta, HealthCheckResult, ResourceUsage, ActiveTask } from '@/types';
import styles from './Dashboard.module.css';

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

interface PipelineTrend {
  date: string;
  success: number;
  failed: number;
  running: number;
}

const Dashboard: React.FC = () => {
  const navigate = useNavigate();
  const { projects, fetchProjects } = useProjectStore();
  const { agentOSStatus, temporalStatus, resourceUsage, activeTasks, fetchAll: fetchMonitor } = useMonitorStore();

  const [stats, setStats] = React.useState<StatsData | null>(null);
  const [activities, setActivities] = React.useState<ActivityItem[]>([]);
  const [trends, setTrends] = React.useState<PipelineTrend[]>([]);
  const [health, setHealth] = React.useState<HealthCheckResult | null>(null);
  const [loading, setLoading] = React.useState(true);

  React.useEffect(() => {
    loadAllData();
  }, []);

  const loadAllData = async () => {
    setLoading(true);
    try {
      await Promise.all([
        fetchProjects(),
        fetchMonitor(),
        loadDashboardData(),
      ]);
    } catch {
      // silently ignore
    } finally {
      setLoading(false);
    }
  };

  const loadDashboardData = async () => {
    try {
      const results = await Promise.allSettled([
        api.get<StatsData>('stats'),
        api.get<{ activities: ActivityItem[] }>('activity'),
        api.get<{ trends: PipelineTrend[] }>('stats/pipeline-trends'),
        api.get<HealthCheckResult>('system/health'),
      ]);

      if (results[0].status === 'fulfilled') setStats(results[0].value);
      if (results[1].status === 'fulfilled') setActivities(results[1].value?.activities || []);
      if (results[2].status === 'fulfilled') setTrends(results[2].value?.trends || []);
      if (results[3].status === 'fulfilled') setHealth(results[3].value);
    } catch {
      // silently ignore
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

  const taskCount = stats?.taskCount || 1;
  const pipelineBars = [
    { label: '运行中', count: stats?.runningTasks || 0, color: '#1890ff', bgColor: '#e6f7ff' },
    { label: '已完成', count: stats?.completedTasks || 0, color: '#52c41a', bgColor: '#f6ffed' },
    { label: '失败', count: stats?.failedTasks || 0, color: '#ff4d4f', bgColor: '#fff2f0' },
    { label: '待审查', count: stats?.pendingReviews || 0, color: '#faad14', bgColor: '#fffbe6' },
  ];

  const recentProjects = projects.slice(0, 5);

  const maxTrendValue = Math.max(
    ...trends.map((t) => t.success + t.failed + t.running),
    1
  );

  const healthItems = [
    {
      label: 'Agent OS',
      icon: <CloudServerOutlined />,
      healthy: health?.agentOS?.healthy ?? agentOSStatus?.running ?? false,
      detail: health?.agentOS?.message || (agentOSStatus?.version ? `v${agentOSStatus.version}` : '未连接'),
    },
    {
      label: 'Temporal',
      icon: <ApiOutlined />,
      healthy: health?.temporal?.healthy ?? temporalStatus?.connected ?? false,
      detail: health?.temporal?.message || (temporalStatus?.namespace ? temporalStatus.namespace : '未连接'),
    },
    {
      label: 'LLM 服务',
      icon: <RobotOutlined />,
      healthy: health?.llm?.healthy ?? false,
      detail: health?.llm?.message || '未配置',
    },
  ];

  const resourceData = resourceUsage || { cpuPercent: 0, memoryUsedMB: 0, memoryTotalMB: 0, diskUsedGB: 0, diskTotalGB: 0 };

  const getResourceColor = (percent: number) => {
    if (percent >= 90) return '#ff4d4f';
    if (percent >= 70) return '#faad14';
    return '#52c41a';
  };

  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <h2 className={styles.pageTitle}>仪表盘</h2>
          <span className={styles.pageSubtitle}>SDLC Agent 平台概览</span>
        </div>
        <Button
          icon={<ReloadOutlined />}
          onClick={loadAllData}
          loading={loading}
        >
          刷新
        </Button>
      </div>

      <Spin spinning={loading}>
        <Row gutter={[16, 16]} style={{ marginBottom: 20 }}>
          <Col xs={12} sm={8} md={4}>
            <Card className={styles.statCard} bordered={false} hoverable>
              <div className={styles.statInner}>
                <div className={styles.statIcon} style={{ background: '#e6f7ff' }}>
                  <ProjectOutlined style={{ color: '#1890ff', fontSize: 20 }} />
                </div>
                <div className={styles.statInfo}>
                  <div className={styles.statValue} style={{ color: '#1890ff' }}>
                    {stats?.projectCount ?? projects.length}
                  </div>
                  <div className={styles.statTitle}>项目总数</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card className={styles.statCard} bordered={false} hoverable>
              <div className={styles.statInner}>
                <div className={styles.statIcon} style={{ background: '#f9f0ff' }}>
                  <ThunderboltOutlined style={{ color: '#722ed1', fontSize: 20 }} />
                </div>
                <div className={styles.statInfo}>
                  <div className={styles.statValue} style={{ color: '#722ed1' }}>
                    {stats?.runningTasks ?? activeTasks?.length ?? 0}
                  </div>
                  <div className={styles.statTitle}>运行中任务</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card className={styles.statCard} bordered={false} hoverable>
              <div className={styles.statInner}>
                <div className={styles.statIcon} style={{ background: '#f6ffed' }}>
                  <CheckCircleOutlined style={{ color: '#52c41a', fontSize: 20 }} />
                </div>
                <div className={styles.statInfo}>
                  <div className={styles.statValue} style={{ color: '#52c41a' }}>
                    {stats?.completedTasks ?? 0}
                  </div>
                  <div className={styles.statTitle}>已完成任务</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card className={styles.statCard} bordered={false} hoverable>
              <div className={styles.statInner}>
                <div className={styles.statIcon} style={{ background: '#fff2f0' }}>
                  <CloseCircleOutlined style={{ color: '#ff4d4f', fontSize: 20 }} />
                </div>
                <div className={styles.statInfo}>
                  <div className={styles.statValue} style={{ color: '#ff4d4f' }}>
                    {stats?.failedTasks ?? 0}
                  </div>
                  <div className={styles.statTitle}>失败任务</div>
                </div>
              </div>
            </Card>
          </Col>
          <Col xs={12} sm={8} md={5}>
            <Card className={styles.statCard} bordered={false} hoverable>
              <div className={styles.statInner}>
                <div className={styles.statIcon} style={{ background: '#fffbe6' }}>
                  <AuditOutlined style={{ color: '#faad14', fontSize: 20 }} />
                </div>
                <div className={styles.statInfo}>
                  <div className={styles.statValue} style={{ color: '#faad14' }}>
                    {stats?.pendingReviews ?? 0}
                  </div>
                  <div className={styles.statTitle}>待审查</div>
                </div>
              </div>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} style={{ marginBottom: 20 }}>
          <Col xs={24} lg={10}>
            <Card
              title={<span className={styles.cardTitle}>系统健康</span>}
              bordered={false}
              className={styles.healthCard}
              extra={
                <Button type="link" size="small" onClick={() => navigate('/monitor')}>
                  监控详情 <RightOutlined />
                </Button>
              }
            >
              <div className={styles.healthList}>
                {healthItems.map((item) => (
                  <div className={styles.healthItem} key={item.label}>
                    <div className={styles.healthIcon} style={{
                      background: item.healthy ? '#f6ffed' : '#fff2f0',
                      color: item.healthy ? '#52c41a' : '#ff4d4f',
                      fontSize: 18,
                    }}>
                      {item.icon}
                    </div>
                    <div className={styles.healthInfo}>
                      <div className={styles.healthLabel}>{item.label}</div>
                      <div className={styles.healthDetail}>{item.detail}</div>
                    </div>
                    <Tag color={item.healthy ? 'success' : 'error'} style={{ marginLeft: 'auto' }}>
                      {item.healthy ? '正常' : '异常'}
                    </Tag>
                  </div>
                ))}
              </div>
            </Card>
          </Col>

          <Col xs={24} lg={14}>
            <Card
              title={<span className={styles.cardTitle}>流水线统计</span>}
              bordered={false}
              extra={
                <span className={styles.taskTotal}>
                  任务总数 <strong>{stats?.taskCount ?? 0}</strong>
                </span>
              }
            >
              <div className={styles.pipelineStats}>
                <div className={styles.pipelineDonut}>
                  <svg viewBox="0 0 120 120" className={styles.donutSvg}>
                    {(() => {
                      const total = taskCount;
                      if (total <= 0) {
                        return (
                          <circle cx="60" cy="60" r="50" fill="none" stroke="rgba(0,0,0,0.06)" strokeWidth="16" />
                        );
                      }
                      const circumference = 2 * Math.PI * 50;
                      let offset = 0;
                      const segments = pipelineBars.filter(b => b.count > 0);
                      if (segments.length === 0) {
                        return (
                          <circle cx="60" cy="60" r="50" fill="none" stroke="rgba(0,0,0,0.06)" strokeWidth="16" />
                        );
                      }
                      return segments.map((bar) => {
                        const pct = bar.count / total;
                        const dashLen = pct * circumference;
                        const dashOffset = -offset * circumference;
                        offset += pct;
                        return (
                          <circle
                            key={bar.label}
                            cx="60"
                            cy="60"
                            r="50"
                            fill="none"
                            stroke={bar.color}
                            strokeWidth="16"
                            strokeDasharray={`${dashLen} ${circumference - dashLen}`}
                            strokeDashoffset={dashOffset}
                            strokeLinecap="butt"
                            transform="rotate(-90 60 60)"
                            className={styles.donutSegment}
                          />
                        );
                      });
                    })()}
                    <text x="60" y="55" textAnchor="middle" className={styles.donutCenterValue}>
                      {stats?.taskCount ?? 0}
                    </text>
                    <text x="60" y="72" textAnchor="middle" className={styles.donutCenterLabel}>
                      任务
                    </text>
                  </svg>
                </div>
                <div className={styles.pipelineBars}>
                  {pipelineBars.map((bar) => (
                    <div className={styles.pipelineBar} key={bar.label}>
                      <div className={styles.pipelineBarHeader}>
                        <span className={styles.pipelineBarDot} style={{ background: bar.color }} />
                        <span className={styles.pipelineBarLabel}>{bar.label}</span>
                        <span className={styles.pipelineBarCount}>{bar.count}</span>
                      </div>
                      <div className={styles.pipelineBarTrack}>
                        <div
                          className={styles.pipelineBarFill}
                          style={{
                            width: `${taskCount > 0 ? (bar.count / taskCount) * 100 : 0}%`,
                            background: bar.color,
                          }}
                        />
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} style={{ marginBottom: 20 }}>
          <Col xs={24} lg={14}>
            <Card
              title={<span className={styles.cardTitle}>流水线趋势</span>}
              bordered={false}
              className={styles.trendCard}
            >
              {trends.length === 0 ? (
                <div className={styles.trendEmpty}>
                  <Empty description="暂无趋势数据" image={Empty.PRESENTED_IMAGE_SIMPLE} />
                </div>
              ) : (
                <div className={styles.trendChart}>
                  {trends.map((t, idx) => {
                    const total = t.success + t.failed + t.running;
                    const successH = (t.success / maxTrendValue) * 100;
                    const failedH = (t.failed / maxTrendValue) * 100;
                    const runningH = (t.running / maxTrendValue) * 100;
                    return (
                      <div className={styles.trendCol} key={idx}>
                        <Tooltip title={`成功: ${t.success} | 失败: ${t.failed} | 运行中: ${t.running}`}>
                          <div className={styles.trendBarStack}>
                            <div className={styles.trendBarSegment} style={{ height: `${successH}%`, background: '#52c41a' }} />
                            <div className={styles.trendBarSegment} style={{ height: `${runningH}%`, background: '#1890ff' }} />
                            <div className={styles.trendBarSegment} style={{ height: `${failedH}%`, background: '#ff4d4f' }} />
                          </div>
                        </Tooltip>
                        <div className={styles.trendLabel}>{t.date}</div>
                        <div className={styles.trendTotal}>{total}</div>
                      </div>
                    );
                  })}
                </div>
              )}
              <div className={styles.trendLegend}>
                <span className={styles.legendItem}><span className={styles.legendDot} style={{ background: '#52c41a' }} />成功</span>
                <span className={styles.legendItem}><span className={styles.legendDot} style={{ background: '#1890ff' }} />运行中</span>
                <span className={styles.legendItem}><span className={styles.legendDot} style={{ background: '#ff4d4f' }} />失败</span>
              </div>
            </Card>
          </Col>

          <Col xs={24} lg={10}>
            <Card
              title={<span className={styles.cardTitle}>资源使用</span>}
              bordered={false}
              extra={
                <Button type="link" size="small" onClick={() => navigate('/monitor')}>
                  详情 <RightOutlined />
                </Button>
              }
            >
              <div className={styles.resourceList}>
                <div className={styles.resourceItem}>
                  <div className={styles.resourceHeader}>
                    <span className={styles.resourceLabel}>CPU</span>
                    <span className={styles.resourceValue}>{resourceData.cpuPercent}%</span>
                  </div>
                  <Progress
                    percent={resourceData.cpuPercent}
                    showInfo={false}
                    strokeColor={getResourceColor(resourceData.cpuPercent)}
                    trailColor="rgba(0,0,0,0.06)"
                    size="small"
                  />
                </div>
                <div className={styles.resourceItem}>
                  <div className={styles.resourceHeader}>
                    <span className={styles.resourceLabel}>内存</span>
                    <span className={styles.resourceValue}>
                      {resourceData.memoryUsedMB} / {resourceData.memoryTotalMB} MB
                    </span>
                  </div>
                  <Progress
                    percent={resourceData.memoryTotalMB > 0 ? Math.round((resourceData.memoryUsedMB / resourceData.memoryTotalMB) * 100) : 0}
                    showInfo={false}
                    strokeColor={getResourceColor(resourceData.memoryTotalMB > 0 ? Math.round((resourceData.memoryUsedMB / resourceData.memoryTotalMB) * 100) : 0)}
                    trailColor="rgba(0,0,0,0.06)"
                    size="small"
                  />
                </div>
                <div className={styles.resourceItem}>
                  <div className={styles.resourceHeader}>
                    <span className={styles.resourceLabel}>磁盘</span>
                    <span className={styles.resourceValue}>
                      {resourceData.diskUsedGB} / {resourceData.diskTotalGB} GB
                    </span>
                  </div>
                  <Progress
                    percent={resourceData.diskTotalGB > 0 ? Math.round((resourceData.diskUsedGB / resourceData.diskTotalGB) * 100) : 0}
                    showInfo={false}
                    strokeColor={getResourceColor(resourceData.diskTotalGB > 0 ? Math.round((resourceData.diskUsedGB / resourceData.diskTotalGB) * 100) : 0)}
                    trailColor="rgba(0,0,0,0.06)"
                    size="small"
                  />
                </div>
              </div>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} style={{ marginBottom: 20 }}>
          <Col xs={24} lg={14}>
            <Card
              title={<span className={styles.cardTitle}>最近活动</span>}
              bordered={false}
              className={styles.activityCard}
              extra={
                <Button type="link" size="small" onClick={() => navigate('/logs')}>
                  查看全部 <RightOutlined />
                </Button>
              }
            >
              {activities.length === 0 && activeTasks.length === 0 ? (
                <Empty description="暂无活动" />
              ) : (
                <div className={styles.activityList}>
                  {(activities.length > 0 ? activities : activeTasks.map((t) => ({
                    type: 'task',
                    taskId: t.taskId,
                    projectId: t.projectId,
                    pipeline: t.pipeline,
                    status: t.status,
                    stage: t.stage,
                    startedAt: t.startedAt,
                    completedAt: '',
                    error: '',
                  }))).slice(0, 8).map((act, idx) => (
                    <div className={styles.activityItem} key={idx}>
                      <div
                        className={styles.activityIcon}
                        style={{ background: statusColor(act.status) }}
                      >
                        {statusIcon(act.status)}
                      </div>
                      <div className={styles.activityContent}>
                        <div className={styles.activityTitle}>
                          <Tooltip title={act.pipeline || act.taskId}>
                            <span className={styles.activityName}>
                              {act.pipeline || act.taskId.slice(0, 8)}
                            </span>
                          </Tooltip>
                          <Tag
                            color={
                              act.status === 'running'
                                ? 'processing'
                                : act.status === 'completed' || act.status === 'success'
                                ? 'success'
                                : act.status === 'failed'
                                ? 'error'
                                : 'default'
                            }
                          >
                            {statusLabel(act.status)}
                          </Tag>
                        </div>
                        <div className={styles.activityDesc}>
                          {act.stage ? `阶段: ${act.stage}` : ''}
                          {act.error ? ` | 错误: ${act.error.slice(0, 50)}` : ''}
                        </div>
                        <div className={styles.activityTime}>
                          {act.startedAt ? formatTime(act.startedAt) : ''}
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </Card>
          </Col>

          <Col xs={24} lg={10}>
            <Card
              title={<span className={styles.cardTitle}>快速操作</span>}
              bordered={false}
            >
              <div className={styles.quickActions}>
                <Button
                  type="primary"
                  icon={<PlusOutlined />}
                  className={styles.quickActionBtn}
                  block
                  onClick={() => navigate('/projects')}
                >
                  新建项目
                </Button>
                <Button
                  icon={<RocketOutlined />}
                  className={styles.quickActionBtn}
                  block
                  onClick={() => navigate('/projects')}
                >
                  项目列表
                </Button>
                <Button
                  icon={<AuditOutlined />}
                  className={styles.quickActionBtn}
                  block
                  onClick={() => navigate('/reviews')}
                >
                  待审查任务
                  {stats?.pendingReviews ? (
                    <Tag color="orange" style={{ marginLeft: 8 }}>
                      {stats.pendingReviews}
                    </Tag>
                  ) : null}
                </Button>
                <Button
                  icon={<BugOutlined />}
                  className={styles.quickActionBtn}
                  block
                  onClick={() => navigate('/logs')}
                >
                  系统日志
                </Button>
              </div>
            </Card>

            <Card
              title={<span className={styles.cardTitle}>最近项目</span>}
              bordered={false}
              style={{ marginTop: 16 }}
              extra={
                <Button type="link" size="small" onClick={() => navigate('/projects')}>
                  查看全部 <RightOutlined />
                </Button>
              }
            >
              {recentProjects.length === 0 ? (
                <Empty description="暂无项目" image={Empty.PRESENTED_IMAGE_SIMPLE} />
              ) : (
                <div className={styles.recentProjectList}>
                  {recentProjects.map((p: ProjectMeta) => (
                    <div
                      key={p.projectId}
                      className={styles.recentProjectItem}
                      onClick={() => navigate(`/projects/${p.projectId}`)}
                    >
                      <div className={styles.recentProjectInfo}>
                        <div className={styles.recentProjectName}>{p.projectName}</div>
                        <div className={styles.recentProjectDesc}>{p.description || '无描述'}</div>
                      </div>
                      <Tag
                        color={
                          p.status === 'running' ? 'processing'
                          : p.status === 'completed' ? 'success'
                          : p.status === 'failed' ? 'error'
                          : 'default'
                        }
                      >
                        {statusLabel(p.status)}
                      </Tag>
                    </div>
                  ))}
                </div>
              )}
            </Card>
          </Col>
        </Row>
      </Spin>
    </div>
  );
};

export default Dashboard;
