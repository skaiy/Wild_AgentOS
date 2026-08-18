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
  getBezierPath,
  type EdgeProps,
  useNodes,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Card, Button, Space, Tag, Switch, InputNumber, Divider, Input, message, Typography, Tooltip, Modal, Form, Radio } from 'antd';
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
  DeleteOutlined,
  EditOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  MinusCircleOutlined,
  SettingOutlined,
  RollbackOutlined,
} from '@ant-design/icons';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { usePipelineStore, useProjectStore } from '@/stores';
import type { StageType, StageStatus } from '@/types';
import styles from './PipelineEditor.module.css';

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
  retryInterval?: number;
  environment?: string;
}

interface ConditionEdgeData extends Record<string, unknown> {
  label?: string;
  conditionType?: 'success' | 'failure' | 'always' | 'custom';
  conditionValue?: string;
  isBacktrack?: boolean;
  isReverseConnection?: boolean;
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

const defaultStageConfig = { color: '#8c8c8c', icon: <FileTextOutlined />, label: '未知阶段' };

const statusColors: Record<StageStatus, string> = {
  pending: '#d9d9d9',
  running: '#1890ff',
  success: '#52c41a',
  failed: '#ff4d4f',
  reviewing: '#faad14',
  skipped: '#bfbfbf',
};

const conditionTypeConfig: Record<string, { color: string; label: string; icon: React.ReactNode; desc: string }> = {
  success: { color: '#52c41a', label: '成功时', icon: <CheckCircleOutlined />, desc: '上一阶段成功时迁移' },
  failure: { color: '#ff4d4f', label: '失败时', icon: <CloseCircleOutlined />, desc: '上一阶段失败时迁移' },
  always: { color: '#1890ff', label: '总是', icon: <MinusCircleOutlined />, desc: '无论成功失败都迁移' },
  custom: { color: '#722ed1', label: '自定义', icon: <SettingOutlined />, desc: '自定义条件表达式' },
};

const ConditionEdge: React.FC<EdgeProps> = ({
  id,
  source,
  target,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style = {},
  data,
  selected,
}) => {
  const nodes = useNodes();
  
  const sourceNode = nodes.find(n => n.id === source);
  const targetNode = nodes.find(n => n.id === target);
  
  const autoDetectedReverse = sourceNode && targetNode 
    ? sourceNode.position.x > targetNode.position.x 
    : false;

  let [edgePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
  });

  const coordPattern = /([-\d.]+)[,\s]+([-\d.]+)/g;
  const coords: [number, number][] = [];
  let match;
  while ((match = coordPattern.exec(edgePath)) !== null) {
    coords.push([parseFloat(match[1]), parseFloat(match[2])]);
  }

  const pathEndX = coords.length > 0 ? coords[coords.length - 1][0] : targetX;
  const pathEndY = coords.length > 0 ? coords[coords.length - 1][1] : targetY;
  const pathPrevX = coords.length > 1 ? coords[coords.length - 2][0] : sourceX;
  const pathPrevY = coords.length > 1 ? coords[coords.length - 2][1] : sourceY;

  const pathStartX = coords.length > 0 ? coords[0][0] : sourceX;
  const pathStartY = coords.length > 0 ? coords[0][1] : sourceY;

  const edgeData = data as ConditionEdgeData | undefined;
  const isBacktrack = edgeData?.isBacktrack;
  const userOverrideReverse = edgeData?.isReverseConnection;
  
  const isReverseConnection = userOverrideReverse !== undefined 
    ? userOverrideReverse 
    : autoDetectedReverse;

  let arrowTipX: number;
  let arrowTipY: number;
  let arrowBaseX: number;
  let arrowBaseY: number;

