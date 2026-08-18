import React from 'react';
import { Tag } from 'antd';
import type { StageStatus } from '@/types';

interface StageStatusBadgeProps {
  status: StageStatus;
}

const statusConfig: Record<StageStatus, { color: string; text: string }> = {
  pending: { color: 'default', text: '待执行' },
  running: { color: 'processing', text: '执行中' },
  success: { color: 'success', text: '成功' },
  failed: { color: 'error', text: '失败' },
  reviewing: { color: 'warning', text: '待审查' },
  skipped: { color: 'default', text: '已跳过' },
};

const StageStatusBadge: React.FC<StageStatusBadgeProps> = ({ status }) => {
  const config = statusConfig[status];

  return (
    <Tag color={config.color}>
      {config.text}
    </Tag>
  );
};

export default StageStatusBadge;