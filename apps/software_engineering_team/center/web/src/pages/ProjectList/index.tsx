import React from 'react';
import { Card, Table, Button, Space, Modal, Form, Input, Tag, App } from 'antd';
import { PlusOutlined, SearchOutlined, DeleteOutlined, FolderOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useProjectStore } from '@/stores';
import type { ProjectMeta } from '@/types';

const ProjectList: React.FC = () => {
  const navigate = useNavigate();
  const { projects, loading, fetchProjects, createProject, deleteProject } = useProjectStore();
  const [searchText, setSearchText] = React.useState('');
  const [isModalOpen, setIsModalOpen] = React.useState(false);
  const [form] = Form.useForm();
  const { message } = App.useApp();

  React.useEffect(() => {
    fetchProjects();
  }, [fetchProjects]);

  const filteredProjects = projects.filter((p) =>
    p.projectName.toLowerCase().includes(searchText.toLowerCase())
  );

  const handleCreate = async (values: { name: string; description: string }) => {
    try {
      const project = await createProject(values);
      message.success('项目创建成功');
      setIsModalOpen(false);
      form.resetFields();
      navigate(`/projects/${project.projectId}`);
    } catch (error) {
      message.error((error as Error).message);
    }
  };

  const handleDelete = async (id: string) => {
    Modal.confirm({
      title: '确认删除',
      content: '确定要删除这个项目吗？此操作不可恢复。',
      okText: '删除',
      okType: 'danger',
      cancelText: '取消',
      onOk: async () => {
        try {
          await deleteProject(id);
          message.success('项目已删除');
        } catch (error) {
          message.error((error as Error).message);
        }
      },
    });
  };

  const statusLabel = (status: string) => {
    const map: Record<string, string> = {
      running: '运行中',
      completed: '已完成',
      failed: '失败',
      pending: '等待中',
      initialized: '已初始化',
    };
    return map[status] || status;
  };

  const columns = [
    {
      title: '项目名称',
      dataIndex: 'projectName',
      key: 'projectName',
      render: (text: string, record: ProjectMeta) => (
        <a onClick={() => navigate(`/projects/${record.projectId}`)}>{text}</a>
      ),
    },
    {
      title: '描述',
      dataIndex: 'description',
      key: 'description',
      ellipsis: true,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      render: (status: string) => {
        const colorMap: Record<string, string> = {
          initialized: 'default',
          running: 'processing',
          completed: 'success',
          failed: 'error',
          archived: 'warning',
        };
        return <Tag color={colorMap[status] || 'default'}>{statusLabel(status)}</Tag>;
      },
    },
    {
      title: '创建时间',
      dataIndex: 'createdAt',
      key: 'createdAt',
      render: (date: string) => (date ? new Date(date).toLocaleString() : '-'),
    },
    {
      title: '操作',
      key: 'actions',
      render: (_: unknown, record: ProjectMeta) => (
        <Space>
          <Button type="link" icon={<FolderOutlined />} onClick={() => navigate(`/projects/${record.projectId}`)}>查看</Button>
          <Button type="link" danger icon={<DeleteOutlined />} onClick={() => handleDelete(record.projectId)}>删除</Button>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Card
        title="项目列表"
        extra={
          <Space>
            <Input
              placeholder="搜索项目"
              prefix={<SearchOutlined />}
              value={searchText}
              onChange={(e) => setSearchText(e.target.value)}
              style={{ width: 200 }}
            />
            <Button type="primary" icon={<PlusOutlined />} onClick={() => setIsModalOpen(true)}>新建项目</Button>
          </Space>
        }
      >
        <Table
          columns={columns}
          dataSource={filteredProjects}
          rowKey="projectId"
          loading={loading}
          pagination={{ pageSize: 10 }}
        />
      </Card>

      <Modal title="新建项目" open={isModalOpen} onCancel={() => setIsModalOpen(false)} footer={null}>
        <Form form={form} layout="vertical" onFinish={handleCreate}>
          <Form.Item name="name" label="项目名称" rules={[{ required: true, message: '请输入项目名称' }]}>
            <Input placeholder="请输入项目名称" />
          </Form.Item>
          <Form.Item name="description" label="项目描述">
            <Input.TextArea rows={3} placeholder="请输入项目描述" />
          </Form.Item>
          <Form.Item>
            <Space>
              <Button type="primary" htmlType="submit">创建</Button>
              <Button onClick={() => setIsModalOpen(false)}>取消</Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};

export default ProjectList;