  if (isReverseConnection === true) {
    const distEndToSource = Math.hypot(pathEndX - sourceX, pathEndY - sourceY);
    const distStartToSource = Math.hypot(pathStartX - sourceX, pathStartY - sourceY);
    const pathEndIsRealTarget = distEndToSource <= distStartToSource;

    if (pathEndIsRealTarget) {
      arrowTipX = pathEndX;
      arrowTipY = pathEndY;
      arrowBaseX = pathPrevX;
      arrowBaseY = pathPrevY;
    } else {
      arrowTipX = pathStartX;
      arrowTipY = pathStartY;
      arrowBaseX = coords.length > 1 ? coords[1][0] : pathEndX;
      arrowBaseY = coords.length > 1 ? coords[1][1] : pathEndY;
    }
  } else {
    const distEndToTarget = Math.hypot(pathEndX - targetX, pathEndY - targetY);
    const distStartToTarget = Math.hypot(pathStartX - targetX, pathStartY - targetY);
    const pathEndIsTarget = distEndToTarget <= distStartToTarget;

    if (pathEndIsTarget) {
      arrowTipX = pathEndX;
      arrowTipY = pathEndY;
      arrowBaseX = pathPrevX;
      arrowBaseY = pathPrevY;
    } else {
      arrowTipX = pathStartX;
      arrowTipY = pathStartY;
      arrowBaseX = coords.length > 1 ? coords[1][0] : pathEndX;
      arrowBaseY = coords.length > 1 ? coords[1][1] : pathEndY;
    }
  }

  const conditionType = edgeData?.conditionType || 'success';
  const conditionConfig = conditionTypeConfig[conditionType];

  const edgeColor = isBacktrack ? '#ff4d4f' : conditionConfig?.color || '#888';

  const labelText = edgeData?.label || conditionConfig?.label || '成功时';

  const arrowSize = 8;
  const dx = arrowTipX - arrowBaseX;
  const dy = arrowTipY - arrowBaseY;
  const len = Math.sqrt(dx * dx + dy * dy) || 1;
  const ux = dx / len;
  const uy = dy / len;
  const baseX = arrowTipX - arrowSize * ux;
  const baseY = arrowTipY - arrowSize * uy;
  const leftX = baseX - arrowSize * 0.5 * (-uy);
  const leftY = baseY - arrowSize * 0.5 * ux;
  const rightX = baseX + arrowSize * 0.5 * (-uy);
  const rightY = baseY + arrowSize * 0.5 * ux;

  return (
    <>
      <path
        id={id}
        d={edgePath}
        fill="none"
        stroke={edgeColor}
        strokeWidth={selected ? 3 : 2}
        className="react-flow__edge-path"
      />
      <polygon
        points={`${arrowTipX},${arrowTipY} ${leftX},${leftY} ${rightX},${rightY}`}
        fill={edgeColor}
        stroke={edgeColor}
        strokeWidth={1}
      />
      <g transform={`translate(${labelX}, ${labelY})`}>
        <rect
          x={-32}
          y={-14}
          width={64}
          height={28}
          rx={6}
          fill="white"
          stroke={edgeColor}
          strokeWidth={1.5}
          opacity={0.95}
        />
        {isBacktrack && (
          <text x={-24} y={4} fontSize={10} fill="#ff4d4f">↩</text>
        )}
        <text
          x={isBacktrack ? 2 : 0}
          y={4}
          textAnchor="middle"
          fontSize={10}
          fontWeight={500}
          fill={edgeColor}
        >
          {labelText}
        </text>
      </g>
    </>
  );
};

