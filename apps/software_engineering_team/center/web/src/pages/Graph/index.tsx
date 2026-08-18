import React from 'react';
import { useParams } from 'react-router-dom';
import GraphView from './GraphView';

const Graph: React.FC = () => {
  const { projectId } = useParams<{ projectId: string }>();
  return <GraphView projectId={projectId} />;
};

export default Graph;