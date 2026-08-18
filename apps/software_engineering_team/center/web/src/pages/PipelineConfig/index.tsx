import React, { useState, useEffect } from 'react';
import {
  Card, Table, Button, Space, Modal, Form, Input, Select, Tag, Popconfirm,
  Typography, Row, Col, Statistic, Divider, List, Avatar, Tooltip, Empty, Tabs,
  Timeline, Badge, Drawer, Descriptions, Alert, App,
} from 'antd';
import {
  PlusOutlined, EditOutlined, DeleteOutlined, CopyOutlined, PlayCircleOutlined,
  AppstoreOutlined, ClockCircleOutlined, CheckCircleOutlined, SettingOutlined,
  EyeOutlined, FileTextOutlined, CodeOutlined, BugOutlined, RocketOutlined,
  CloudUploadOutlined, ApartmentOutlined, ArrowRightOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import type { ColumnsType } from 'antd/es/table';

const { Title, Text, Paragraph } = Typography;
const { Option } = Select;

interface StageConfig {
  id: string;
  name: string;
  type: 'requirement' | 'design' | 'coding' | 'testing' | 'review' | 'cicd' | 'deploy';
  timeout: number;
  aiReview: boolean;
  humanReview: boolean;
  description?: string;
  retryCount?: number;
}

interface PipelineConfigItem {
  id: string;
  name: string;
  description: string;
  stages: StageConfig[];
  createdAt: string;
  updatedAt: string;
  isTemplate: boolean;
  usageCount: number;
  tags?: string[];
  category?: string;
}

interface PipelineTemplate {
  id: string;
  name: string;
  description: string;
  category: 'basic' | 'advanced' | 'enterprise';
  stages: StageConfig[];
  icon: string;
  features?: string[];
}

const stageTypeConfig: Record<string, { color: string; label: string; icon: React.ReactNode }> = {
  requirement: { color: '#1890ff', label: '需求分析', icon: <FileTextOutlined /> },
  design: { color: '#722ed1', label: '系统设计', icon: <ApartmentOutlined /> },
  coding: { color: '#52c41a', label: '编码实现', icon: <CodeOutlined /> },
  testing: { color: '#fa8c16', label: '测试验证', icon: <BugOutlined /> },
  review: { color: '#eb2f96', label: '代码审查', icon: <EyeOutlined /> },
  cicd: { color: '#13c2c2', label: 'CI/CD', icon: <CloudUploadOutlined /> },
  deploy: { color: '#fa541c', label: '部署发布', icon: <RocketOutlined /> },
};

const defaultTemplates: PipelineTemplate[] = [
  {
    id: 'template-basic', name: '基础开发流程', description: '适用于小型项目的基础开发流程',
    category: 'basic', icon: '🚀', features: ['快速启动', '轻量级', '适合MVP'],
    stages: [
      { id: 'req', name: '需求分析', type: 'requirement', timeout: 300, aiReview: true, humanReview: false },
      { id: 'code', name: '编码实现', type: 'coding', timeout: 1800, aiReview: false, humanReview: false },
      { id: 'test', name: '测试验证', type: 'testing', timeout: 600, aiReview: true, humanReview: false },
    ],
  },
  {
    id: 'template-standard', name: '标准开发流程', description: '适用于中型项目的标准开发流程',
    category: 'basic', icon: '📋', features: ['完整流程', '双重审查', '质量保证'],
    stages: [
      { id: 'req', name: '需求分析', type: 'requirement', timeout: 600, aiReview: true, humanReview: true },
      { id: 'design', name: '系统设计', type: 'design', timeout: 900, aiReview: true, humanReview: true },
      { id: 'code', name: '编码实现', type: 'coding', timeout: 3600, aiReview: false, humanReview: false },
      { id: 'review', name: '代码审查', type: 'review', timeout: 600, aiReview: true, humanReview: true },
      { id: 'test', name: '测试验证', type: 'testing', timeout: 1200, aiReview: true, humanReview: false },
    ],
  },
  {
    id: 'template-full', name: '完整DevOps流程', description: '适用于大型项目的完整DevOps流程',
    category: 'advanced', icon: '⚙️', features: ['自动化', 'CI/CD集成', '持续交付'],
    stages: [
      { id: 'req', name: '需求分析', type: 'requirement', timeout: 900, aiReview: true, humanReview: true },
      { id: 'design', name: '系统设计', type: 'design', timeout: 1200, aiReview: true, humanReview: true },
      { id: 'code', name: '编码实现', type: 'coding', timeout: 4800, aiReview: false, humanReview: false },
      { id: 'review', name: '代码审查', type: 'review', timeout: 900, aiReview: true, humanReview: true },
      { id: 'test', name: '测试验证', type: 'testing', timeout: 1800, aiReview: true, humanReview: true },
      { id: 'cicd', name: 'CI/CD', type: 'cicd', timeout: 600, aiReview: true, humanReview: false },
      { id: 'deploy', name: '部署发布', type: 'deploy', timeout: 300, aiReview: true, humanReview: true },
    ],
  },
  {
    id: 'template-agile', name: '敏捷迭代流程', description: '适用于敏捷开发的快速迭代流程',
    category: 'advanced', icon: '⚡', features: ['快速迭代', '敏捷开发', '持续改进'],
    stages: [
      { id: 'req', name: '需求分析', type: 'requirement', timeout: 300, aiReview: true, humanReview: false },
      { id: 'code', name: '编码实现', type: 'coding', timeout: 2400, aiReview: false, humanReview: false },
      { id: 'review', name: '代码审查', type: 'review', timeout: 300, aiReview: true, humanReview: false },
      { id: 'test', name: '测试验证', type: 'testing', timeout: 600, aiReview: true, humanReview: false },
      { id: 'deploy', name: '部署发布', type: 'deploy', timeout: 180, aiReview: true, humanReview: false },
    ],
  },
  {
    id: 'template-enterprise', name: '企业级安全流程', description: '企业级安全开发流程',
    category: 'enterprise', icon: '🔒', features: ['安全审计', '多级审查', '合规检查'],
    stages: [
      { id: 'req', name: '需求分析', type: 'requirement', timeout: 1200, aiReview: true, humanReview: true },
      { id: 'design', name: '系统设计', type: 'design', timeout: 1800, aiReview: true, humanReview: true },
      { id: 'code', name: '编码实现', type: 'coding', timeout: 7200, aiReview: false, humanReview: false },
      { id: 'review', name: '代码审查', type: 'review', timeout: 1200, aiReview: true, humanReview: true },
      { id: 'test', name: '安全测试', type: 'testing', timeout: 2400, aiReview: true, humanReview: true },
      { id: 'cicd', name: 'CI/CD', type: 'cicd', timeout: 900, aiReview: true, humanReview: true },
      { id: 'deploy', name: '部署发布', type: 'deploy', timeout: 600, aiReview: true, humanReview: true },
    ],
  },
];

const PipelineConfig: React.FC = () => {
  const navigate = useNavigate();
  const [configs, setConfigs] = useState<PipelineConfigItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalVisible, setModalVisible] = useState(false);
  const [templateModalVisible, setTemplateModalVisible] = useState(false);
  const [detailDrawerVisible, setDetailDrawerVisible] = useState(false);
  const [selectedConfig, setSelectedConfig] = useState<PipelineConfigItem | null>(null);
  const [selectedTemplate, setSelectedTemplate] = useState<PipelineTemplate | null>(null);
  const [form] = Form.useForm();
  const { message } = App.useApp();

  useEffect(() => { loadConfigs(); }, []);

  const loadConfigs = () => {
    setLoading(true);
    const saved = localStorage.getItem('pipeline-configs');
    if (saved) { try { setConfigs(JSON.parse(saved)); } catch { setConfigs([]); } }
    setLoading(false);
  };

  const saveConfigs = (newConfigs: PipelineConfigItem[]) => {
    localStorage.setItem('pipeline-configs', JSON.stringify(newConfigs));
    setConfigs(newConfigs);
  };

  const handleCreateFromTemplate = (template: PipelineTemplate) => {
    setSelectedTemplate(template);
    form.resetFields();
    form.setFieldsValue({ name: `${template.name} - 副本`, description: template.description, category: template.category });
    setTemplateModalVisible(false);
    setModalVisible(true);
  };

  const handleSave = async () => {
    try {
      const values = await form.validateFields();
      const stages = selectedTemplate ? selectedTemplate.stages : [{ id: 'req', name: '需求分析', type: 'requirement' as const, timeout: 600, aiReview: true, humanReview: false }];
      if (selectedConfig) {
        const updated = configs.map((c) => c.id === selectedConfig.id ? { ...c, name: values.name, description: values.description, category: values.category, updatedAt: new Date().toISOString() } : c);
        saveConfigs(updated);
        message.success('管线配置已更新');
      } else {
        const newConfig: PipelineConfigItem = { id: `config-${Date.now()}`, name: values.name, description: values.description, stages, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), isTemplate: false, usageCount: 0, category: values.category, tags: [] };
        saveConfigs([...configs, newConfig]);
        message.success('管线配置已创建');
      }
      setModalVisible(false);
    } catch {}
  };

  const handleDelete = (id: string) => { saveConfigs(configs.filter((c) => c.id !== id)); message.success('已删除'); };

  const handleDuplicate = (config: PipelineConfigItem) => {
    saveConfigs([...configs, { ...config, id: `config-${Date.now()}`, name: `${config.name} - 副本`, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), usageCount: 0 }]);
    message.success('已复制');
  };

  const getTotalTimeout = (stages: StageConfig[]) => stages.reduce((sum, s) => sum + s.timeout, 0);

  const formatTimeout = (seconds: number) => {
    if (seconds >= 3600) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
    if (seconds >= 60) return `${Math.floor(seconds / 60)}m`;
    return `${seconds}s`;
  };

  const columns: ColumnsType<PipelineConfigItem> = [
    {
      title: '配置名称', dataIndex: 'name', key: 'name',
      render: (text: string, record: PipelineConfigItem) => (
        <Space><AppstoreOutlined style={{ color: '#1890ff' }} /><Text strong>{text}</Text>
          {record.category && <Tag color={record.category === 'basic' ? 'green' : record.category === 'advanced' ? 'blue' : 'purple'}>{record.category === 'basic' ? '基础' : record.category === 'advanced' ? '高级' : '企业级'}</Tag>}
        </Space>
      ),
    },
    { title: '描述', dataIndex: 'description', key: 'description', ellipsis: true, width: 200 },
    {
      title: '阶段流程', dataIndex: 'stages', key: 'stages', width: 300,
      render: (stages: StageConfig[]) => (
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexWrap: 'wrap' }}>
          {stages.map((stage, index) => (
            <React.Fragment key={stage.id}>
              <Tooltip title={`${stage.name} (${formatTimeout(stage.timeout)})`}>
                <Tag color={stageTypeConfig[stage.type]?.color} style={{ margin: 0 }}>{stageTypeConfig[stage.type]?.icon}</Tag>
              </Tooltip>
              {index < stages.length - 1 && <ArrowRightOutlined style={{ fontSize: 10, color: '#bbb' }} />}
            </React.Fragment>
          ))}
        </div>
      ),
    },
    { title: '总时长', key: 'totalTimeout', width: 100, render: (_, record) => <Space><ClockCircleOutlined /><Text>{formatTimeout(getTotalTimeout(record.stages))}</Text></Space> },
    { title: '使用次数', dataIndex: 'usageCount', key: 'usageCount', width: 100, render: (count) => <Badge count={count} showZero color="#52c41a" /> },
    { title: '更新时间', dataIndex: 'updatedAt', key: 'updatedAt', width: 160, render: (date) => new Date(date).toLocaleString('zh-CN') },
    {
      title: '操作', key: 'actions', width: 220, fixed: 'right',
      render: (_, record) => (
        <Space>
          <Tooltip title="查看详情"><Button type="text" icon={<EyeOutlined />} onClick={() => { setSelectedConfig(record); setDetailDrawerVisible(true); }} /></Tooltip>
          <Tooltip title="编辑阶段"><Button type="text" icon={<SettingOutlined />} onClick={() => navigate(`/pipeline-config/${record.id}/editor`)} /></Tooltip>
          <Tooltip title="复制"><Button type="text" icon={<CopyOutlined />} onClick={() => handleDuplicate(record)} /></Tooltip>
          <Popconfirm title="确定删除？" onConfirm={() => handleDelete(record.id)}><Tooltip title="删除"><Button type="text" danger icon={<DeleteOutlined />} /></Tooltip></Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 16 }}>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>管线配置</Title>
          <Text type="secondary">管理和配置软件开发流程管线模板</Text>
        </div>
        <Space>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setTemplateModalVisible(true)}>从模板创建</Button>
          <Button icon={<PlusOutlined />} onClick={() => { setSelectedTemplate(null); form.resetFields(); setModalVisible(true); }}>自定义配置</Button>
        </Space>
      </div>

      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col span={6}><Card><Statistic title="配置总数" value={configs.length} prefix={<AppstoreOutlined />} /></Card></Col>
        <Col span={6}><Card><Statistic title="阶段总数" value={configs.reduce((s, c) => s + c.stages.length, 0)} prefix={<SettingOutlined />} /></Card></Col>
        <Col span={6}><Card><Statistic title="使用次数" value={configs.reduce((s, c) => s + c.usageCount, 0)} prefix={<PlayCircleOutlined />} /></Card></Col>
        <Col span={6}><Card><Statistic title="模板数量" value={defaultTemplates.length} prefix={<CheckCircleOutlined />} /></Card></Col>
      </Row>

      <Card>
        <Tabs defaultActiveKey="list" items={[
          {
            key: 'list', label: '配置列表',
            children: <Table columns={columns} dataSource={configs} rowKey="id" loading={loading} pagination={{ pageSize: 10 }} scroll={{ x: 1200 }} locale={{ emptyText: <Empty description="暂无配置" /> }} />
          },
          {
            key: 'templates', label: '模板库',
            children: (
              <List grid={{ gutter: 16, column: 3 }} dataSource={defaultTemplates}
                renderItem={(template) => (
                  <List.Item>
                    <Card hoverable actions={[<Button type="link" onClick={() => handleCreateFromTemplate(template)}>使用此模板</Button>]}>
                      <Card.Meta
                        avatar={<Avatar size={48} style={{ backgroundColor: '#f0f2f5', fontSize: 24 }}>{template.icon}</Avatar>}
                        title={<Space>{template.name}<Tag color={template.category === 'basic' ? 'green' : template.category === 'advanced' ? 'blue' : 'purple'}>{template.category === 'basic' ? '基础' : template.category === 'advanced' ? '高级' : '企业级'}</Tag></Space>}
                        description={<div><Paragraph ellipsis={{ rows: 2 }}>{template.description}</Paragraph><Space wrap>{template.features?.map((f, i) => <Tag key={i}>{f}</Tag>)}</Space></div>}
                      />
                    </Card>
                  </List.Item>
                )}
              />
            ),
          },
        ]} />
      </Card>

      <Modal title={selectedConfig ? '编辑配置' : '创建配置'} open={modalVisible} onOk={handleSave} onCancel={() => setModalVisible(false)} okText="保存" cancelText="取消" width={600}>
        <Form form={form} layout="vertical">
          <Form.Item name="name" label="配置名称" rules={[{ required: true }]}><Input placeholder="请输入名称" /></Form.Item>
          <Form.Item name="description" label="描述"><Input.TextArea rows={3} /></Form.Item>
          <Form.Item name="category" label="分类"><Select placeholder="选择分类"><Option value="basic">基础</Option><Option value="advanced">高级</Option><Option value="enterprise">企业级</Option></Select></Form.Item>
          {selectedTemplate && <Alert message={`基于模板: ${selectedTemplate.name}`} description={`包含 ${selectedTemplate.stages.length} 个阶段`} type="info" showIcon />}
        </Form>
      </Modal>

      <Modal title="选择模板" open={templateModalVisible} onCancel={() => setTemplateModalVisible(false)} footer={null} width={900}>
        <List grid={{ gutter: 16, column: 2 }} dataSource={defaultTemplates}
          renderItem={(template) => (
            <List.Item>
              <Card hoverable onClick={() => handleCreateFromTemplate(template)}>
                <Card.Meta avatar={<Avatar size={48} style={{ backgroundColor: '#f0f2f5', fontSize: 24 }}>{template.icon}</Avatar>}
                  title={<Space>{template.name}<Tag color={template.category === 'basic' ? 'green' : template.category === 'advanced' ? 'blue' : 'purple'}>{template.category === 'basic' ? '基础' : template.category === 'advanced' ? '高级' : '企业级'}</Tag></Space>}
                  description={<div><Paragraph ellipsis={{ rows: 2 }}>{template.description}</Paragraph><Space wrap>{template.features?.map((f, i) => <Tag key={i}>{f}</Tag>)}<Tag color="blue">{template.stages.length} 个阶段</Tag></Space></div>}
                />
              </Card>
            </List.Item>
          )}
        />
      </Modal>

      <Drawer title="配置详情" placement="right" width={600} onClose={() => setDetailDrawerVisible(false)} open={detailDrawerVisible}
        extra={<Button icon={<EditOutlined />} onClick={() => { setDetailDrawerVisible(false); if (selectedConfig) navigate(`/pipeline-config/${selectedConfig.id}/editor`); }}>编辑阶段</Button>}>
        {selectedConfig && (
          <div>
            <Descriptions column={2} bordered size="small">
              <Descriptions.Item label="名称" span={2}>{selectedConfig.name}</Descriptions.Item>
              <Descriptions.Item label="创建时间">{new Date(selectedConfig.createdAt).toLocaleString('zh-CN')}</Descriptions.Item>
              <Descriptions.Item label="更新时间">{new Date(selectedConfig.updatedAt).toLocaleString('zh-CN')}</Descriptions.Item>
              <Descriptions.Item label="描述" span={2}>{selectedConfig.description || '无'}</Descriptions.Item>
            </Descriptions>
            <Divider>阶段配置 ({selectedConfig.stages.length} 个阶段)</Divider>
            <Timeline>
              {selectedConfig.stages.map((stage) => (
                <Timeline.Item key={stage.id} color={stageTypeConfig[stage.type]?.color}
                  dot={<div style={{ color: stageTypeConfig[stage.type]?.color }}>{stageTypeConfig[stage.type]?.icon}</div>}>
                  <Card size="small">
                    <Text strong>{stage.name}</Text> <Tag color={stageTypeConfig[stage.type]?.color}>{stageTypeConfig[stage.type]?.label}</Tag>
                    <div style={{ marginTop: 4 }}><ClockCircleOutlined /> {formatTimeout(stage.timeout)} {stage.aiReview && <Tag color="blue" style={{ marginLeft: 4 }}>AI审查</Tag>}{stage.humanReview && <Tag color="orange">人工审查</Tag>}</div>
                  </Card>
                </Timeline.Item>
              ))}
            </Timeline>
          </div>
        )}
      </Drawer>
    </div>
  );
};

export default PipelineConfig;