const StageNode: React.FC<{ data: StageNodeData }> = ({ data }) => {
  const config = stageTypeConfig[data.stageType] || defaultStageConfig;
  const statusColor = statusColors[data.status] || '#d9d9d9';

  return (
    <div
      className={styles.stageNode}
      style={{ borderColor: config.color, borderTopColor: config.color }}
    >
      <Handle
        type="target"
        position={Position.Left}
        id="target"
        className={styles.handle}
        style={{ background: config.color }}
      />
      <Handle
        type="target"
        position={Position.Top}
        id="target-top"
        className={styles.handle}
        style={{ background: config.color, left: '50%' }}
      />
      <div className={styles.nodeHeader} style={{ backgroundColor: config.color }}>
        {config.icon}
        <span className={styles.nodeTitle}>{data.label}</span>
      </div>
      <div className={styles.nodeContent}>
        <div className={styles.nodeStatus}>
          <Tag color={statusColor}>{data.status}</Tag>
        </div>
        <div className={styles.nodeFlags}>
          {data.hasAIReview && <Tag color="blue">AI审查</Tag>}
          {data.hasHumanReview && <Tag color="orange">人工审查</Tag>}
        </div>
        <div className={styles.nodeMeta}>
          <span>超时: {data.timeoutSeconds}s</span>
          {data.retryCount && data.retryCount > 0 && (
            <span style={{ marginLeft: 8 }}>重试: {data.retryCount}次</span>
          )}
        </div>
      </div>
      <Handle
        type="source"
        position={Position.Right}
        id="source"
        className={styles.handle}
        style={{ background: config.color }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="source-bottom"
        className={styles.handle}
        style={{ background: config.color, left: '50%' }}
      />
    </div>
  );
};

const nodeTypes: NodeTypes = {
  stage: StageNode,
};

const edgeTypes: EdgeTypes = {
  condition: ConditionEdge,
};

const defaultEdgeOptions = {
  type: 'condition' as const,
};

const PipelineEditor: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const location = useLocation();
  const { currentProject, fetchProject } = useProjectStore();
  const { stages, taskMeta, startPipeline, fetchPipelineStatus } = usePipelineStore();

  const isConfigMode = location.pathname.startsWith('/pipeline-config');
  
  const [nodes, setNodes, onNodesChange] = useNodesState([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);
  const [selectedNode, setSelectedNode] = useState<Node<StageNodeData> | null>(null);
  const [selectedEdge, setSelectedEdge] = useState<Edge<ConditionEdgeData> | null>(null);
  const [running, setRunning] = useState(false);
  const [configName, setConfigName] = useState('');
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
          setConfigName(config.name);
          const loadedNodes: Node<StageNodeData>[] = config.stages.map(
            (stage: any, index: number) => ({
              id: stage.id,
              type: 'stage',
              position: { x: 50 + index * 280, y: 200 },
              data: {
                label: stage.name,
                stageType: (stage.stageType || stage.type) as StageType,
                status: 'pending' as StageStatus,
                hasAIReview: stage.aiReview,
                hasHumanReview: stage.humanReview,
                timeoutSeconds: stage.timeout,
              },
            })
          );
          const loadedEdges: Edge<ConditionEdgeData>[] = (config.edges && config.edges.length > 0)
            ? config.edges.map((edge: any) => ({
                id: edge.id,
                source: edge.source,
                target: edge.target,
                sourceHandle: edge.sourceHandle || undefined,
                targetHandle: edge.targetHandle || undefined,
                type: 'condition',
                animated: false,
                data: {
                  label: edge.label,
                  conditionType: edge.conditionType || 'success',
                  conditionValue: edge.conditionValue,
                  isBacktrack: edge.isBacktrack || false,
                },
              }))
            : loadedNodes.slice(0, -1).map((node, index) => ({
                id: `e-${node.id}-${loadedNodes[index + 1].id}`,
                source: node.id,
                target: loadedNodes[index + 1].id,
                type: 'condition' as const,
                animated: false,
                data: {
                  conditionType: 'success' as const,
                  isBacktrack: false,
                },
              }));
          setNodes(loadedNodes);
          setEdges(loadedEdges);
        }
      } catch (error) {
        message.error('加载管线配置失败');
      }
    }
  };

  useEffect(() => {
    if (stages.length > 0) {
      const flowNodes: Node<StageNodeData>[] = stages.map((stage, index: number) => {
        const s = stage as unknown as Record<string, unknown>;
        return {
          id: (s.stageId || s.stage_id) as string,
          type: 'stage',
          position: { x: 50 + index * 280, y: 200 },
          data: {
            label: s.name as string,
            stageType: (s.stageType || s.stage_type || 'requirement') as StageType,
            status: (s.status || 'pending') as StageStatus,
            hasAIReview: false,
            hasHumanReview: false,
            timeoutSeconds: 600,
          },
        };
      });
      const flowEdges: Edge<ConditionEdgeData>[] = stages.slice(0, -1).map((stage, index: number) => {
        const s = stage as unknown as Record<string, unknown>;
        const nextS = stages[index + 1] as unknown as Record<string, unknown>;
        return {
          id: `e-${s.stageId || s.stage_id}-${nextS.stageId || nextS.stage_id}`,
          source: (s.stageId || s.stage_id) as string,
          target: (nextS.stageId || nextS.stage_id) as string,
          type: 'condition',
          animated: false,
          data: {
            conditionType: 'success' as const,
            isBacktrack: false,
          },
        };
      });
      setNodes(flowNodes);
      setEdges(flowEdges);
    }
  }, [stages, setNodes, setEdges]);

  const onConnect = useCallback(
    (params: Connection) => {
      const newEdge: Edge<ConditionEdgeData> = {
        ...params,
        id: `e-${params.source}-${params.target}-${Date.now()}`,
        type: 'condition',
        animated: false,
        data: {
          conditionType: 'success',
          isBacktrack: false,
          isReverseConnection: false,
        },
      };
      setEdges((eds) => addEdge(newEdge, eds));
      message.success('连线已添加，双击连线可编辑迁移条件和方向');
    },
    [setEdges]
  );

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

  const onEdgeDoubleClick = useCallback((event: React.MouseEvent, edge: Edge) => {
    event.stopPropagation();
    const e = edge as Edge<ConditionEdgeData>;
    setEditingEdge(e);
    edgeForm.setFieldsValue({
      label: e.data?.label || '',
      conditionType: e.data?.conditionType || 'success',
      conditionValue: e.data?.conditionValue || '',
      isBacktrack: e.data?.isBacktrack || false,
      isReverseConnection: e.data?.isReverseConnection || false,
    });
    setEdgeModalVisible(true);
  }, [edgeForm]);

  const handleAddStage = (type: StageType) => {
    const config = stageTypeConfig[type] || defaultStageConfig;
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
    message.success(`已添加 ${config.label} 阶段`);
  };

  const handleDeleteNode = () => {
    if (selectedNode) {
      setNodes((nds) => nds.filter((n) => n.id !== selectedNode.id));
      setEdges((eds) => eds.filter((e) => e.source !== selectedNode.id && e.target !== selectedNode.id));
      setSelectedNode(null);
      message.success('节点已删除');
    }
  };

  const handleDeleteEdge = () => {
    if (selectedEdge) {
      setEdges((eds) => eds.filter((e) => e.id !== selectedEdge.id));
      setSelectedEdge(null);
      message.success('连线已删除');
    }
  };

  const handleEditEdge = () => {
    if (selectedEdge) {
      setEditingEdge(selectedEdge);
      edgeForm.setFieldsValue({
        label: selectedEdge.data?.label || '',
        conditionType: selectedEdge.data?.conditionType || 'success',
        conditionValue: selectedEdge.data?.conditionValue || '',
        isBacktrack: selectedEdge.data?.isBacktrack || false,
      });
      setEdgeModalVisible(true);
    }
  };

  const handleSaveEdge = async () => {
    try {
      const values = await edgeForm.validateFields();
      if (editingEdge) {
        const conditionType = values.conditionType || 'success';
        const isBacktrack = values.isBacktrack || false;
        const isReverseConnection = values.isReverseConnection || false;

        setEdges((eds) =>
          eds.map((e) =>
            e.id === editingEdge.id
              ? {
                  ...e,
                  data: {
                    ...e.data,
                    label: values.label,
                    conditionType,
                    conditionValue: values.conditionValue,
                    isBacktrack,
                    isReverseConnection,
                  },
                  animated: false,
                }
              : e
          )
        );
        message.success('连线配置已更新');
      }
      setEdgeModalVisible(false);
      setEditingEdge(null);
    } catch (error) {
      console.error('Validation failed:', error);
    }
  };

  const updateNodeData = (key: keyof StageNodeData, value: any) => {
    setNodes((nds) =>
      nds.map((n) =>
        n.id === selectedNode?.id
          ? { ...n, data: { ...n.data, [key]: value } }
          : n
      )
    );
    setSelectedNode((prev) =>
      prev ? { ...prev, data: { ...prev.data, [key]: value } } : null
    );
  };

  const handleSave = () => {
    if (isConfigMode && id) {
      const saved = localStorage.getItem('pipeline-configs');
      let configs = saved ? JSON.parse(saved) : [];
      const stagesData = nodes.map((n) => {
        const d = n.data as StageNodeData;
        return {
          id: n.id,
          name: d.label,
          type: d.stageType,
          timeout: d.timeoutSeconds,
          aiReview: d.hasAIReview,
          humanReview: d.hasHumanReview,
        };
      });
      
      const edgesData = edges.map((e) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        sourceHandle: e.sourceHandle,
        targetHandle: e.targetHandle,
        label: e.data?.label,
        conditionType: e.data?.conditionType,
        conditionValue: e.data?.conditionValue,
        isBacktrack: e.data?.isBacktrack,
      }));
      
      configs = configs.map((c: any) => 
        c.id === id 
          ? { ...c, stages: stagesData, edges: edgesData, updatedAt: new Date().toISOString() }
          : c
      );
      localStorage.setItem('pipeline-configs', JSON.stringify(configs));
      message.success('管线配置已保存');
    } else {
      const pipelineData = {
        nodes: nodes.map((n) => ({
          id: n.id,
          label: (n.data as StageNodeData).label,
          stageType: (n.data as StageNodeData).stageType,
          hasAIReview: (n.data as StageNodeData).hasAIReview,
          hasHumanReview: (n.data as StageNodeData).hasHumanReview,
          timeoutSeconds: (n.data as StageNodeData).timeoutSeconds,
        })),
        edges: edges.map((e) => ({
          source: e.source,
          target: e.target,
          conditionType: e.data?.conditionType,
          isBacktrack: e.data?.isBacktrack,
        })),
      };
      localStorage.setItem(`pipeline-${id}`, JSON.stringify(pipelineData));
      message.success('管线配置已保存');
    }
  };

  const handleRun = async () => {
    if (!id) return;
    setRunning(true);
    try {
      await startPipeline(id, `Pipeline-${id}`);
      message.success('管线已启动');
    } catch (error) {
      message.error('管线启动失败');
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className={styles.container}>
      <div className={styles.editorLayout}>
        <div className={styles.canvasArea}>
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onNodeClick={onNodeClick}
            onEdgeClick={onEdgeClick}
            onEdgeDoubleClick={onEdgeDoubleClick}
            onPaneClick={onPaneClick}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            defaultEdgeOptions={defaultEdgeOptions}
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
            <MiniMap
              nodeColor={(node) => (stageTypeConfig[(node.data as StageNodeData).stageType] || defaultStageConfig).color}
              maskColor="rgba(0,0,0,0.1)"
            />
            <Panel position="top-right">
              <Space>
                <Button 
                  icon={<ArrowLeftOutlined />} 
                  onClick={() => isConfigMode ? navigate('/pipeline-config') : navigate(`/projects/${id}`)}
                >
                  返回
                </Button>
                <Button icon={<SaveOutlined />} onClick={handleSave}>
                  保存
                </Button>
                {!isConfigMode && (
                  <Button type="primary" icon={<PlayCircleOutlined />} onClick={handleRun} loading={running}>
                    运行
                  </Button>
                )}
              </Space>
            </Panel>
          </ReactFlow>
        </div>

        <div className={styles.sidePanel}>
          <Card title="阶段面板" size="small" className={styles.paletteCard}>
            <div className={styles.paletteGrid}>
              {Object.entries(stageTypeConfig).map(([type, config]) => (
                <Tooltip key={type} title={`点击添加 ${config.label}`}>
                  <div
                    className={styles.paletteItem}
                    style={{ borderColor: config.color }}
                    onClick={() => handleAddStage(type as StageType)}
                  >
                    <div className={styles.paletteIcon} style={{ color: config.color }}>
                      {config.icon}
                    </div>
                    <span className={styles.paletteLabel}>{config.label}</span>
                  </div>
                </Tooltip>
              ))}
            </div>
            <Divider style={{ margin: '12px 0' }} />
            <div className={styles.helpText}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                提示：从节点右侧连接点拖拽到另一节点左侧可连线；双击连线可编辑迁移条件
              </Text>
            </div>
          </Card>

          {selectedNode && (
            <Card
              title="节点属性"
              size="small"
              className={styles.propertyCard}
              extra={
                <Button size="small" danger icon={<DeleteOutlined />} onClick={handleDeleteNode}>
                  删除
                </Button>
              }
            >
              <div className={styles.propertyForm}>
                <div className={styles.propertySection}>
                  <div className={styles.propertySectionTitle}>基本信息</div>
                  <div className={styles.formItem}>
                    <label>阶段名称</label>
                    <Input
                      value={selectedNode.data.label}
                      onChange={(e) => updateNodeData('label', e.target.value)}
                      size="small"
                    />
                  </div>
                  <div className={styles.formItem}>
                    <label>阶段类型</label>
                    <Tag color={(stageTypeConfig[selectedNode.data.stageType] || defaultStageConfig).color}>
                      {(stageTypeConfig[selectedNode.data.stageType] || defaultStageConfig).label}
                    </Tag>
                  </div>
                </div>

                <Divider style={{ margin: '12px 0' }} />
                
                <div className={styles.propertySection}>
                  <div className={styles.propertySectionTitle}>审查配置</div>
                  <div className={styles.formItem}>
                    <label>AI 审查</label>
                    <Switch
                      checked={selectedNode.data.hasAIReview}
                      onChange={(checked) => updateNodeData('hasAIReview', checked)}
                    />
                  </div>
                  <div className={styles.formItem}>
                    <label>人工审查</label>
                    <Switch
                      checked={selectedNode.data.hasHumanReview}
                      onChange={(checked) => updateNodeData('hasHumanReview', checked)}
                    />
                  </div>
                </div>

                <Divider style={{ margin: '12px 0' }} />
                
                <div className={styles.propertySection}>
                  <div className={styles.propertySectionTitle}>执行配置</div>
                  <div className={styles.formItem}>
                    <label>超时时间 (秒)</label>
                    <InputNumber
                      value={selectedNode.data.timeoutSeconds}
                      onChange={(value) => updateNodeData('timeoutSeconds', value || 600)}
                      min={60}
                      max={86400}
                      style={{ width: '100%' }}
                      size="small"
                    />
                  </div>
                  <div className={styles.formItem}>
                    <label>重试次数</label>
                    <InputNumber
                      value={selectedNode.data.retryCount || 0}
                      onChange={(value) => updateNodeData('retryCount', value || 0)}
                      min={0}
                      max={10}
                      style={{ width: '100%' }}
                      size="small"
                    />
                  </div>
                </div>
              </div>
            </Card>
          )}

          {selectedEdge && (
            <Card
              title="连线属性"
              size="small"
              className={styles.propertyCard}
              extra={
                <Space>
                  <Button size="small" type="primary" icon={<EditOutlined />} onClick={handleEditEdge}>
                    编辑条件
                  </Button>
                  <Button size="small" danger icon={<DeleteOutlined />} onClick={handleDeleteEdge}>
                    删除
                  </Button>
                </Space>
              }
            >
              <div className={styles.propertyForm}>
                <div className={styles.propertySection}>
                  <div className={styles.propertySectionTitle}>迁移条件</div>
                  <div className={styles.formItem}>
                    <label>条件类型</label>
                    <Tag color={conditionTypeConfig[selectedEdge.data?.conditionType || 'success']?.color}>
                      {conditionTypeConfig[selectedEdge.data?.conditionType || 'success']?.icon}
                      {' '}
                      {conditionTypeConfig[selectedEdge.data?.conditionType || 'success']?.label}
                    </Tag>
                  </div>
                  <div style={{ fontSize: 11, color: '#999', marginTop: 4 }}>
                    {conditionTypeConfig[selectedEdge.data?.conditionType || 'success']?.desc}
                  </div>
                  {selectedEdge.data?.conditionType === 'custom' && selectedEdge.data?.conditionValue && (
                    <div className={styles.formItem} style={{ marginTop: 8 }}>
                      <label>条件表达式</label>
                      <Text code style={{ fontSize: 11 }}>{selectedEdge.data.conditionValue}</Text>
                    </div>
                  )}
                </div>

                <Divider style={{ margin: '12px 0' }} />

                <div className={styles.propertySection}>
                  <div className={styles.propertySectionTitle}>回退配置</div>
                  <div className={styles.formItem}>
                    <label>回退连线</label>
                    <Tag color={selectedEdge.data?.isBacktrack ? 'red' : 'default'} icon={selectedEdge.data?.isBacktrack ? <RollbackOutlined /> : undefined}>
                      {selectedEdge.data?.isBacktrack ? '回退边' : '正向边'}
                    </Tag>
                  </div>
                  {selectedEdge.data?.isBacktrack && (
                    <div style={{ fontSize: 11, color: '#ff4d4f', marginTop: 4 }}>
                      回退连线：当目标阶段失败时，流程将回退到源阶段重新执行
                    </div>
                  )}
                </div>

                {selectedEdge.data?.label && (
                  <>
                    <Divider style={{ margin: '12px 0' }} />
                    <div className={styles.formItem}>
                      <label>连线标签</label>
                      <Text>{selectedEdge.data.label}</Text>
                    </div>
                  </>
                )}
              </div>
            </Card>
          )}
        </div>
      </div>

      <Modal
        title="编辑迁移条件"
        open={edgeModalVisible}
        onOk={handleSaveEdge}
        onCancel={() => {
          setEdgeModalVisible(false);
          setEditingEdge(null);
        }}
        okText="保存"
        cancelText="取消"
        width={520}
      >
        <Form form={edgeForm} layout="vertical">
          <div style={{ background: '#f6f6f6', padding: 12, borderRadius: 8, marginBottom: 16 }}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              💡 迁移条件定义了从一个阶段到下一个阶段的流转规则。例如"成功时"表示上一阶段成功后才流转到下一阶段。
            </Text>
          </div>

          <Form.Item name="conditionType" label="迁移条件" rules={[{ required: true, message: '请选择迁移条件' }]}>
            <Radio.Group>
              <Space direction="vertical">
                <Radio value="success">
                  <CheckCircleOutlined style={{ color: '#52c41a' }} /> 成功时 — 上一阶段成功时迁移
                </Radio>
                <Radio value="failure">
                  <CloseCircleOutlined style={{ color: '#ff4d4f' }} /> 失败时 — 上一阶段失败时迁移
                </Radio>
                <Radio value="always">
                  <MinusCircleOutlined style={{ color: '#1890ff' }} /> 总是 — 无论成功失败都迁移
                </Radio>
                <Radio value="custom">
                  <SettingOutlined style={{ color: '#722ed1' }} /> 自定义 — 使用条件表达式
                </Radio>
              </Space>
            </Radio.Group>
          </Form.Item>

          <Form.Item noStyle shouldUpdate>
            {({ getFieldValue }) => 
              getFieldValue('conditionType') === 'custom' && (
                <Form.Item name="conditionValue" label="自定义条件表达式" rules={[{ required: true, message: '请输入条件表达式' }]}>
                  <Input placeholder="例如: output.testCoverage > 80" />
                </Form.Item>
              )
            }
          </Form.Item>

          <Divider />

          <Form.Item name="isBacktrack" label="回退连线" valuePropName="checked">
            <Switch checkedChildren="回退" unCheckedChildren="正向" />
          </Form.Item>

          <Form.Item name="isReverseConnection" label="反转箭头方向" valuePropName="checked" extra="如果箭头方向不正确，请勾选此项">
            <Switch checkedChildren="反转" unCheckedChildren="正常" />
          </Form.Item>

          <Form.Item noStyle shouldUpdate>
            {({ getFieldValue }) => 
              getFieldValue('isBacktrack') && (
                <div style={{ background: '#fff2f0', padding: 12, borderRadius: 8, marginBottom: 16, border: '1px solid #ffccc7' }}>
                  <Text type="danger" style={{ fontSize: 12 }}>
                    ⚠️ 回退连线表示当目标阶段失败时，流程将回退到源阶段重新执行。回退边将以红色虚线显示。
                  </Text>
                </div>
              )
            }
          </Form.Item>

          <Form.Item name="label" label="连线标签（可选）">
            <Input placeholder="可选，显示在连线上的标签文字" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};

export default PipelineEditor;
