import React from 'react';
import { Card, Tabs, Button, Space, Descriptions, Tag, Spin, Drawer, Typography, Divider, Empty, List, App } from 'antd';
import { PlayCircleOutlined, StopOutlined, ReloadOutlined, RollbackOutlined, EyeOutlined } from '@ant-design/icons';
import { useParams, useNavigate } from 'react-router-dom';
import { useProjectStore, useWebSocketStore } from '@/stores';
import { StageStatusBadge, MermaidRenderer } from '@/components';
import type { StageInstanceMeta, StageDetail } from '@/types';
import { pipelineApi } from '@/api';

const { Paragraph } = Typography;

const StageDetailDrawer: React.FC<{
  stage: StageDetail | null;
  open: boolean;
  onClose: () => void;
}> = ({ stage, open, onClose }) => {
  if (!stage) return null;

  const stageTypeLabels: Record<string, string> = {
    requirement: '需求分析',
    design: '系统设计',
    coding: '编码实现',
    testing: '测试验证',
    review: '代码审查',
    cicd: 'CI/CD',
    deploy: '部署发布',
  };

  return (
    <Drawer
      title={stage.name || stageTypeLabels[stage.stageType] || stage.stageType}
      placement="right"
      width={640}
      open={open}
      onClose={onClose}
    >
      <Descriptions column={2} size="small" bordered>
        <Descriptions.Item label="阶段ID">{stage.stageId}</Descriptions.Item>
        <Descriptions.Item label="类型">{stageTypeLabels[stage.stageType] || stage.stageType}</Descriptions.Item>
        <Descriptions.Item label="状态"><StageStatusBadge status={stage.status} /></Descriptions.Item>
        <Descriptions.Item label="顺序">{stage.order}</Descriptions.Item>
        {stage.startedAt && (
          <Descriptions.Item label="开始时间">{new Date(stage.startedAt).toLocaleString()}</Descriptions.Item>
        )}
        {stage.completedAt && (
          <Descriptions.Item label="完成时间">{new Date(stage.completedAt).toLocaleString()}</Descriptions.Item>
        )}
        {stage.durationMs != null && (
          <Descriptions.Item label="耗时" span={2}>
            {stage.durationMs < 1000 ? `${stage.durationMs}ms` : `${(stage.durationMs / 1000).toFixed(1)}s`}
          </Descriptions.Item>
        )}
        {stage.retryCount > 0 && (
          <Descriptions.Item label="重试次数" span={2}>{stage.retryCount}</Descriptions.Item>
        )}
        <Descriptions.Item label="超时设置" span={2}>{stage.timeoutSeconds ? `${stage.timeoutSeconds}s` : '-'}</Descriptions.Item>
        <Descriptions.Item label="失败策略" span={2}>{stage.onFailure || '-'}</Descriptions.Item>
      </Descriptions>

      {stage.summary && (
        <>
          <Divider>摘要</Divider>
          <Paragraph>{stage.summary}</Paragraph>
        </>
      )}

      {stage.errors && stage.errors.length > 0 && (
        <>
          <Divider>错误信息</Divider>
          <List
            size="small"
            dataSource={stage.errors}
            renderItem={(err) => (
              <List.Item>
                <Tag color="error">{err.code}</Tag>
                <span>{err.message}</span>
                <span style={{ fontSize: 12, color: '#999', marginLeft: 8 }}>
                  {err.timestamp ? new Date(err.timestamp).toLocaleString() : ''}
                </span>
              </List.Item>
            )}
          />
        </>
      )}

      {stage.output && Object.keys(stage.output).length > 0 && (() => {
        const output = stage.output as Record<string, unknown>;
        return (
          <>
            <Divider>输出结果</Divider>
            <div>
              {!!output.summary && <Paragraph>{String(output.summary)}</Paragraph>}
              {!!output.mermaid && <MermaidRenderer code={String(output.mermaid)} theme="light" />}
              {!!output.code && (
                <pre style={{ background: '#f5f5f5', padding: 12, borderRadius: 6, overflow: 'auto' }}>
                  <code>{String(output.code)}</code>
                </pre>
              )}
              {!output.summary && !output.mermaid && !output.code && (
                <pre style={{ background: '#f5f5f5', padding: 12, borderRadius: 6 }}>{JSON.stringify(output, null, 2)}</pre>
              )}
            </div>
          </>
        );
      })()}

      {stage.artifacts && stage.artifacts.length > 0 && (
        <>
          <Divider>产物</Divider>
          <List
            size="small"
            dataSource={stage.artifacts}
            renderItem={(a) => (
              <List.Item>
                <Tag>{a.type}</Tag>
                <span>{a.name}</span>
                <span style={{ fontSize: 12, color: '#999', marginLeft: 8 }}>{a.path}</span>
              </List.Item>
            )}
          />
        </>
      )}

      {stage.error && (
        <>
          <Divider>错误</Divider>
          <Paragraph type="danger">{stage.error}</Paragraph>
        </>
      )}
    </Drawer>
  );
};

