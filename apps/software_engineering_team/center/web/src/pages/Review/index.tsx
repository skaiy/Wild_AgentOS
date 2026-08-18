import React from 'react';
import { Card, Table, Button, Tag, Space, Modal, Input, App } from 'antd';
import { CheckCircleOutlined, CloseCircleOutlined, EyeOutlined } from '@ant-design/icons';
import { reviewApi } from '@/api';
import type { PendingReview } from '@/types';

const Review: React.FC = () => {
  const [reviews, setReviews] = React.useState<PendingReview[]>([]);
  const [loading, setLoading] = React.useState(false);
  const [isModalOpen, setIsModalOpen] = React.useState(false);
  const [selectedReview, setSelectedReview] = React.useState<PendingReview | null>(null);
  const [comment, setComment] = React.useState('');
  const { message } = App.useApp();

  const fetchReviews = async () => {
    setLoading(true);
    try {
      const result = await reviewApi.getPending();
      setReviews(result.reviews);
    } catch (error) {
      message.error((error as Error).message);
    }
    setLoading(false);
  };

  React.useEffect(() => { fetchReviews(); }, []);

  const handleSubmit = async (approved: boolean) => {
    if (!selectedReview) return;
    try {
      await reviewApi.submit(
        selectedReview.stageId,
        { approved, comments: comment ? [comment] : [], reviewer: 'user' },
        { taskId: selectedReview.taskId, workflowId: selectedReview.workflowId || '' }
      );
      message.success(approved ? '已通过' : '已拒绝');
      setIsModalOpen(false);
      setSelectedReview(null);
      setComment('');
      fetchReviews();
    } catch (error) {
      message.error((error as Error).message);
    }
  };

  const columns = [
    { title: '项目', dataIndex: 'projectName', key: 'projectName' },
    { title: '阶段', dataIndex: 'stageName', key: 'stageName' },
    { title: '类型', dataIndex: 'stageType', key: 'stageType', render: (type: string) => <Tag>{type}</Tag> },
    { title: '开始时间', dataIndex: 'startedAt', key: 'startedAt', render: (date: string) => new Date(date).toLocaleString() },
    {
      title: '操作', key: 'actions',
      render: (_: unknown, record: PendingReview) => (
        <Button type="link" icon={<EyeOutlined />} onClick={() => { setSelectedReview(record); setIsModalOpen(true); }}>审查</Button>
      ),
    },
  ];

  return (
    <div>
      <Card title="待审查列表">
        <Table columns={columns} dataSource={reviews} rowKey="stageId" loading={loading} pagination={{ pageSize: 10 }} />
      </Card>

      <Modal title="审查详情" open={isModalOpen} onCancel={() => setIsModalOpen(false)} footer={null} width={600}>
        {selectedReview && (
          <div>
            <div style={{ marginBottom: 16 }}>
              <p><strong>项目:</strong> {selectedReview.projectName}</p>
              <p><strong>阶段:</strong> {selectedReview.stageName}</p>
              <p><strong>摘要:</strong> {selectedReview.summary || '无'}</p>
            </div>
            <div style={{ marginBottom: 16 }}>
              <Input.TextArea placeholder="请输入审查意见（可选）" rows={4} value={comment} onChange={(e) => setComment(e.target.value)} />
            </div>
            <Space>
              <Button type="primary" icon={<CheckCircleOutlined />} onClick={() => handleSubmit(true)}>通过</Button>
              <Button danger icon={<CloseCircleOutlined />} onClick={() => handleSubmit(false)}>拒绝</Button>
            </Space>
          </div>
        )}
      </Modal>
    </div>
  );
};

export default Review;