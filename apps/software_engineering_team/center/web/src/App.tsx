import React, { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AppLayout, ErrorBoundary } from '@/components';
import { useSettingsStore } from '@/stores';
import {
  Dashboard,
  ProjectList,
  ProjectDetail,
  PipelineEditor,
  PipelineConfig,
  Chat,
  Review,
  Graph,
  Settings,
  Monitor,
  Logs,
} from '@/pages';

const App: React.FC = () => {
  const loadSettings = useSettingsStore((s) => s.loadSettings);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  return (
    <ErrorBoundary>
      <BrowserRouter>
        <AppLayout>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/projects" element={<ProjectList />} />
            <Route path="/projects/:id" element={<ProjectDetail />} />
            <Route path="/projects/:id/editor" element={<PipelineEditor />} />
            <Route path="/pipeline-config" element={<PipelineConfig />} />
            <Route path="/pipeline-config/:id/editor" element={<PipelineEditor />} />
            <Route path="/chat" element={<Chat />} />
            <Route path="/chat/:projectId" element={<Chat />} />
            <Route path="/reviews" element={<Review />} />
            <Route path="/graph" element={<Graph />} />
            <Route path="/graph/:projectId" element={<Graph />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/monitor" element={<Monitor />} />
            <Route path="/logs" element={<Logs />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </AppLayout>
      </BrowserRouter>
    </ErrorBoundary>
  );
};

export default App;