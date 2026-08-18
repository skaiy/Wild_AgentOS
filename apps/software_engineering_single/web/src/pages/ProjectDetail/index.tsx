import React from 'react';
import { Card, Tabs, Button, Space, Descriptions, Tag, Spin, message, Drawer, Typography, Divider, Empty, List } from 'antd';
import { PlayCircleOutlined, StopOutlined, ReloadOutlined, RollbackOutlined, EyeOutlined } from '@ant-design/icons';
import { useParams, useNavigate } from 'react-router-dom';
import { useProjectStore, usePipelineStore, useWebSocketStore } from '@/stores';
import { StageStatusBadge, MermaidRenderer } from '@/components';
import type { StageInstanceMeta, StageDetail } from '@/types';
import { pipelineApi } from '@/api';
import MessageContentRenderer from '@/pages/Chat/MessageContentRenderer';
import styles from './ProjectDetail.module.css';

const { Paragraph, Title } = Typography;

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
          <Descriptions.Item label="开始时间">
            {new Date(stage.startedAt).toLocaleString()}
          </Descriptions.Item>
        )}
        {stage.completedAt && (
          <Descriptions.Item label="完成时间">
            {new Date(stage.completedAt).toLocaleString()}
          </Descriptions.Item>
        )}
        {stage.durationMs != null && (
          <Descriptions.Item label="耗时" span={2}>
            {stage.durationMs < 1000
              ? `${stage.durationMs}ms`
              : `${(stage.durationMs / 1000).toFixed(1)}s`}
          </Descriptions.Item>
        )}
        {stage.retryCount > 0 && (
          <Descriptions.Item label="重试次数" span={2}>
            {stage.retryCount}
          </Descriptions.Item>
        )}
        <Descriptions.Item label="超时设置" span={2}>
          {stage.timeoutSeconds ? `${stage.timeoutSeconds}s` : '-'}
        </Descriptions.Item>
        <Descriptions.Item label="失败策略" span={2}>
          {stage.onFailure || '-'}
        </Descriptions.Item>
      </Descriptions>

      {stage.summary && (
        <>
          <Divider>摘要</Divider>
          <MessageContentRenderer
            content={[{ type: 'text', data: stage.summary }]}
            role="assistant"
          />
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
            <div className={styles.outputSection}>
              {!!output.summary && (
                <MessageContentRenderer
                  content={[{ type: 'text', data: String(output.summary) }]}
                  role="assistant"
                />
              )}
              {!!output.mermaid && (
                <MermaidRenderer code={String(output.mermaid)} theme="light" />
              )}
              {!!output.code && (
                <MessageContentRenderer
                  content={[{
                    type: 'code',
                    data: { code: String(output.code), language: String(output.language || 'text') },
                  }]}
                  role="assistant"
                />
              )}
              {!output.summary && !output.mermaid && !output.code && (
                <pre className={styles.outputPre}>{JSON.stringify(output, null, 2)}</pre>
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
  const { stages, taskMeta, loading, startPipeline, fetchPipelineStatus, retryTask, rollbackTask, fetchStageDetail, currentStage } = usePipelineStore();
  const { connect, disconnect, lastEvent } = useWebSocketStore();

  const [selectedStage, setSelectedStage] = React.useState<StageDetail | null>(null);
  const [drawerOpen, setDrawerOpen] = React.useState(false);
  const [stageLoading, setStageLoading] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (id) {
      fetchProject(id);
      connect(id);
    }
    return () => disconnect();
  }, [id, fetchProject, connect, disconnect]);

  React.useEffect(() => {
    if (lastEvent) {
      if (taskMeta?.taskId) {
        fetchPipelineStatus(taskMeta.taskId);
      }
    }
  }, [lastEvent, taskMeta?.taskId, fetchPipelineStatus]);

  const handleStartPipeline = async () => {
    if (!id) return;
    try {
      await startPipeline(id, currentProject?.projectName || 'default');
      message.success('管线已启动');
    } catch (error) {
      message.error((error as Error).message);
    }
  };

  const handleRetry = async () => {
    if (!taskMeta?.taskId) return;
    try {
      await retryTask(taskMeta.taskId);
      message.success('重试成功');
    } catch (error) {
      message.error((error as Error).message);
    }
  };

  const handleRollback = async () => {
    if (!taskMeta?.taskId) return;
    try {
      await rollbackTask(taskMeta.taskId);
      message.success('回退成功');
    } catch (error) {
      message.error((error as Error).message);
    }
  };

  const handleStageClick = async (stage: StageInstanceMeta) => {
    if (!taskMeta?.taskId) return;
    setStageLoading(stage.stageId);
    try {
      await fetchStageDetail(taskMeta.taskId, stage.stageId);
      setSelectedStage(currentStage);
      setDrawerOpen(true);
    } catch {
      setSelectedStage(null);
    } finally {
      setStageLoading(null);
    }
  };

  React.useEffect(() => {
    if (currentStage && stageLoading === currentStage.stageId) {
      setSelectedStage(currentStage);
      setDrawerOpen(true);
    }
  }, [currentStage, stageLoading]);

  if (!currentProject) {
    return <Spin />;
  }

  const tabItems = [
    {
      key: 'workspace',
      label: '管线工作区',
      children: (
        <div className={styles.workspace}>
          <Space className={styles.actions}>
            <Button type="primary" icon={<PlayCircleOutlined />} onClick={handleStartPipeline}>
              启动管线
            </Button>
            <Button icon={<StopOutlined />}>停止</Button>
            <Button icon={<ReloadOutlined />} onClick={handleRetry}>
              重试
            </Button>
            <Button icon={<RollbackOutlined />} onClick={handleRollback}>
              回退
            </Button>
          </Space>

          <Card title="阶段列表" className={styles.stagesCard}>
            {stages.length === 0 ? (
              <div className={styles.empty}>暂无阶段数据，请启动管线</div>
            ) : (
              <div className={styles.stagesList}>
                {stages.map((stage: StageInstanceMeta) => (
                  <div
                    key={stage.stageId}
                    className={styles.stageItem}
                    onClick={() => handleStageClick(stage)}
                    style={{ cursor: 'pointer' }}
                  >
                    <div className={styles.stageInfo}>
                      <span className={styles.stageName}>{stage.name}</span>
                      <StageStatusBadge status={stage.status} />
                    </div>
                    <div className={styles.stageMeta}>
                      <span>类型: {stage.stageType}</span>
                      {stage.durationMs != null && <span>耗时: {stage.durationMs < 1000 ? `${stage.durationMs}ms` : `${(stage.durationMs / 1000).toFixed(1)}s`}</span>}
                      {stage.retryCount > 0 && <span>重试: {stage.retryCount}次</span>}
                    </div>
                    <Button
                      type="link"
                      size="small"
                      icon={<EyeOutlined />}
                      loading={stageLoading === stage.stageId}
                    >
                      详情
                    </Button>
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
        <div className={styles.editorPlaceholder}>
          <Button type="primary" onClick={() => navigate(`/projects/${id}/editor`)}>
            打开管线编辑器
          </Button>
        </div>
      ),
    },
  ];

  return (
    <div className={styles.container}>
      <Card className={styles.infoCard}>
        <Descriptions title="项目信息">
          <Descriptions.Item label="项目名称">{currentProject.projectName}</Descriptions.Item>
          <Descriptions.Item label="描述">{currentProject.description || '-'}</Descriptions.Item>
          <Descriptions.Item label="状态">
            <Tag color={currentProject.status === 'running' ? 'processing' : currentProject.status === 'completed' ? 'success' : currentProject.status === 'failed' ? 'error' : 'default'}>
              {currentProject.status === 'initialized' ? '已初始化' : currentProject.status === 'running' ? '运行中' : currentProject.status === 'completed' ? '已完成' : currentProject.status === 'failed' ? '失败' : currentProject.status}
            </Tag>
          </Descriptions.Item>
          <Descriptions.Item label="创建时间">
            {new Date(currentProject.createdAt).toLocaleString()}
          </Descriptions.Item>
        </Descriptions>
      </Card>

      <Tabs defaultActiveKey="workspace" items={tabItems} />

      <StageDetailDrawer
        stage={selectedStage}
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
      />
    </div>
  );
};

export default ProjectDetail;
