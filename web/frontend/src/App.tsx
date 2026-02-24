import { useState } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AppProvider } from './context/AppContext';
import { ErrorBoundary } from './components/ErrorBoundary';
import { MainLayout } from './components/layout/MainLayout';
import { SplashScreen } from './components/SplashScreen';
import { CommandCenter } from './pages/CommandCenter';
import { Dashboard } from './pages/Dashboard';
import { NodesPage } from './pages/NodesPage';
import { NodeDetailPage } from './pages/NodeDetailPage';
import { AgentDetailPage } from './pages/AgentDetailPage';
import { OrchestratorPage } from './pages/OrchestratorPage';
import { OrchestratorComingSoonPage } from './pages/OrchestratorComingSoonPage';
import { AgentChatComingSoonPage } from './pages/AgentChatComingSoonPage';
import { ToolkitPage } from './pages/ToolkitPage';
import { OperationsPage } from './pages/OperationsPage';
import { InterceptPage } from './pages/InterceptPage';
import { HuntingPage } from './pages/HuntingPage';
// import { DiscoveryPage } from './pages/DiscoveryPage';  // Hidden - feature not ready
import { SettingsPage } from './pages/SettingsPage';
import { NotFoundPage } from './pages/NotFoundPage';
import AgentChatPage from './pages/AgentChatPage';
import { getFeatureFlags } from './utils/featureFlags';
import { getUiMode } from './utils/uiMode';

export default function App() {
  //
  // Only show splash screen when navigating to root path.
  //
  const [showSplash, setShowSplash] = useState(() => window.location.pathname === '/');

  //
  // Check feature flags for orchestrator.
  //
  const flags = getFeatureFlags();

  if (showSplash) {
    return <SplashScreen onComplete={() => setShowSplash(false)} />;
  }

  //
  // Determine root route based on UI mode preference.
  //
  const uiMode = getUiMode();
  const rootElement = uiMode === 'legacy'
    ? <Navigate to="/dashboard" replace />
    : <CommandCenter />;

  return (
    <ErrorBoundary>
      <BrowserRouter>
        <AppProvider>
          <Routes>
            {/*
            //
            // Root route — depends on UI mode preference.
            //
            */}
            <Route path="/" element={rootElement} />

            {/*
            //
            // Classic pages with sidebar layout.
            //
            */}
            <Route element={<MainLayout />}>
              <Route path="/dashboard" element={<Dashboard />} />
              <Route path="/nodes" element={<NodesPage />} />
              <Route path="/nodes/:nodeId" element={<NodeDetailPage />} />
              <Route path="/nodes/:nodeId/agents/:agentShortName" element={<AgentDetailPage />} />
              <Route path="/orchestrator" element={flags.orchestrator ? <OrchestratorPage /> : <OrchestratorComingSoonPage />} />
              <Route path="/toolkit" element={<ToolkitPage />} />
              <Route path="/operations" element={<OperationsPage />} />
              <Route path="/intercept" element={<InterceptPage />} />
              <Route path="/hunting" element={<HuntingPage />} />
              <Route path="/agent-chat" element={flags.agentChat ? <AgentChatPage /> : <AgentChatComingSoonPage />} />
              {/* <Route path="/discovery" element={<DiscoveryPage />} /> */}  {/* Hidden - feature not ready */}
              <Route path="/settings" element={<SettingsPage />} />
              <Route path="*" element={<NotFoundPage />} />
            </Route>
          </Routes>
        </AppProvider>
      </BrowserRouter>
    </ErrorBoundary>
  );
}
