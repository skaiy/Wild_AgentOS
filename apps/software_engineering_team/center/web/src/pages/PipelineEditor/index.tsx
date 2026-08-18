import React, { useCallback, useEffect, useState } from 'react';
import {
  ReactFlow,
  type Node,
  type Edge,
  Controls,
  Background,
  MiniMap,
  useNodesState,
  useEdgesState,
  addEdge,
  type Connection,
  BackgroundVariant,
  Panel,
  type NodeTypes,
  type EdgeTypes,
  Handle,
  Position,
  ConnectionMode,
  MarkerType,
  BaseEdge,
  getBezierPath,
  type EdgeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Card, Button, Space, Tag, Switch, InputNumber, Divider, Input, message, Select, Typography, Tooltip, Modal, Form, Radio } from 'antd';
import {
  SaveOutlined,
  PlayCircleOutlined,
  ApartmentOutlined,
  FileTextOutlined,
  CodeOutlined,
  BugOutlined,
  EyeOutlined,
  RocketOutlined,
  CloudUploadOutlined,
  ArrowLeftOutlined,
  PlusOutlined,
  DeleteOutlined,
  EditOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  MinusCircleOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { useProjectStore } from '@/stores';
import { pipelineApi } from '@/api';
import type { StageType, StageStatus } from '@/types';

const { Text } = Typography;

interface StageNodeData extends Record<string, unknown> {
  label: string;
  stageType: StageType;
  status: StageStatus;
  hasAIReview: boolean;
  hasHumanReview: boolean;
  timeoutSeconds: number;
  description?: string;
  retryCount?: number;
}

interface ConditionEdgeData extends Record<string, unknown> {
  label?: string;
  conditionType?: 'success' | 'failure' | 'always' | 'custom';
  conditionValue?: string;
  isBacktrack?: boolean;
}

const stageTypeConfig: Record<StageType, { color: string; icon: React.ReactNode; label: string }> = {
  requirement: { color: '#1890ff', icon: <FileTextOutlined />, label: '需求分析' },
  design: { color: '#722ed1', icon: <ApartmentOutlined />, label: '系统设计' },
  coding: { color: '#52c41a', icon: <CodeOutlined />, label: '编码实现' },
  testing: { color: '#fa8c16', icon: <BugOutlined />, label: '测试验证' },
  review: { color: '#eb2f96', icon: <EyeOutlined />, label: '代码审查' },
  cicd: { color: '#13c2c2', icon: <CloudUploadOutlined />, label: 'CI/CD' },
  deploy: { color: '#fa541c', icon: <RocketOutlined />, label: '部署发布' },
};

const statusColors: Record<StageStatus, string> = {
  pending: '#d9d9d9',
  running: '#1890ff',
  success: '#52c41a',
  failed: '#ff4d4f',
  reviewing: '#faad14',
  skipped: '#bfbfbf',
};

const conditionTypeConfig: Record<string, { color: string; label: string; icon: React.ReactNode }> = {
  success: { color: '#52c41a', label: '成功时', icon: <CheckCircleOutlined /> },
  failure: { color: '#ff4d4f', label: '失败时', icon: <CloseCircleOutlined /> },
  always: { color: '#1890ff', label: '总是', icon: <MinusCircleOutlined /> },
  custom: { color: '#722ed1', label: '自定义', icon: <SettingOutlined /> },
};

const ConditionEdge: React.FC<EdgeProps<Edge<ConditionEdgeData>>> = ({
  id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, style = {}, markerEnd, data, selected,
}) => {
  const [edgePath, labelX, labelY] = getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition });

  const edgeData = data as ConditionEdgeData | undefined;
  const isBacktrack = edgeData?.isBacktrack;
  const conditionType = edgeData?.conditionType || 'success';
  const conditionConfig = conditionTypeConfig[conditionType];

  const edgeStyle = {
    ...style,
    stroke: isBacktrack ? '#ff4d4f' : conditionConfig?.color || '#888',
    strokeWidth: selected ? 3 : 2,
    strokeDasharray: isBacktrack ? '5,5' : undefined,
  };

  return (
    <>
      <BaseEdge id={id} path={edgePath} markerEnd={markerEnd} style={edgeStyle} />
      {(edgeData?.label || conditionType !== 'success') && (
        <g transform={`translate(${labelX}, ${labelY})`}>
          <rect x={-30} y={-12} width={60} height={24} rx={4} fill="white" stroke={conditionConfig?.color || '#888'} strokeWidth={1} />
          <text x={0} y={4} textAnchor="middle" fontSize={11} fill={conditionConfig?.color || '#888'}>
            {edgeData?.label || conditionConfig?.label}
          </text>
        </g>
      )}
    </>
  );
};