const ProjectDetail: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { currentProject, fetchProject } = useProjectStore();
  const { connect, disconnect, lastEvent } = useWebSocketStore();

  const [stages, setStages] = React.useState<StageInstanceMeta[]>([]);
  const [taskMeta, setTaskMeta] = React.useState<{ taskId: string } | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [selectedStage, setSelectedStage] = React.useState<StageDetail | null>(null);
  const [drawerOpen, setDrawerOpen] = React.useState(false);
  const [stageLoading, setStageLoading] = React.useState<string | null>(null);
  const { message } = App.useApp();

  React.useEffect(() => {
    if (id) {
      fetchProject(id);
      connect(id);
    }
    return () => disconnect();
  }, [id, fetchProject, connect, disconnect]);

  const handleStartPipeline = async () => {
    if (!id) return;
    setLoading(true);
    try {
      const result = await pipelineApi.start({ project_name: currentProject?.projectName || 'default' });
      setTaskMeta({ taskId: result.taskId });
      const stagesData = await pipelineApi.getStages(result.taskId);
      setStages(stagesData);
      message.success('管线已启动');
    } catch (error) {
      message.error((error as Error).message);
    } finally {
      setLoading(false);
    }
  };

  const handleStageClick = async (stage: StageInstanceMeta) => {
    if (!taskMeta?.taskId) return;
    setStageLoading(stage.stageId);
    try {
      const detail = await pipelineApi.getStage(taskMeta.taskId, stage.stageId);
      setSelectedStage(detail);
      setDrawerOpen(true);
    } catch {
      setSelectedStage(null);
    } finally {
      setStageLoading(null);
    }
  };

  if (!currentProject) {
    return <Spin />;
  }

  const tabItems = [
    {
      key: 'workspace',
      label: '管线工作区',
      children: (
        <div>
          <Space style={{ marginBottom: 16 }}>
            <Button type="primary" icon={<PlayCircleOutlined />} onClick={handleStartPipeline} loading={loading}>启动管线</Button>
            <Button icon={<StopOutlined />}>停止</Button>
            <Button icon={<ReloadOutlined />}>重试</Button>
            <Button icon={<RollbackOutlined />}>回退</Button>
          </Space>

          <Card title="阶段列表">
            {stages.length === 0 ? (
              <div style={{ textAlign: 'center', padding: 40, color: '#888' }}>暂无阶段数据，请启动管线</div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                {stages.map((stage: StageInstanceMeta) => (
                  <div
                    key={stage.stageId}
                    style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '12px 16px', border: '1px solid #f0f0f0', borderRadius: 6, cursor: 'pointer' }}
                    onClick={() => handleStageClick(stage)}
                  >
                    <div>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                        <span style={{ fontWeight: 500 }}>{stage.name}</span>
                        <StageStatusBadge status={stage.status} />
                      </div>
                      <div style={{ fontSize: 12, color: '#888' }}>
                        <span>类型: {stage.stageType}</span>
                        {stage.durationMs != null && <span style={{ marginLeft: 12 }}>耗时: {stage.durationMs < 1000 ? `${stage.durationMs}ms` : `${(stage.durationMs / 1000).toFixed(1)}s`}</span>}
                        {stage.retryCount > 0 && <span style={{ marginLeft: 12 }}>重试: {stage.retryCount}次</span>}
                      </div>
                    </div>
                    <Button type="link" size="small" icon={<EyeOutlined />} loading={stageLoading === stage.stageId}>详情</Button>
                  </div>
                ))}
              </div>
            )}
          </Card>
        </div>
      ),
    },
    {
      key: 'editor',
      label: '管线编辑器',
      children: (
        <div style={{ textAlign: 'center', padding: 60 }}>
          <Button type="primary" onClick={() => navigate(`/projects/${id}/editor`)}>打开管线编辑器</Button>
        </div>
      ),
    },
  ];

  return (
    <div>
      <Card style={{ marginBottom: 16 }}>
        <Descriptions title="项目信息">
          <Descriptions.Item label="项目名称">{currentProject.projectName}</Descriptions.Item>
          <Descriptions.Item label="描述">{currentProject.description || '-'}</Descriptions.Item>
          <Descriptions.Item label="状态">
            <Tag color={currentProject.status === 'running' ? 'processing' : currentProject.status === 'completed' ? 'success' : currentProject.status === 'failed' ? 'error' : 'default'}>
              {currentProject.status === 'initialized' ? '已初始化' : currentProject.status === 'running' ? '运行中' : currentProject.status === 'completed' ? '已完成' : currentProject.status === 'failed' ? '失败' : currentProject.status}
            </Tag>
          </Descriptions.Item>
          <Descriptions.Item label="创建时间">{new Date(currentProject.createdAt).toLocaleString()}</Descriptions.Item>
        </Descriptions>
      </Card>

      <Tabs defaultActiveKey="workspace" items={tabItems} />

      <StageDetailDrawer stage={selectedStage} open={drawerOpen} onClose={() => setDrawerOpen(false)} />
    </div>
  );
};

export default ProjectDetail;