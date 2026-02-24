import { useState, useEffect } from 'react';
import {
  Monitor, LayoutGrid, Sun, Moon, Cpu, Server, Info, Wifi, WifiOff,
  Plus, Trash2, Edit2, Save, Check, X, Key, List, Loader2,
  Circle, CircleCheck, Download, ExternalLink, FileCode,
} from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';
import { useTheme } from '../../context/ThemeContext';
import { getFeatureFlags } from '../../utils/featureFlags';
import { getUiMode, setUiMode, type UiMode } from '../../utils/uiMode';

type Tab = 'display' | 'llm' | 'service' | 'about';
type LLMView = 'models' | 'features';

interface SettingsModalProps {
  onClose: () => void;
}

interface ProviderOption {
  value: string;
  label: string;
}

interface ModelDefinition {
  name: string;
  provider: string;
  model: string;
  apiKey: string;
}

interface FeatureAssignments {
  orchestrator: string | null;
  semanticOps: string | null;
  semanticParser: string | null;
  trafficParser: string | null;
}

interface NodeDownloadInfo {
  platform: string;
  filename: string;
  available: boolean;
  size: number | null;
}

export function SettingsModal({ onClose }: SettingsModalProps) {
  const { state, getConfig, setConfig, clearEventLog } = useApp();
  const { theme, setTheme } = useTheme();
  const navigate = useNavigate();

  const [activeTab, setActiveTab] = useState<Tab>('display');
  const [llmView, setLlmView] = useState<LLMView>('models');

  //
  // Model definitions state.
  //

  const [modelDefinitions, setModelDefinitions] = useState<ModelDefinition[]>([]);
  const [editingModel, setEditingModel] = useState<ModelDefinition | null>(null);
  const [isAddingModel, setIsAddingModel] = useState(false);
  const [newModel, setNewModel] = useState<Omit<ModelDefinition, 'name'>>({
    provider: 'anthropic',
    model: '',
    apiKey: '',
  });
  const [isSavingModels, setIsSavingModels] = useState(false);
  const [showModelsSaved, setShowModelsSaved] = useState(false);

  //
  // Feature assignments.
  //

  const [featureAssignments, setFeatureAssignments] = useState<FeatureAssignments>({
    orchestrator: null,
    semanticOps: null,
    semanticParser: null,
    trafficParser: null,
  });
  const [orchestratorMaxTokens, setOrchestratorMaxTokens] = useState('25000');
  const [isSavingFeatures, setIsSavingFeatures] = useState(false);
  const [showFeaturesSaved, setShowFeaturesSaved] = useState(false);

  //
  // Model chooser.
  //

  const [showModelChooser, setShowModelChooser] = useState(false);
  const [modelChooserTarget, setModelChooserTarget] = useState<'new' | 'edit' | null>(null);
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [isLoadingModels, setIsLoadingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);

  //
  // Provider options.
  //

  const [providers, setProviders] = useState<ProviderOption[]>([]);

  //
  // Service settings.
  //

  const [eventLoggingEnabled, setEventLoggingEnabled] = useState(false);
  const [huntingQueryRowLimit, setHuntingQueryRowLimit] = useState('10000000');
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [mcpServerEnabled, setMcpServerEnabled] = useState(false);
  const [mcpServerPort, setMcpServerPort] = useState('8585');
  const [nodeDownloads, setNodeDownloads] = useState<NodeDownloadInfo[]>([]);
  const [isLoadingDownloads, setIsLoadingDownloads] = useState(false);

  //
  // Load config and providers on mount.
  //

  useEffect(() => {
    if (!state.connected) return;
    getConfig([
      'llm_model_definitions',
      'llm_feature_orchestrator',
      'llm_feature_semantic_ops',
      'llm_feature_semantic_parser',
      'llm_feature_traffic_parser',
      'llm_orchestrator_max_tokens',
      'application_logs_enabled',
      'hunting_query_row_limit',
      'mcp_server_enabled',
      'mcp_server_port',
    ]);
  }, [state.connected, getConfig]);

  useEffect(() => {
    fetch('/api/providers')
      .then(res => res.json())
      .then(data => {
        const opts = (data.providers || [])
          .map((p: { id: string; name: string }) => ({ value: p.id, label: p.name }))
          .sort((a: ProviderOption, b: ProviderOption) => a.label.localeCompare(b.label));
        setProviders(opts);
      })
      .catch(err => console.error('Failed to fetch providers:', err));
  }, []);

  //
  // Fetch downloads when service tab becomes active.
  //

  useEffect(() => {
    if (activeTab === 'service') {
      setIsLoadingDownloads(true);
      fetch('/api/downloads/info')
        .then(res => res.json())
        .then(data => setNodeDownloads(data.nodes || []))
        .catch(err => console.error('Failed to fetch downloads info:', err))
        .finally(() => setIsLoadingDownloads(false));
    }
  }, [activeTab]);

  //
  // Sync config into local state.
  //

  useEffect(() => {
    const cfg = state.config;

    if (cfg.llm_model_definitions) {
      try {
        const defs = JSON.parse(cfg.llm_model_definitions);
        if (Array.isArray(defs)) setModelDefinitions(defs);
      } catch { /* ignore */ }
    }

    setFeatureAssignments({
      orchestrator: cfg.llm_feature_orchestrator || null,
      semanticOps: cfg.llm_feature_semantic_ops || null,
      semanticParser: cfg.llm_feature_semantic_parser || null,
      trafficParser: cfg.llm_feature_traffic_parser || null,
    });

    setOrchestratorMaxTokens(cfg.llm_orchestrator_max_tokens || '25000');

    if (cfg.application_logs_enabled) {
      const v = cfg.application_logs_enabled.toLowerCase();
      setEventLoggingEnabled(!(v === 'false' || v === '0' || v === 'no'));
    } else {
      setEventLoggingEnabled(false);
    }

    setHuntingQueryRowLimit(cfg.hunting_query_row_limit || '10000000');

    if (cfg.mcp_server_enabled) {
      const v = cfg.mcp_server_enabled.toLowerCase();
      setMcpServerEnabled(!(v === 'false' || v === '0' || v === 'no'));
    } else {
      setMcpServerEnabled(false);
    }
    setMcpServerPort(cfg.mcp_server_port || '8585');
  }, [state.config]);

  //
  // Model CRUD handlers.
  //

  const genName = (provider: string, model: string) => `${provider}::${model}`;

  const handleAddModel = () => {
    if (!newModel.model.trim()) return;
    const name = genName(newModel.provider, newModel.model);
    if (modelDefinitions.some(m => m.name === name)) {
      alert(`Model "${name}" already exists.`);
      return;
    }
    setModelDefinitions([...modelDefinitions, { name, ...newModel }]);
    setNewModel({ provider: 'anthropic', model: '', apiKey: '' });
    setIsAddingModel(false);
  };

  const handleUpdateModel = () => {
    if (!editingModel) return;
    const newName = genName(editingModel.provider, editingModel.model);
    const oldName = editingModel.name;
    if (newName !== oldName && modelDefinitions.some(m => m.name === newName)) {
      alert(`Model "${newName}" already exists.`);
      return;
    }
    setModelDefinitions(modelDefinitions.map(m =>
      m.name === oldName ? { ...editingModel, name: newName } : m
    ));
    if (newName !== oldName) {
      const a = { ...featureAssignments };
      if (a.orchestrator === oldName) a.orchestrator = newName;
      if (a.semanticOps === oldName) a.semanticOps = newName;
      if (a.semanticParser === oldName) a.semanticParser = newName;
      if (a.trafficParser === oldName) a.trafficParser = newName;
      setFeatureAssignments(a);
    }
    setEditingModel(null);
  };

  const handleDeleteModel = (name: string) => {
    if (!confirm(`Delete model "${name}"?`)) return;
    setModelDefinitions(modelDefinitions.filter(m => m.name !== name));
    const a = { ...featureAssignments };
    if (a.orchestrator === name) a.orchestrator = null;
    if (a.semanticOps === name) a.semanticOps = null;
    if (a.semanticParser === name) a.semanticParser = null;
    if (a.trafficParser === name) a.trafficParser = null;
    setFeatureAssignments(a);
  };

  const handleSaveModels = () => {
    setIsSavingModels(true);
    setConfig({ llm_model_definitions: JSON.stringify(modelDefinitions) });
    setTimeout(() => {
      setIsSavingModels(false);
      setShowModelsSaved(true);
      setTimeout(() => setShowModelsSaved(false), 2000);
    }, 500);
  };

  const handleSaveFeatures = () => {
    setIsSavingFeatures(true);
    setConfig({
      llm_feature_orchestrator: featureAssignments.orchestrator || '',
      llm_feature_semantic_ops: featureAssignments.semanticOps || '',
      llm_feature_semantic_parser: featureAssignments.semanticParser || '',
      llm_feature_traffic_parser: featureAssignments.trafficParser || '',
      llm_orchestrator_max_tokens: orchestratorMaxTokens,
    });
    setTimeout(() => {
      setIsSavingFeatures(false);
      setShowFeaturesSaved(true);
      getConfig([
        'llm_model_definitions',
        'llm_feature_orchestrator',
        'llm_feature_semantic_ops',
        'llm_feature_semantic_parser',
        'llm_feature_traffic_parser',
      ]);
      setTimeout(() => setShowFeaturesSaved(false), 2000);
    }, 500);
  };

  const fetchModels = async (provider: string, apiKey: string) => {
    setModelError(null);
    setIsLoadingModels(true);
    setShowModelChooser(true);
    setAvailableModels([]);
    if (!apiKey) {
      setModelError('API key is required to fetch models');
      setIsLoadingModels(false);
      return;
    }
    try {
      const response = await fetch('/api/models', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ provider, api_key: apiKey }),
      });
      if (!response.ok) {
        const text = await response.text();
        let msg = `HTTP ${response.status}`;
        try { msg = JSON.parse(text).error || msg; } catch { if (text) msg = text; }
        throw new Error(msg);
      }
      const data = await response.json();
      setAvailableModels(data.models || []);
    } catch (err) {
      setModelError(err instanceof Error ? err.message : 'Unknown error');
    } finally {
      setIsLoadingModels(false);
    }
  };

  const handleModelSelect = (model: string) => {
    if (modelChooserTarget === 'new') {
      setNewModel(m => ({ ...m, model }));
    } else if (modelChooserTarget === 'edit' && editingModel) {
      setEditingModel({ ...editingModel, model });
    }
    setShowModelChooser(false);
    setModelChooserTarget(null);
  };

  //
  // Service handlers.
  //

  const handleEventLoggingToggle = () => {
    const next = !eventLoggingEnabled;
    setEventLoggingEnabled(next);
    setConfig({ application_logs_enabled: next ? 'true' : 'false' });
  };

  const handleMcpToggle = () => {
    const next = !mcpServerEnabled;
    setMcpServerEnabled(next);
    setConfig({ mcp_server_enabled: next ? 'true' : 'false' });
  };

  const handleMcpPortSave = () => {
    const port = parseInt(mcpServerPort, 10);
    if (port > 0 && port < 65536) {
      setConfig({ mcp_server_port: mcpServerPort });
    }
  };

  //
  // UI mode handler.
  //

  const handleModeChange = (mode: UiMode) => {
    setUiMode(mode);
    if (mode === 'legacy') {
      navigate('/dashboard');
      onClose();
    }
  };

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'display', label: 'Display', icon: <Monitor size={14} /> },
    { id: 'llm', label: 'LLM', icon: <Cpu size={14} /> },
    { id: 'service', label: 'Service', icon: <Server size={14} /> },
    { id: 'about', label: 'About', icon: <Info size={14} /> },
  ];

  //
  // Shared styling for select/input elements.
  //

  const inputCls = 'w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors';
  const btnSave = 'inline-flex items-center gap-1.5 px-2.5 py-1 text-xs bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors disabled:opacity-50';
  const btnGreen = 'inline-flex items-center gap-1.5 px-2.5 py-1 text-xs bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors';

  return (
    <Modal isOpen={true} onClose={onClose} title="Settings" size="xl" noPadding>
      <div className="flex h-[70vh]">

        {/*
        //
        // Tab sidebar.
        //
        */}

        <div className="w-36 flex-shrink-0 border-r border-subtle bg-[var(--bg-secondary)] flex flex-col">
          {tabs.map(tab => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 px-3 py-2.5 text-xs text-left transition-colors ${
                activeTab === tab.id
                  ? 'bg-[var(--highlight)] text-highlight border-l-2 border-[var(--accent-info)]'
                  : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] border-l-2 border-transparent'
              }`}
            >
              {tab.icon}
              <span className="font-medium">{tab.label}</span>
            </button>
          ))}

          <div className="flex-1" />

          {/*
          //
          // Link to agent scripts (too complex for modal).
          //
          */}

          <button
            onClick={() => { navigate('/settings?tab=agents'); onClose(); }}
            className="flex items-center gap-2 px-3 py-2.5 text-xs text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] transition-colors border-l-2 border-transparent"
          >
            <FileCode size={14} />
            <span>Agents</span>
            <ExternalLink size={10} className="ml-auto opacity-50" />
          </button>
        </div>

        {/*
        //
        // Content area.
        //
        */}

        <div className="flex-1 overflow-y-auto p-5">

          {/*
          //
          // Display tab.
          //
          */}

          {activeTab === 'display' && (
            <div className="space-y-5">
              <div>
                <h3 className="text-xs font-semibold text-highlight tracking-wider mb-0.5">INTERFACE MODE</h3>
                <p className="text-[10px] text-muted mb-3">Choose your preferred layout</p>

                <div className="space-y-1.5">
                  <button
                    onClick={() => handleModeChange('command_center')}
                    className={`w-full flex items-center gap-3 p-2.5 border transition-colors text-left ${
                      getUiMode() === 'command_center'
                        ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                        : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
                    }`}
                  >
                    <LayoutGrid size={16} className={getUiMode() === 'command_center' ? 'text-[var(--accent-info)]' : 'text-muted'} />
                    <div className="flex-1 min-w-0">
                      <p className={`text-xs font-medium ${getUiMode() === 'command_center' ? 'text-highlight' : 'text-[var(--text-primary)]'}`}>Command Center</p>
                      <p className="text-[10px] text-muted">Full-screen grid with node cards and orchestrator</p>
                    </div>
                    {getUiMode() === 'command_center' && (
                      <span className="text-[9px] tracking-wider text-[var(--accent-info)] flex-shrink-0">ACTIVE</span>
                    )}
                  </button>

                  <button
                    onClick={() => handleModeChange('legacy')}
                    className={`w-full flex items-center gap-3 p-2.5 border transition-colors text-left ${
                      getUiMode() === 'legacy'
                        ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                        : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
                    }`}
                  >
                    <Monitor size={16} className={getUiMode() === 'legacy' ? 'text-[var(--accent-info)]' : 'text-muted'} />
                    <div className="flex-1 min-w-0">
                      <p className={`text-xs font-medium ${getUiMode() === 'legacy' ? 'text-highlight' : 'text-[var(--text-primary)]'}`}>Classic</p>
                      <p className="text-[10px] text-muted">Sidebar navigation with dedicated pages</p>
                    </div>
                    {getUiMode() === 'legacy' && (
                      <span className="text-[9px] tracking-wider text-[var(--accent-info)] flex-shrink-0">ACTIVE</span>
                    )}
                  </button>
                </div>
              </div>

              <div className="pt-4 border-t border-subtle">
                <h3 className="text-xs font-semibold text-highlight tracking-wider mb-0.5">THEME</h3>
                <p className="text-[10px] text-muted mb-3">Visual appearance</p>

                <div className="flex gap-2">
                  <button
                    onClick={() => setTheme('origin_light')}
                    className={`flex-1 flex items-center justify-center gap-2 p-2.5 border transition-colors ${
                      theme === 'origin_light'
                        ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                        : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
                    }`}
                  >
                    <Sun size={14} className={theme === 'origin_light' ? 'text-[var(--accent-info)]' : 'text-muted'} />
                    <span className={`text-xs ${theme === 'origin_light' ? 'text-highlight' : 'text-muted'}`}>Light</span>
                  </button>
                  <button
                    onClick={() => setTheme('praxis_dark')}
                    className={`flex-1 flex items-center justify-center gap-2 p-2.5 border transition-colors ${
                      theme === 'praxis_dark'
                        ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                        : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
                    }`}
                  >
                    <Moon size={14} className={theme === 'praxis_dark' ? 'text-[var(--accent-info)]' : 'text-muted'} />
                    <span className={`text-xs ${theme === 'praxis_dark' ? 'text-highlight' : 'text-muted'}`}>Dark</span>
                  </button>
                </div>
              </div>
            </div>
          )}

          {/*
          //
          // LLM tab.
          //
          */}

          {activeTab === 'llm' && (
            <div className="space-y-4">
              <div>
                <h3 className="text-xs font-semibold text-highlight tracking-wider mb-0.5">LLM PROVIDERS</h3>
                <p className="text-[10px] text-muted">Model credentials and feature assignments</p>
              </div>

              {/*
              //
              // Sub-tab toggle.
              //
              */}

              <div className="flex gap-1 border-b border-subtle">
                {([
                  { id: 'models' as LLMView, label: 'Model Definitions' },
                  { id: 'features' as LLMView, label: 'Feature Config' },
                ]).map(v => (
                  <button
                    key={v.id}
                    onClick={() => setLlmView(v.id)}
                    className={`px-3 py-1.5 text-xs font-medium transition-colors border-b-2 -mb-px ${
                      llmView === v.id
                        ? 'text-highlight border-[var(--accent-info)]'
                        : 'text-muted hover:text-[var(--text-primary)] border-transparent'
                    }`}
                  >
                    {v.label}
                  </button>
                ))}
              </div>

              {/*
              //
              // Model Definitions view.
              //
              */}

              {llmView === 'models' && (
                <div className="space-y-3">
                  <div className="flex items-center justify-between">
                    <p className="text-[10px] text-muted">Define model credentials for feature assignment</p>
                    <button onClick={() => setIsAddingModel(true)} className={btnGreen}>
                      <Plus size={12} />
                      Add Model
                    </button>
                  </div>

                  {/*
                  //
                  // Add model form.
                  //
                  */}

                  {isAddingModel && (
                    <div className="p-3 bg-[var(--bg-secondary)] border border-dim space-y-3">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-semibold text-highlight">New Model Definition</span>
                        <button onClick={() => setIsAddingModel(false)} className="p-0.5 hover:bg-[var(--bg-tertiary)]">
                          <X size={14} />
                        </button>
                      </div>

                      <div className="grid grid-cols-2 gap-3">
                        <div>
                          <label className="block text-[10px] tracking-wider text-muted mb-1">Provider</label>
                          <select
                            value={newModel.provider}
                            onChange={e => setNewModel(m => ({ ...m, provider: e.target.value }))}
                            className={inputCls}
                          >
                            {providers.map(p => (
                              <option key={p.value} value={p.value}>{p.label}</option>
                            ))}
                          </select>
                        </div>
                        <div>
                          <label className="block text-[10px] tracking-wider text-muted mb-1">API Key</label>
                          <input
                            type="text"
                            value={newModel.apiKey}
                            onChange={e => setNewModel(m => ({ ...m, apiKey: e.target.value }))}
                            placeholder="sk-..."
                            className={inputCls}
                          />
                        </div>
                        <div className="col-span-2">
                          <label className="block text-[10px] tracking-wider text-muted mb-1">Model</label>
                          <div className="flex gap-1.5">
                            <input
                              type="text"
                              value={newModel.model}
                              onChange={e => setNewModel(m => ({ ...m, model: e.target.value }))}
                              placeholder="e.g., claude-sonnet-4-20250514"
                              className={`flex-1 ${inputCls}`}
                            />
                            <button
                              onClick={() => { setModelChooserTarget('new'); fetchModels(newModel.provider, newModel.apiKey); }}
                              disabled={!newModel.apiKey}
                              title={newModel.apiKey ? 'Browse models' : 'Enter API key first'}
                              className="px-1.5 py-1 bg-[var(--bg-primary)] border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors disabled:opacity-50"
                            >
                              <List size={14} />
                            </button>
                          </div>
                        </div>
                      </div>

                      {newModel.model && (
                        <p className="text-[10px] text-muted">
                          Name: <span className="font-mono text-highlight">{genName(newModel.provider, newModel.model)}</span>
                        </p>
                      )}

                      <div className="flex gap-2">
                        <button onClick={handleAddModel} disabled={!newModel.model.trim()} className={btnSave}>
                          <Plus size={12} /> Add
                        </button>
                        <button onClick={() => setIsAddingModel(false)} className="px-2.5 py-1 text-xs text-muted hover:text-highlight transition-colors">
                          Cancel
                        </button>
                      </div>
                    </div>
                  )}

                  {/*
                  //
                  // Model list.
                  //
                  */}

                  {modelDefinitions.length === 0 && !isAddingModel ? (
                    <div className="p-6 text-center text-muted border border-dashed border-subtle">
                      <Key size={24} className="mx-auto mb-2 opacity-50" />
                      <p className="text-xs">No model definitions yet</p>
                      <p className="text-[10px] mt-0.5">Add a model definition to get started</p>
                    </div>
                  ) : (
                    <div className="space-y-1.5">
                      {modelDefinitions.map(model => (
                        <div key={model.name} className="p-2.5 bg-[var(--bg-secondary)] border border-dim">
                          {editingModel?.name === model.name ? (
                            <div className="space-y-3">
                              <div className="grid grid-cols-2 gap-3">
                                <div>
                                  <label className="block text-[10px] tracking-wider text-muted mb-1">Provider</label>
                                  <select
                                    value={editingModel.provider}
                                    onChange={e => setEditingModel({ ...editingModel, provider: e.target.value })}
                                    className={inputCls}
                                  >
                                    {providers.map(p => (
                                      <option key={p.value} value={p.value}>{p.label}</option>
                                    ))}
                                  </select>
                                </div>
                                <div>
                                  <label className="block text-[10px] tracking-wider text-muted mb-1">API Key</label>
                                  <input
                                    type="text"
                                    value={editingModel.apiKey}
                                    onChange={e => setEditingModel({ ...editingModel, apiKey: e.target.value })}
                                    placeholder="sk-..."
                                    className={inputCls}
                                  />
                                </div>
                                <div className="col-span-2">
                                  <label className="block text-[10px] tracking-wider text-muted mb-1">Model</label>
                                  <div className="flex gap-1.5">
                                    <input
                                      type="text"
                                      value={editingModel.model}
                                      onChange={e => setEditingModel({ ...editingModel, model: e.target.value })}
                                      className={`flex-1 ${inputCls}`}
                                    />
                                    <button
                                      onClick={() => { setModelChooserTarget('edit'); fetchModels(editingModel.provider, editingModel.apiKey); }}
                                      disabled={!editingModel.apiKey}
                                      className="px-1.5 py-1 bg-[var(--bg-primary)] border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors disabled:opacity-50"
                                    >
                                      <List size={14} />
                                    </button>
                                  </div>
                                </div>
                              </div>
                              <div className="flex gap-2">
                                <button onClick={handleUpdateModel} className={btnSave}>
                                  <Check size={12} /> Update
                                </button>
                                <button onClick={() => setEditingModel(null)} className="px-2.5 py-1 text-xs text-muted hover:text-highlight transition-colors">
                                  Cancel
                                </button>
                              </div>
                            </div>
                          ) : (
                            <div className="flex items-center justify-between gap-2">
                              <div className="min-w-0">
                                <p className="font-mono text-xs text-highlight truncate">{model.name}</p>
                                <p className="text-[10px] text-muted">{providers.find(p => p.value === model.provider)?.label || model.provider}</p>
                              </div>
                              <div className="flex gap-1 flex-shrink-0">
                                <button
                                  onClick={() => setEditingModel(model)}
                                  className="p-1 text-muted hover:text-highlight hover:bg-[var(--bg-tertiary)] transition-colors"
                                  title="Edit"
                                >
                                  <Edit2 size={13} />
                                </button>
                                <button
                                  onClick={() => handleDeleteModel(model.name)}
                                  className="p-1 text-muted hover:text-[var(--accent-error)] hover:bg-[var(--accent-error)]/10 transition-colors"
                                  title="Delete"
                                >
                                  <Trash2 size={13} />
                                </button>
                              </div>
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  )}

                  {modelDefinitions.length > 0 && (
                    <button onClick={handleSaveModels} disabled={isSavingModels} className={btnSave}>
                      {showModelsSaved ? <><Check size={12} /> Saved</> : <><Save size={12} /> {isSavingModels ? 'Saving...' : 'Save Definitions'}</>}
                    </button>
                  )}
                </div>
              )}

              {/*
              //
              // Feature Config view.
              //
              */}

              {llmView === 'features' && (
                <div className="space-y-3">
                  {modelDefinitions.length === 0 ? (
                    <div className="p-6 text-center text-muted border border-dashed border-subtle">
                      <Key size={24} className="mx-auto mb-2 opacity-50" />
                      <p className="text-xs">No model definitions available</p>
                      <p className="text-[10px] mt-0.5">
                        <button onClick={() => setLlmView('models')} className="text-[var(--accent-info)] hover:underline">
                          Add model definitions
                        </button> to assign them to features
                      </p>
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {getFeatureFlags().orchestrator && (
                        <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                          <div className="w-32 flex-shrink-0">
                            <p className="text-xs font-medium text-highlight">Orchestrator</p>
                            <p className="text-[10px] text-muted">AI assistant</p>
                          </div>
                          <select
                            value={featureAssignments.orchestrator || ''}
                            onChange={e => setFeatureAssignments(a => ({ ...a, orchestrator: e.target.value || null }))}
                            className={`flex-1 ${inputCls}`}
                          >
                            <option value="">Select model...</option>
                            {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                          </select>
                          <input
                            type="number"
                            value={orchestratorMaxTokens}
                            onChange={e => setOrchestratorMaxTokens(e.target.value)}
                            placeholder="Max tokens"
                            min="1000"
                            max="100000"
                            className={`w-20 ${inputCls}`}
                            title="Max tokens"
                          />
                        </div>
                      )}

                      <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-32 flex-shrink-0">
                          <p className="text-xs font-medium text-highlight">Semantic Ops</p>
                          <p className="text-[10px] text-muted">Default for ops</p>
                        </div>
                        <select
                          value={featureAssignments.semanticOps || ''}
                          onChange={e => setFeatureAssignments(a => ({ ...a, semanticOps: e.target.value || null }))}
                          className={`flex-1 ${inputCls}`}
                        >
                          <option value="">Select model...</option>
                          {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                        </select>
                      </div>

                      <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-32 flex-shrink-0">
                          <p className="text-xs font-medium text-highlight">Semantic Parser</p>
                          <p className="text-[10px] text-muted">Tool call parsing</p>
                        </div>
                        <select
                          value={featureAssignments.semanticParser || ''}
                          onChange={e => setFeatureAssignments(a => ({ ...a, semanticParser: e.target.value || null }))}
                          className={`flex-1 ${inputCls}`}
                        >
                          <option value="">Select model...</option>
                          {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                        </select>
                      </div>

                      <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-32 flex-shrink-0">
                          <p className="text-xs font-medium text-highlight">Traffic Parser</p>
                          <p className="text-[10px] text-muted">Summarization</p>
                        </div>
                        <select
                          value={featureAssignments.trafficParser || ''}
                          onChange={e => setFeatureAssignments(a => ({ ...a, trafficParser: e.target.value || null }))}
                          className={`flex-1 ${inputCls}`}
                        >
                          <option value="">Select model...</option>
                          {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                        </select>
                      </div>
                    </div>
                  )}

                  {modelDefinitions.length > 0 && (
                    <div className="flex justify-end">
                      <button onClick={handleSaveFeatures} disabled={isSavingFeatures} className={btnSave}>
                        {showFeaturesSaved ? <><Check size={12} /> Saved</> : <><Save size={12} /> {isSavingFeatures ? 'Saving...' : 'Save Feature Config'}</>}
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}

          {/*
          //
          // Service tab.
          //
          */}

          {activeTab === 'service' && (
            <div className="space-y-5">
              <div>
                <h3 className="text-xs font-semibold text-highlight tracking-wider mb-0.5">SERVICE</h3>
                <p className="text-[10px] text-muted">Connection and service configuration</p>
              </div>

              {/*
              //
              // Connection info.
              //
              */}

              <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[10px]">
                <span className="text-muted">Status</span>
                <span className="flex items-center gap-1.5">
                  {state.connected ? (
                    <><Wifi size={10} className="status-online" /><span className="status-online">Connected</span></>
                  ) : (
                    <><WifiOff size={10} className="status-offline" /><span className="status-offline">Disconnected</span></>
                  )}
                </span>
                {state.clientId && (
                  <>
                    <span className="text-muted">Client ID</span>
                    <span className="font-mono text-muted">{state.clientId}</span>
                  </>
                )}
                <span className="text-muted">WebSocket</span>
                <span className="font-mono text-muted">
                  {`${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`}
                </span>
                <span className="text-muted">Version</span>
                <span className="font-mono text-muted">{state.version ?? 'unknown'}</span>
              </div>

              {/*
              //
              // Event Logging.
              //
              */}

              <div className="pt-4 border-t border-subtle">
                <h4 className="text-xs font-semibold text-highlight mb-1">Event Logging</h4>
                <p className="text-[10px] text-muted mb-2">Centralized application logs</p>

                <div className="flex items-center gap-3">
                  <button onClick={handleEventLoggingToggle} className="flex items-center gap-1.5 text-xs text-muted hover:text-highlight transition-colors">
                    {eventLoggingEnabled
                      ? <CircleCheck size={14} className="text-[var(--accent-success)]" />
                      : <Circle size={14} className="text-[var(--text-secondary)]" />}
                    <span>{eventLoggingEnabled ? 'Enabled' : 'Disabled'}</span>
                  </button>

                  {!showClearConfirm ? (
                    <button onClick={() => setShowClearConfirm(true)} className="flex items-center gap-1 text-[10px] text-muted hover:text-highlight transition-colors">
                      <Trash2 size={11} /> Clear
                    </button>
                  ) : (
                    <div className="flex items-center gap-2 text-[10px]">
                      <span className="text-muted">Clear all?</span>
                      <button onClick={() => { clearEventLog(); setShowClearConfirm(false); }} className="text-[var(--accent-error)] hover:underline">Confirm</button>
                      <button onClick={() => setShowClearConfirm(false)} className="text-muted hover:text-highlight">Cancel</button>
                    </div>
                  )}
                </div>

                <div className="flex items-center gap-2 mt-2">
                  <label className="text-[10px] text-muted">Row limit</label>
                  <input
                    type="number"
                    value={huntingQueryRowLimit}
                    onChange={e => setHuntingQueryRowLimit(e.target.value)}
                    onBlur={() => {
                      const n = parseInt(huntingQueryRowLimit, 10);
                      if (n > 0) setConfig({ hunting_query_row_limit: huntingQueryRowLimit });
                    }}
                    min="1"
                    className={`w-28 ${inputCls}`}
                  />
                  <span className="text-[10px] text-muted">per table</span>
                </div>
              </div>

              {/*
              //
              // MCP Server.
              //
              */}

              <div className="pt-4 border-t border-subtle">
                <h4 className="text-xs font-semibold text-highlight mb-1">MCP Server</h4>
                <p className="text-[10px] text-muted mb-2">Expose tools via Model Context Protocol (SSE)</p>

                <div className="space-y-2">
                  <button onClick={handleMcpToggle} className="flex items-center gap-1.5 text-xs text-muted hover:text-highlight transition-colors">
                    {mcpServerEnabled
                      ? <CircleCheck size={14} className="text-[var(--accent-success)]" />
                      : <Circle size={14} className="text-[var(--text-secondary)]" />}
                    <span>{mcpServerEnabled ? 'Enabled' : 'Disabled'}</span>
                  </button>

                  {mcpServerEnabled && (
                    <div className="flex items-center gap-2 pl-5">
                      <label className="text-[10px] text-muted">Port</label>
                      <input
                        type="number"
                        value={mcpServerPort}
                        onChange={e => setMcpServerPort(e.target.value)}
                        onBlur={handleMcpPortSave}
                        min="1"
                        max="65535"
                        className={`w-20 ${inputCls}`}
                      />
                      <span className="text-[10px] text-muted font-mono">http://localhost:{mcpServerPort}/sse</span>
                    </div>
                  )}
                </div>
              </div>

              {/*
              //
              // Node downloads.
              //
              */}

              <div className="pt-4 border-t border-subtle">
                <h4 className="text-xs font-semibold text-highlight mb-1">Node Downloads</h4>
                <p className="text-[10px] text-muted mb-2">Download node agent for target machines</p>

                {isLoadingDownloads ? (
                  <div className="flex items-center gap-2 text-muted">
                    <Loader2 size={14} className="animate-spin" />
                    <span className="text-[10px]">Loading...</span>
                  </div>
                ) : nodeDownloads.length === 0 ? (
                  <p className="text-[10px] text-muted">No node binaries available</p>
                ) : (
                  <div className="space-y-1.5">
                    {nodeDownloads.map(node => (
                      <div key={node.platform} className="flex items-center justify-between p-2 bg-[var(--bg-secondary)] border border-dim">
                        <div className="flex items-center gap-2">
                          <Monitor size={14} className="text-muted" />
                          <div>
                            <span className="text-xs font-medium capitalize">{node.platform}</span>
                            <p className="text-[10px] text-muted">
                              {node.filename}
                              {node.available && node.size && (
                                <span className="ml-1">({(node.size / 1024 / 1024).toFixed(1)} MB)</span>
                              )}
                            </p>
                          </div>
                        </div>
                        {node.available ? (
                          <a
                            href={`/api/downloads/node/${node.platform}`}
                            download={node.filename}
                            className={btnSave}
                          >
                            <Download size={12} /> Download
                          </a>
                        ) : (
                          <span className="text-[10px] text-muted italic">N/A</span>
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          )}

          {/*
          //
          // About tab.
          //
          */}

          {activeTab === 'about' && (
            <div className="space-y-5">
              <div>
                <h3 className="text-xs font-semibold text-[var(--accent-success)] tracking-wider mb-3">
                  PRAXIS BY [&Oslash;] ORIGIN
                </h3>
                <p className="text-xs text-muted leading-relaxed mb-4">
                  <a href="https://originhq.com" target="_blank" rel="noopener noreferrer" className="text-[var(--accent-info)]/70 hover:text-[var(--accent-info)] hover:underline">Origin</a> is
                  an endpoint security company building protection for the semantic era of computing.
                  As AI agents become integral to enterprise workflows, Origin provides the visibility
                  and control organizations need to safely grant agents the permissions they require.
                </p>
                <p className="text-xs text-muted leading-relaxed mb-5">
                  <a href="https://github.com/originsec/praxis" target="_blank" rel="noopener noreferrer" className="text-[var(--accent-info)]/70 hover:text-[var(--accent-info)] hover:underline">Praxis</a> is
                  Origin's experimental research platform for exploring the adversarial boundaries of
                  legitimate semantic tools. By understanding how computer-use agents and their
                  underlying capabilities can be leveraged offensively, we build better defenses for
                  the endpoints they operate on.
                </p>

                <div className="flex gap-3">
                  <a
                    href="https://originhq.com"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors"
                  >
                    <ExternalLink size={12} /> originhq.com
                  </a>
                  <a
                    href="https://praxis.originhq.com"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors"
                  >
                    <ExternalLink size={12} /> praxis.originhq.com
                  </a>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>

      {/*
      //
      // Model Chooser overlay.
      //
      */}

      {showModelChooser && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-[60]">
          <div className="bg-[var(--bg-card)] border border-subtle ascii-box w-full max-w-sm max-h-[60vh] flex flex-col">
            <div className="flex items-center justify-between px-3 py-2 border-b border-subtle">
              <span className="text-xs font-semibold text-highlight">Choose Model</span>
              <button
                onClick={() => { setShowModelChooser(false); setModelChooserTarget(null); }}
                className="p-0.5 hover:bg-[var(--bg-tertiary)]"
              >
                <X size={16} />
              </button>
            </div>
            <div className="flex-1 overflow-y-auto p-2">
              {isLoadingModels && (
                <div className="flex items-center justify-center py-6">
                  <Loader2 className="animate-spin" size={18} />
                  <span className="ml-2 text-xs text-muted">Loading...</span>
                </div>
              )}
              {modelError && (
                <div className="p-3 text-xs bg-[var(--accent-error)]/10 text-[var(--accent-error)]">{modelError}</div>
              )}
              {!isLoadingModels && !modelError && availableModels.length === 0 && (
                <div className="text-center text-muted py-6 text-xs">No models available</div>
              )}
              {!isLoadingModels && availableModels.length > 0 && (
                <div className="space-y-0.5">
                  {availableModels.map(model => (
                    <button
                      key={model}
                      onClick={() => handleModelSelect(model)}
                      className="w-full text-left px-3 py-2 hover:bg-[var(--bg-tertiary)] transition-colors text-xs"
                    >
                      {model}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </Modal>
  );
}
