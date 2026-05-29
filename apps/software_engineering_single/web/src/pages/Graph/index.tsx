import React from 'react';
import { useParams } from 'react-router-dom';
import GraphView from './GraphView';
import styles from './Graph.module.css';

const Graph: React.FC = () => {
  const { projectId } = useParams<{ projectId: string }>();

  return (
    <div className={styles.pageContainer}>
      <GraphView projectId={projectId} />
    </div>
  );
};

export default Graph;
