import React, { useEffect, useRef, useState } from 'react';
import { Card, Input, Button, Space, Drawer, Descriptions, Empty, Spin, Tag } from 'antd';
import { SearchOutlined, ZoomInOutlined, ZoomOutOutlined, ReloadOutlined } from '@ant-design/icons';
import { Graph } from '@antv/g6';
import type { GraphData, GraphNode } from '@/types';
import { graphApi } from '@/api';

interface GraphViewProps {
  projectId?: string;
}

const GraphViewComponent: React.FC<GraphViewProps> = ({ projectId }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const graphRef = useRef<any>(null);
  const [loading, setLoading] = useState(false);
  const [graphData, setGraphData] = useState<GraphData | null>(null);
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [searchText, setSearchText] = useState('');

  useEffect(() => {
    if (!containerRef.current) return;

    const width = containerRef.current.offsetWidth;
    const height = containerRef.current.offsetHeight || 500;

    const graph = new Graph({
      container: containerRef.current,
      width,
      height,
      layout: {
        type: 'force',
        preventOverlap: true,
        linkDistance: 150,
        nodeStrength: -30,
        edgeStrength: 0.1,
      },
    } as any);

    graph.on('node:click', (evt: any) => {
      const { item } = evt;
      if (!item) return;
      const model = item.getModel() as GraphNode;
      setSelectedNode(model);
      setDrawerOpen(true);
    });

    graphRef.current = graph;

    const handleResize = () => {
      if (!containerRef.current || !graphRef.current) return;
      graphRef.current.changeSize(containerRef.current.offsetWidth, containerRef.current.offsetHeight || 500);
    };

    window.addEventListener('resize', handleResize);

    return () => {
      window.removeEventListener('resize', handleResize);
      graph.destroy();
    };
  }, []);

  useEffect(() => {
    if (projectId) fetchGraphData(projectId);
    else loadDemoData();
  }, [projectId]);

  const fetchGraphData = async (id: string) => {
    setLoading(true);
    try {
      const data = await graphApi.getProjectGraph(id);
      setGraphData(data);
      renderGraph(data);
    } catch {
      loadDemoData();
    }
    setLoading(false);
  };

  const loadDemoData = () => {
    const demoData: GraphData = {
      nodes: [
        { id: 'project-1', label: '项目: 电商平台', type: 'Project', iri: 'iri://project/1' },
        { id: 'req-1', label: '需求: 用户登录', type: 'Requirement', iri: 'iri://req/1' },
        { id: 'req-2', label: '需求: 商品浏览', type: 'Requirement', iri: 'iri://req/2' },
        { id: 'design-1', label: '设计: 登录模块', type: 'Design', iri: 'iri://design/1' },
        { id: 'design-2', label: '设计: 商品模块', type: 'Design', iri: 'iri://design/2' },
        { id: 'code-1', label: '代码: auth.go', type: 'Code', iri: 'iri://code/1' },
        { id: 'code-2', label: '代码: product.go', type: 'Code', iri: 'iri://code/2' },
        { id: 'test-1', label: '测试: 登录测试', type: 'Test', iri: 'iri://test/1' },
      ],
      edges: [
        { id: 'e1', source: 'project-1', target: 'req-1', label: 'hasRequirement', type: 'has' },
        { id: 'e2', source: 'project-1', target: 'req-2', label: 'hasRequirement', type: 'has' },
        { id: 'e3', source: 'req-1', target: 'design-1', label: 'designBy', type: 'derives' },
        { id: 'e4', source: 'req-2', target: 'design-2', label: 'designBy', type: 'derives' },
        { id: 'e5', source: 'design-1', target: 'code-1', label: 'implements', type: 'derives' },
        { id: 'e6', source: 'design-2', target: 'code-2', label: 'implements', type: 'derives' },
        { id: 'e7', source: 'code-1', target: 'test-1', label: 'testedBy', type: 'verifies' },
      ],
    };
    setGraphData(demoData);
    renderGraph(demoData);
  };

  const renderGraph = (data: GraphData) => {
    if (!graphRef.current) return;

    const nodeTypeColors: Record<string, string> = {
      Project: '#1890ff', Requirement: '#722ed1', Design: '#13c2c2',
      Code: '#52c41a', Test: '#fa8c16', default: '#666',
    };

    const g6Data = {
      nodes: data.nodes.map((node) => ({
        id: node.id,
        label: node.label,
        style: { fill: nodeTypeColors[node.type] || nodeTypeColors.default, stroke: nodeTypeColors[node.type] || nodeTypeColors.default },
        originalData: node,
      })),
      edges: data.edges.map((edge) => ({
        id: edge.id, source: edge.source, target: edge.target, label: edge.label,
        style: { endArrow: true }, originalData: edge,
      })),
    };

    graphRef.current.data(g6Data);
    graphRef.current.render();
  };

  const handleZoomIn = () => graphRef.current?.zoom(1.2);
  const handleZoomOut = () => graphRef.current?.zoom(0.8);
  const handleFitView = () => graphRef.current?.fitView({ padding: 20 });

  const handleSearch = () => {
    if (!searchText || !graphRef.current || !graphData) return;
    const foundNode = graphData.nodes.find((n) => n.label.toLowerCase().includes(searchText.toLowerCase()) || n.id.includes(searchText));
    if (foundNode) {
      graphRef.current.focusItem(foundNode.id, true, { easing: 'easeCubicOut' });
      setSelectedNode(foundNode);
      setDrawerOpen(true);
    }
  };

  const getNodeColor = (type: string): string => {
    const colors: Record<string, string> = {
      Project: '#1890ff', Requirement: '#722ed1', Design: '#13c2c2',
      Code: '#52c41a', Test: '#fa8c16',
    };
    return colors[type] || '#666';
  };

  return (
    <div>
      <Card
        title="知识图谱"
        extra={
          <Space>
            <Input placeholder="搜索节点..." prefix={<SearchOutlined />} value={searchText}
              onChange={(e) => setSearchText(e.target.value)} onPressEnter={handleSearch} style={{ width: 200 }} />
            <Button icon={<ZoomInOutlined />} onClick={handleZoomIn} />
            <Button icon={<ZoomOutOutlined />} onClick={handleZoomOut} />
            <Button icon={<ReloadOutlined />} onClick={handleFitView} />
          </Space>
        }
      >
        <Spin spinning={loading}>
          {graphData ? (
            <div ref={containerRef} style={{ height: 500, width: '100%' }} />
          ) : (
            <Empty description="暂无图谱数据" />
          )}
        </Spin>
      </Card>

      <Drawer title="节点详情" placement="right" open={drawerOpen} onClose={() => setDrawerOpen(false)} width={400}>
        {selectedNode && (
          <div>
            <div style={{ marginBottom: 16 }}>
              <Tag color={getNodeColor(selectedNode.type)}>{selectedNode.type}</Tag>
              <h3>{selectedNode.label}</h3>
            </div>
            <Descriptions column={1} bordered size="small">
              <Descriptions.Item label="ID">{selectedNode.id}</Descriptions.Item>
              <Descriptions.Item label="类型">{selectedNode.type}</Descriptions.Item>
              {selectedNode.iri && <Descriptions.Item label="IRI"><code>{selectedNode.iri}</code></Descriptions.Item>}
              {selectedNode.properties && (
                <Descriptions.Item label="属性">
                  <pre style={{ margin: 0, fontSize: 12 }}>{JSON.stringify(selectedNode.properties, null, 2)}</pre>
                </Descriptions.Item>
              )}
            </Descriptions>
          </div>
        )}
      </Drawer>
    </div>
  );
};

export default GraphViewComponent;