const StageNode: React.FC<{ data: StageNodeData }> = ({ data }) => {
  const config = stageTypeConfig[data.stageType];
  const statusColor = statusColors[data.status];

  return (
    <div style={{ border: `2px solid ${config.color}`, borderRadius: 8, background: '#fff', minWidth: 180, boxShadow: '0 2px 8px rgba(0,0,0,0.1)' }}>
      <Handle type="target" position={Position.Left} id="target" style={{ background: config.color }} />
      <Handle type="target" position={Position.Top} id="target-top" style={{ background: config.color, left: '50%' }} />
      <div style={{ background: config.color, padding: '6px 12px', color: '#fff', display: 'flex', alignItems: 'center', gap: 6, borderTopLeftRadius: 6, borderTopRightRadius: 6 }}>
        {config.icon}
        <span style={{ fontWeight: 600, fontSize: 13 }}>{data.label}</span>
      </div>
      <div style={{ padding: '8px 12px' }}>
        <div style={{ marginBottom: 4 }}>
          <Tag color={statusColor}>{data.status}</Tag>
        </div>
        <div style={{ marginBottom: 4 }}>
          {data.hasAIReview && <Tag color="blue" style={{ fontSize: 11 }}>AI审查</Tag>}
          {data.hasHumanReview && <Tag color="orange" style={{ fontSize: 11 }}>人工审查</Tag>}
        </div>
        <div style={{ fontSize: 11, color: '#888' }}>
          <span>超时: {data.timeoutSeconds}s</span>
        </div>
      </div>
      <Handle type="source" position={Position.Right} id="source" style={{ background: config.color }} />
      <Handle type="source" position={Position.Bottom} id="source-bottom" style={{ background: config.color, left: '50%' }} />
    </div>
  );
};

const nodeTypes: NodeTypes = { stage: StageNode };
const edgeTypes: EdgeTypes = { condition: ConditionEdge };

const PipelineEditor: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const location = useLocation();
  const { currentProject, fetchProject } = useProjectStore();
  const isConfigMode = location.pathname.startsWith('/pipeline-config');
  const { message } = App.useApp();
  const [loading, setLoading] = React.useState(false);

  const initNodes: Node<StageNodeData>[] = [];
  const initEdges: Edge<ConditionEdgeData>[] = [];
  const [nodes, setNodes, onNodesChange] = useNodesState(initNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initEdges);
  const [selectedNode, setSelectedNode] = useState<Node<StageNodeData> | null>(null);
  const [selectedEdge, setSelectedEdge] = useState<Edge<ConditionEdgeData> | null>(null);
  const [running, setRunning] = useState(false);
  const [edgeModalVisible, setEdgeModalVisible] = useState(false);
  const [editingEdge, setEditingEdge] = useState<Edge<ConditionEdgeData> | null>(null);
  const [edgeForm] = Form.useForm();

  useEffect(() => {
    if (isConfigMode && id) {
      loadPipelineConfig(id);
    } else if (id) {
      fetchProject(id);
    }
  }, [id, fetchProject, isConfigMode]);

  const loadPipelineConfig = (configId: string) => {
    const saved = localStorage.getItem('pipeline-configs');
    if (saved) {
      try {
        const configs = JSON.parse(saved);
        const config = configs.find((c: any) => c.id === configId);
        if (config) {
          const loadedNodes: Node<StageNodeData>[] = (config.stages || []).map((stage: any, index: number) => ({
            id: stage.id,
            type: 'stage',
            position: { x: 50 + index * 280, y: 200 },
            data: {
              label: stage.name,
              stageType: stage.stageType as StageType,
              status: 'pending' as StageStatus,
              hasAIReview: stage.aiReview,
              hasHumanReview: stage.humanReview,
              timeoutSeconds: stage.timeout,
            },
          }));
          const loadedEdges: Edge<ConditionEdgeData>[] = (config.edges || []).map((edge: any) => ({
            id: edge.id,
            source: edge.source,
            target: edge.target,
            sourceHandle: edge.sourceHandle || 'source',
            targetHandle: edge.targetHandle || 'target',
            type: 'condition',
            animated: edge.animated || false,
            data: {
              label: edge.label,
              conditionType: edge.conditionType || 'success',
              isBacktrack: edge.isBacktrack || false,
            },
          }));
          setNodes(loadedNodes);
          setEdges(loadedEdges);
        }
      } catch {
        message.error('加载管线配置失败');
      }
    }
  };

  const onConnect = useCallback((params: Connection) => {
    const newEdge: Edge<ConditionEdgeData> = {
      ...params,
      id: `e-${params.source}-${params.target}-${Date.now()}`,
      type: 'condition',
      animated: true,
      data: { conditionType: 'success', isBacktrack: false },
    };
    setEdges((eds) => addEdge(newEdge, eds) as Edge<ConditionEdgeData>[]);
  }, [setEdges]);

  const onEdgeClick = useCallback((event: React.MouseEvent, edge: Edge) => {
    event.stopPropagation();
    setSelectedEdge(edge as Edge<ConditionEdgeData>);
    setSelectedNode(null);
  }, []);

  const onNodeClick = useCallback((_event: React.MouseEvent, node: Node) => {
    setSelectedNode(node as Node<StageNodeData>);
    setSelectedEdge(null);
  }, []);

  const onPaneClick = useCallback(() => {
    setSelectedNode(null);
    setSelectedEdge(null);
  }, []);

  const handleAddStage = (type: StageType) => {
    const config = stageTypeConfig[type];
    const newNode: Node<StageNodeData> = {
      id: `stage-${Date.now()}`,
      type: 'stage',
      position: { x: 100 + nodes.length * 280, y: 200 },
      data: {
        label: config.label,
        stageType: type,
        status: 'pending',
        hasAIReview: true,
        hasHumanReview: false,
        timeoutSeconds: 600,
      },
    };
    setNodes((nds) => [...nds, newNode]);
  };

  const handleDeleteNode = () => {
    if (selectedNode) {
      setNodes((nds) => nds.filter((n) => n.id !== selectedNode.id));
      setEdges((eds) => eds.filter((e) => e.source !== selectedNode.id && e.target !== selectedNode.id));
      setSelectedNode(null);
    }
  };

  const handleDeleteEdge = () => {
    if (selectedEdge) {
      setEdges((eds) => eds.filter((e) => e.id !== selectedEdge.id));
      setSelectedEdge(null);
    }
  };

  const handleEditEdge = () => {
    if (selectedEdge) {
      setEditingEdge(selectedEdge);
      edgeForm.setFieldsValue({
        label: selectedEdge.data?.label || '',
        conditionType: selectedEdge.data?.conditionType || 'success',
        isBacktrack: selectedEdge.data?.isBacktrack || false,
      });
      setEdgeModalVisible(true);
    }
  };

  const handleSaveEdge = async () => {
    try {
      const values = await edgeForm.validateFields();
      if (editingEdge) {
        setEdges((eds) =>
          eds.map((e) =>
            e.id === editingEdge.id
              ? { ...e, data: { ...e.data, label: values.label, conditionType: values.conditionType, isBacktrack: values.isBacktrack }, animated: values.conditionType === 'failure' || values.isBacktrack }
              : e
          )
        );
      }
      setEdgeModalVisible(false);
      setEditingEdge(null);
    } catch {}
  };

  const updateNodeData = (key: keyof StageNodeData, value: any) => {
    setNodes((nds) =>
      nds.map((n) => (n.id === selectedNode?.id ? { ...n, data: { ...n.data, [key]: value } } : n))
    );
    setSelectedNode((prev) => (prev ? { ...prev, data: { ...prev.data, [key]: value } } : null));
  };

  const handleSave = () => {
    if (isConfigMode && id) {
      const saved = localStorage.getItem('pipeline-configs');
      let configs = saved ? JSON.parse(saved) : [];
      const stages = nodes.map((n) => {
        const d = n.data as StageNodeData;
        return { id: n.id, name: d.label, type: d.stageType, timeout: d.timeoutSeconds, aiReview: d.hasAIReview, humanReview: d.hasHumanReview };
      });
      const edgesData = edges.map((e) => ({
        id: e.id, source: e.source, target: e.target, sourceHandle: e.sourceHandle, targetHandle: e.targetHandle,
        label: e.data?.label, conditionType: e.data?.conditionType, isBacktrack: e.data?.isBacktrack,
      }));
      configs = configs.map((c: any) => c.id === id ? { ...c, stages, edges: edgesData, updatedAt: new Date().toISOString() } : c);
      localStorage.setItem('pipeline-configs', JSON.stringify(configs));
      message.success('管线配置已保存');
    } else {
      localStorage.setItem(`pipeline-${id}`, JSON.stringify({ nodes, edges }));
      message.success('管线配置已保存');
    }
  };

  const handleRun = async () => {
    if (!id) return;
    setRunning(true);
    try {
      await pipelineApi.start({ project_name: `Pipeline-${id}` });
      message.success('管线已启动');
    } catch {
      message.error('管线启动失败');
    } finally {
      setRunning(false);
    }
  };

  return (
    <div style={{ height: 'calc(100vh - 120px)', display: 'flex', gap: 12 }}>
      <div style={{ flex: 1, position: 'relative' }}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onNodeClick={onNodeClick}
          onEdgeClick={onEdgeClick}
          onPaneClick={onPaneClick}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          connectionMode={ConnectionMode.Loose}
          fitView
          attributionPosition="bottom-left"
          deleteKeyCode={['Backspace', 'Delete']}
          multiSelectionKeyCode="Shift"
          snapToGrid
          snapGrid={[15, 15]}
        >
          <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
          <Controls />
          <MiniMap nodeColor={(node) => stageTypeConfig[(node.data as StageNodeData).stageType].color} maskColor="rgba(0,0,0,0.1)" />
          <Panel position="top-right">
            <Space>
              <Button icon={<ArrowLeftOutlined />} onClick={() => isConfigMode ? navigate('/pipeline-config') : navigate(`/projects/${id}`)}>返回</Button>
              <Button icon={<SaveOutlined />} onClick={handleSave}>保存</Button>
              {!isConfigMode && <Button type="primary" icon={<PlayCircleOutlined />} onClick={handleRun} loading={running}>运行</Button>}
            </Space>
          </Panel>
        </ReactFlow>
      </div>

      <div style={{ width: 260, display: 'flex', flexDirection: 'column', gap: 12 }}>
        <Card title="阶段面板" size="small">
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
            {Object.entries(stageTypeConfig).map(([type, config]) => (
              <Tooltip key={type} title={`点击添加 ${config.label}`}>
                <div
                  style={{ border: `1px solid ${config.color}`, borderRadius: 6, padding: 8, cursor: 'pointer', textAlign: 'center' }}
                  onClick={() => handleAddStage(type as StageType)}
                >
                  <div style={{ color: config.color, fontSize: 16 }}>{config.icon}</div>
                  <div style={{ fontSize: 11, marginTop: 2 }}>{config.label}</div>
                </div>
              </Tooltip>
            ))}
          </div>
          <Divider style={{ margin: '8px 0' }} />
          <Text type="secondary" style={{ fontSize: 12 }}>拖拽节点可移动，从节点右侧连接点拖拽到另一节点左侧可连线</Text>
        </Card>

        {selectedNode && (
          <Card title="节点属性" size="small" extra={<Button size="small" danger icon={<DeleteOutlined />} onClick={handleDeleteNode}>删除</Button>}>
            <div>
              <div style={{ marginBottom: 8 }}>
                <label style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>阶段名称</label>
                <Input value={selectedNode.data.label} onChange={(e) => updateNodeData('label', e.target.value)} size="small" />
              </div>
              <div style={{ marginBottom: 8 }}>
                <label style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>AI 审查</label>
                <Switch checked={selectedNode.data.hasAIReview} onChange={(checked) => updateNodeData('hasAIReview', checked)} />
              </div>
              <div style={{ marginBottom: 8 }}>
                <label style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>人工审查</label>
                <Switch checked={selectedNode.data.hasHumanReview} onChange={(checked) => updateNodeData('hasHumanReview', checked)} />
              </div>
              <div style={{ marginBottom: 8 }}>
                <label style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>超时 (秒)</label>
                <InputNumber value={selectedNode.data.timeoutSeconds} onChange={(value) => updateNodeData('timeoutSeconds', value || 600)} min={60} max={86400} style={{ width: '100%' }} size="small" />
              </div>
            </div>
          </Card>
        )}

        {selectedEdge && (
          <Card title="连线属性" size="small" extra={<Space><Button size="small" icon={<EditOutlined />} onClick={handleEditEdge}>编辑</Button><Button size="small" danger icon={<DeleteOutlined />} onClick={handleDeleteEdge}>删除</Button></Space>}>
            <div>
              <div style={{ marginBottom: 8 }}>
                <label style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>条件类型</label>
                <Tag color={conditionTypeConfig[selectedEdge.data?.conditionType || 'success']?.color}>
                  {conditionTypeConfig[selectedEdge.data?.conditionType || 'success']?.label}
                </Tag>
              </div>
              <div>
                <label style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>回退</label>
                <Tag color={selectedEdge.data?.isBacktrack ? 'red' : 'default'}>{selectedEdge.data?.isBacktrack ? '是' : '否'}</Tag>
              </div>
            </div>
          </Card>
        )}
      </div>

      <Modal title="编辑连线" open={edgeModalVisible} onOk={handleSaveEdge} onCancel={() => { setEdgeModalVisible(false); setEditingEdge(null); }} okText="保存" cancelText="取消">
        <Form form={edgeForm} layout="vertical">
          <Form.Item name="label" label="连线标签"><Input placeholder="可选" /></Form.Item>
          <Form.Item name="conditionType" label="条件类型" rules={[{ required: true }]}>
            <Radio.Group>
              <Radio.Button value="success">成功时</Radio.Button>
              <Radio.Button value="failure">失败时</Radio.Button>
              <Radio.Button value="always">总是</Radio.Button>
              <Radio.Button value="custom">自定义</Radio.Button>
            </Radio.Group>
          </Form.Item>
          <Form.Item name="isBacktrack" label="回退连线" valuePropName="checked"><Switch /></Form.Item>
        </Form>
      </Modal>
    </div>
  );
};

export default PipelineEditor;