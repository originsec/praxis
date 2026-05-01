import { useState, useEffect, useCallback } from 'react';
import {
  Monitor, Cpu, Server, Info, Wifi, WifiOff,
  Plus, Trash2, Edit2, Save, Check, X, Key, List, Loader2,
  Circle, CircleCheck, Download, ExternalLink, FileCode,
  Upload, RotateCcw, AlertTriangle,
} from 'lucide-react';
import { Modal } from '../common/Modal';
import { LuaCodeEditor } from '../common/LuaCodeEditor';
import { useApp } from '../../context/AppContext';
import { getFeatureFlags } from '../../utils/featureFlags';

type Tab = 'llm' | 'agents' | 'intercept' | 'service' | 'about';
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
  baseUrl?: string;
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
  const {
    state, getConfig, setConfig, clearEventLog,
    listLuaAgentScripts, addLuaAgentScript, updateLuaAgentScript,
    deleteLuaAgentScript, resetLuaAgentScriptDefaults, toggleLuaAgentScriptDisabled,
    listInterceptTargets, addInterceptTarget, updateInterceptTarget,
    deleteInterceptTarget, toggleInterceptTargetDisabled,
  } = useApp();


  const [activeTab, setActiveTab] = useState<Tab>('llm');
  const [llmView, setLlmView] = useState<LLMView>('models');

  //
  // Praxis agent settings.
  //

  const [praxisModelRef, setPraxisModelRef] = useState('');
  const [praxisThinkingEffort, setPraxisThinkingEffort] = useState('');
  const [praxisEnabled, setPraxisEnabled] = useState(false);
  const [praxisSystemPrompt, setPraxisSystemPrompt] = useState('');

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
    baseUrl: '',
  });

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
  const [logQueryRowLimit, setLogQueryRowLimit] = useState('10000000');
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [mcpServerEnabled, setMcpServerEnabled] = useState(true);
  const [mcpServerPort, setMcpServerPort] = useState('8585');
  const [promptTimeoutSecs, setPromptTimeoutSecs] = useState('600');
  const [ccrV1Enabled, setCcrV1Enabled] = useState(false);
  const [ccrV1Port, setCcrV1Port] = useState('8586');
  const [ccrV2Enabled, setCcrV2Enabled] = useState(false);
  const [ccrV2Port, setCcrV2Port] = useState('8587');
  const [nodeDownloads, setNodeDownloads] = useState<NodeDownloadInfo[]>([]);
  const [isLoadingDownloads, setIsLoadingDownloads] = useState(false);

  //
  // Agent script state.
  //

  const [selectedScriptId, setSelectedScriptId] = useState<string | null>(null);
  const [editingScriptName, setEditingScriptName] = useState('');
  const [editingScriptContent, setEditingScriptContent] = useState('');
  const [isEditingScript, setIsEditingScript] = useState(false);
  const [isAddingScript, setIsAddingScript] = useState(false);
  const [showResetModal, setShowResetModal] = useState(false);
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [deletingScriptId, setDeletingScriptId] = useState<string | null>(null);
  const [showBuiltinWarning, setShowBuiltinWarning] = useState(false);

  //
  // Intercept target form state. editingTargetId === null with isEditingTarget
  // == true means "create new"; else it's an edit on the named id.
  //

  const [isEditingTarget, setIsEditingTarget] = useState(false);
  const [editingTargetId, setEditingTargetId] = useState<string | null>(null);
  const [targetForm, setTargetForm] = useState<{
    name: string;
    agent_short_name: string;
    domains: string;
    url_pattern: string;
  }>({ name: '', agent_short_name: '', domains: '', url_pattern: '' });
  const [targetFormError, setTargetFormError] = useState<string | null>(null);
  const [confirmDeleteTargetId, setConfirmDeleteTargetId] = useState<string | null>(null);

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
      'log_query_row_limit',
      'mcp_server_enabled',
      'mcp_server_port',
      'prompt_timeout_secs',
      'claude_ccrv1_enabled',
      'claude_ccrv1_port',
      'claude_ccrv2_enabled',
      'claude_ccrv2_port',
      'praxis_agent_settings',
      'praxis_agent_system_prompt',
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
  // Load agent scripts when agents tab becomes active.
  //

  useEffect(() => {
    if (activeTab === 'agents' && state.connected) {
      listLuaAgentScripts();
    }
  }, [activeTab, state.connected, listLuaAgentScripts]);

  //
  // Load intercept targets when the intercept tab becomes active.
  //

  useEffect(() => {
    if (activeTab === 'intercept' && state.connected) {
      listInterceptTargets();
    }
  }, [activeTab, state.connected, listInterceptTargets]);

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

    setLogQueryRowLimit(cfg.log_query_row_limit || '10000000');

    if (cfg.mcp_server_enabled) {
      const v = cfg.mcp_server_enabled.toLowerCase();
      setMcpServerEnabled(!(v === 'false' || v === '0' || v === 'no'));
    } else {
      setMcpServerEnabled(true);
    }
    setMcpServerPort(cfg.mcp_server_port || '8585');
    setPromptTimeoutSecs(cfg.prompt_timeout_secs || '600');

    if (cfg.claude_ccrv1_enabled) {
      const v = cfg.claude_ccrv1_enabled.toLowerCase();
      setCcrV1Enabled(!(v === 'false' || v === '0' || v === 'no'));
    } else {
      setCcrV1Enabled(false);
    }
    setCcrV1Port(cfg.claude_ccrv1_port || '8586');
    if (cfg.claude_ccrv2_enabled) {
      const v = cfg.claude_ccrv2_enabled.toLowerCase();
      setCcrV2Enabled(!(v === 'false' || v === '0' || v === 'no'));
    } else {
      setCcrV2Enabled(false);
    }
    setCcrV2Port(cfg.claude_ccrv2_port || '8587');

    if (cfg.praxis_agent_settings) {
      try {
        const s = JSON.parse(cfg.praxis_agent_settings);
        setPraxisModelRef(s.modelRef || '');
        setPraxisThinkingEffort(s.thinkingEffort || '');
        setPraxisEnabled(!!s.enabled);
      } catch { /* ignore */ }
    }
    if (cfg.praxis_agent_system_prompt) {
      setPraxisSystemPrompt(cfg.praxis_agent_system_prompt);
    }
  }, [state.config]);

  //
  // Auto-save helpers. Persist model definitions to backend whenever they change.
  //

  const saveModels = useCallback((defs: ModelDefinition[]) => {
    setConfig({ llm_model_definitions: JSON.stringify(defs) });
  }, [setConfig]);

  const saveFeatures = useCallback((assignments: FeatureAssignments, maxTokens: string) => {
    setConfig({
      llm_feature_orchestrator: assignments.orchestrator || '',
      llm_feature_semantic_ops: assignments.semanticOps || '',
      llm_feature_semantic_parser: assignments.semanticParser || '',
      llm_feature_traffic_parser: assignments.trafficParser || '',
      llm_orchestrator_max_tokens: maxTokens,
    });
  }, [setConfig]);

  const savePraxisSettings = useCallback(() => {
    setConfig({
      praxis_agent_settings: JSON.stringify({
        modelRef: praxisModelRef,
        thinkingEffort: praxisThinkingEffort,
        enabled: praxisEnabled,
      }),
      praxis_agent_system_prompt: praxisSystemPrompt,
    });
  }, [setConfig, praxisModelRef, praxisThinkingEffort, praxisEnabled, praxisSystemPrompt]);

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
    const def: ModelDefinition = { name, ...newModel };
    if (!def.baseUrl) delete def.baseUrl;
    const updated = [...modelDefinitions, def];
    setModelDefinitions(updated);
    saveModels(updated);
    setNewModel({ provider: 'anthropic', model: '', apiKey: '', baseUrl: '' });
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
    const cleanModel = { ...editingModel, name: newName };
    if (!cleanModel.baseUrl) delete cleanModel.baseUrl;
    const updated = modelDefinitions.map(m =>
      m.name === oldName ? cleanModel : m
    );
    setModelDefinitions(updated);
    saveModels(updated);
    if (newName !== oldName) {
      const a = { ...featureAssignments };
      if (a.orchestrator === oldName) a.orchestrator = newName;
      if (a.semanticOps === oldName) a.semanticOps = newName;
      if (a.semanticParser === oldName) a.semanticParser = newName;
      if (a.trafficParser === oldName) a.trafficParser = newName;
      setFeatureAssignments(a);
      saveFeatures(a, orchestratorMaxTokens);
    }
    setEditingModel(null);
  };

  const handleDeleteModel = (name: string) => {
    if (!confirm(`Delete model "${name}"?`)) return;
    const updated = modelDefinitions.filter(m => m.name !== name);
    setModelDefinitions(updated);
    saveModels(updated);
    const a = { ...featureAssignments };
    if (a.orchestrator === name) a.orchestrator = null;
    if (a.semanticOps === name) a.semanticOps = null;
    if (a.semanticParser === name) a.semanticParser = null;
    if (a.trafficParser === name) a.trafficParser = null;
    setFeatureAssignments(a);
    saveFeatures(a, orchestratorMaxTokens);
  };

  //
  // Feature assignment change — auto-saves immediately.
  //

  const handleFeatureChange = (key: keyof FeatureAssignments, value: string | null) => {
    const updated = { ...featureAssignments, [key]: value };
    setFeatureAssignments(updated);
    saveFeatures(updated, orchestratorMaxTokens);
  };

  const handleMaxTokensBlur = () => {
    saveFeatures(featureAssignments, orchestratorMaxTokens);
  };

  const isLocalProvider = (p: string) => p === 'ollama' || p === 'custom';

  const fetchModels = async (provider: string, apiKey: string, baseUrl?: string) => {
    setModelError(null);
    setIsLoadingModels(true);
    setShowModelChooser(true);
    setAvailableModels([]);
    if (!apiKey && !isLocalProvider(provider)) {
      setModelError('API key is required to fetch models');
      setIsLoadingModels(false);
      return;
    }
    if (provider === 'custom' && !baseUrl) {
      setModelError('Base URL is required for Custom provider');
      setIsLoadingModels(false);
      return;
    }
    try {
      const body: Record<string, string> = { provider, api_key: apiKey };
      if (baseUrl) body.base_url = baseUrl;
      const response = await fetch('/api/models', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
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

  const handleCcrV1Toggle = () => {
    const next = !ccrV1Enabled;
    setCcrV1Enabled(next);
    setConfig({ claude_ccrv1_enabled: next ? 'true' : 'false' });
  };

  const handleCcrV1PortSave = () => {
    const port = parseInt(ccrV1Port, 10);
    if (port > 0 && port < 65536) {
      setConfig({ claude_ccrv1_port: ccrV1Port });
    }
  };

  const handleCcrV2Toggle = () => {
    const next = !ccrV2Enabled;
    setCcrV2Enabled(next);
    setConfig({ claude_ccrv2_enabled: next ? 'true' : 'false' });
  };

  const handleCcrV2PortSave = () => {
    const port = parseInt(ccrV2Port, 10);
    if (port > 0 && port < 65536) {
      setConfig({ claude_ccrv2_port: ccrV2Port });
    }
  };

  //
  // Agent script handlers.
  //

  const handleSelectScript = (scriptId: string) => {
    const script = state.luaAgentScripts.find(s => s.id === scriptId);
    if (script) {
      setSelectedScriptId(scriptId);
      setEditingScriptName(script.name);
      setEditingScriptContent(script.script);
      setIsEditingScript(false);
      setIsAddingScript(false);
    }
  };

  const handleSaveScript = () => {
    if (!editingScriptName.trim()) return;
    if (isAddingScript) {
      addLuaAgentScript(editingScriptName, editingScriptContent);
    } else if (selectedScriptId) {
      updateLuaAgentScript(selectedScriptId, editingScriptName, editingScriptContent);
    }
    setIsEditingScript(false);
    setIsAddingScript(false);
  };

  const handleDeleteScript = (scriptId: string) => {
    setDeletingScriptId(scriptId);
    setShowDeleteModal(true);
  };

  const handleConfirmDelete = () => {
    if (deletingScriptId) {
      deleteLuaAgentScript(deletingScriptId);
      if (selectedScriptId === deletingScriptId) {
        setSelectedScriptId(null);
        setEditingScriptName('');
        setEditingScriptContent('');
        setIsEditingScript(false);
      }
    }
    setShowDeleteModal(false);
    setDeletingScriptId(null);
  };

  const handleConfirmReset = () => {
    resetLuaAgentScriptDefaults();
    setSelectedScriptId(null);
    setEditingScriptName('');
    setEditingScriptContent('');
    setIsEditingScript(false);
    setIsAddingScript(false);
    setShowResetModal(false);
  };

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      const content = ev.target?.result as string;
      const name = file.name.replace(/\.lua$/, '');
      setSelectedScriptId(null);
      setEditingScriptName(name);
      setEditingScriptContent(content);
      setIsEditingScript(true);
      setIsAddingScript(true);
    };
    reader.readAsText(file);
    e.target.value = '';
  };

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'llm', label: 'LLM', icon: <Cpu size={14} /> },
    { id: 'agents', label: 'Agents', icon: <FileCode size={14} /> },
    { id: 'intercept', label: 'Intercept', icon: <Wifi size={14} /> },
    { id: 'service', label: 'Service', icon: <Server size={14} /> },
    { id: 'about', label: 'About', icon: <Info size={14} /> },
  ];

  //
  // Shared styling for select/input elements.
  //

  const inputCls = 'w-full bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors';
  const btnSave = 'inline-flex items-center gap-1.5 px-2.5 py-1 text-xs bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors disabled:opacity-50';
  const btnGreen = 'inline-flex items-center gap-1.5 px-2.5 py-1 text-xs rounded bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors';

  return (
    <Modal isOpen={true} onClose={onClose} title="Settings" size="xl" noPadding resizable storageKey="cmd-settings" defaultWidth={760} defaultHeight={Math.round(window.innerHeight * 0.7)}>
      <div className="flex h-full">

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
        </div>

        {/*
        //
        // Content area.
        //
        */}

        <div className={`flex-1 flex flex-col min-h-0 ${activeTab === 'agents' ? '' : 'overflow-y-auto p-5'}`}>

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
                    onClick={() => { setLlmView(v.id); }}
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
                            onChange={e => setNewModel(m => ({ ...m, provider: e.target.value, baseUrl: e.target.value === 'ollama' ? 'http://localhost:11434/v1' : e.target.value === 'custom' ? '' : '' }))}
                            className={inputCls}
                          >
                            {providers.map(p => (
                              <option key={p.value} value={p.value}>{p.label}</option>
                            ))}
                          </select>
                        </div>
                        <div>
                          <label className="block text-[10px] tracking-wider text-muted mb-1">
                            API Key {isLocalProvider(newModel.provider) && <span className="text-muted">(optional)</span>}
                          </label>
                          <input
                            type="text"
                            value={newModel.apiKey}
                            onChange={e => setNewModel(m => ({ ...m, apiKey: e.target.value }))}
                            placeholder={isLocalProvider(newModel.provider) ? '(optional)' : 'sk-...'}
                            className={inputCls}
                          />
                        </div>
                        {isLocalProvider(newModel.provider) && (
                          <div className="col-span-2">
                            <label className="block text-[10px] tracking-wider text-muted mb-1">
                              Base URL {newModel.provider === 'custom' && <span className="text-[var(--accent-error)]">*</span>}
                            </label>
                            <input
                              type="text"
                              value={newModel.baseUrl || ''}
                              onChange={e => setNewModel(m => ({ ...m, baseUrl: e.target.value }))}
                              placeholder={newModel.provider === 'ollama' ? 'http://localhost:11434/v1' : 'http://localhost:8000/v1'}
                              className={inputCls}
                            />
                            <p className="text-[9px] text-muted mt-0.5">
                              {newModel.provider === 'ollama'
                                ? 'Ollama OpenAI-compatible endpoint (default: localhost:11434/v1)'
                                : 'OpenAI-compatible API endpoint (vLLM, llama.cpp, LM Studio, etc.)'}
                            </p>
                          </div>
                        )}
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
                              onClick={() => { setModelChooserTarget('new'); fetchModels(newModel.provider, newModel.apiKey, newModel.baseUrl); }}
                              disabled={!newModel.apiKey && !isLocalProvider(newModel.provider)}
                              title={newModel.apiKey || isLocalProvider(newModel.provider) ? 'Browse models' : 'Enter API key first'}
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
                                    onChange={e => setEditingModel({ ...editingModel, provider: e.target.value, baseUrl: e.target.value === 'ollama' ? (editingModel.baseUrl || 'http://localhost:11434/v1') : e.target.value === 'custom' ? (editingModel.baseUrl || '') : editingModel.baseUrl })}
                                    className={inputCls}
                                  >
                                    {providers.map(p => (
                                      <option key={p.value} value={p.value}>{p.label}</option>
                                    ))}
                                  </select>
                                </div>
                                <div>
                                  <label className="block text-[10px] tracking-wider text-muted mb-1">
                                    API Key {isLocalProvider(editingModel.provider) && <span className="text-muted">(optional)</span>}
                                  </label>
                                  <input
                                    type="text"
                                    value={editingModel.apiKey}
                                    onChange={e => setEditingModel({ ...editingModel, apiKey: e.target.value })}
                                    placeholder={isLocalProvider(editingModel.provider) ? '(optional)' : 'sk-...'}
                                    className={inputCls}
                                  />
                                </div>
                                {isLocalProvider(editingModel.provider) && (
                                  <div className="col-span-2">
                                    <label className="block text-[10px] tracking-wider text-muted mb-1">
                                      Base URL {editingModel.provider === 'custom' && <span className="text-[var(--accent-error)]">*</span>}
                                    </label>
                                    <input
                                      type="text"
                                      value={editingModel.baseUrl || ''}
                                      onChange={e => setEditingModel({ ...editingModel, baseUrl: e.target.value })}
                                      placeholder={editingModel.provider === 'ollama' ? 'http://localhost:11434/v1' : 'http://localhost:8000/v1'}
                                      className={inputCls}
                                    />
                                  </div>
                                )}
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
                                      onClick={() => { setModelChooserTarget('edit'); fetchModels(editingModel.provider, editingModel.apiKey, editingModel.baseUrl); }}
                                      disabled={!editingModel.apiKey && !isLocalProvider(editingModel.provider)}
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
                                <p className="text-xs truncate">
                                  <span className="text-muted">{providers.find(p => p.value === model.provider)?.label || model.provider}</span>
                                  {' '}
                                  <span className="text-highlight">{model.model}</span>
                                </p>
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
                          <div className="w-28 flex-shrink-0">
                            <p className="text-xs font-medium text-highlight">Orchestrator</p>
                            <p className="text-[10px] text-muted">Default model</p>
                          </div>
                          <select
                            value={featureAssignments.orchestrator || ''}
                            onChange={e => handleFeatureChange('orchestrator', e.target.value || null)}
                            className="flex-1 min-w-0 bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
                          >
                            <option value="">Select model...</option>
                            {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                          </select>
                          <input
                            type="text"
                            inputMode="numeric"
                            pattern="[0-9]*"
                            value={orchestratorMaxTokens}
                            onChange={e => {
                              const v = e.target.value.replace(/[^0-9]/g, '');
                              setOrchestratorMaxTokens(v);
                            }}
                            onBlur={handleMaxTokensBlur}
                            placeholder="Tokens"
                            title="Max tokens"
                            className="w-16 flex-shrink-0 bg-[var(--bg-primary)] border border-dim px-2.5 py-1.5 text-xs text-highlight focus:outline-none focus:border-subtle transition-colors"
                          />
                        </div>
                      )}

                      <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-28 flex-shrink-0">
                          <p className="text-xs font-medium text-highlight">Semantic Ops</p>
                          <p className="text-[10px] text-muted">Default for ops</p>
                        </div>
                        <select
                          value={featureAssignments.semanticOps || ''}
                          onChange={e => handleFeatureChange('semanticOps', e.target.value || null)}
                          className={`flex-1 ${inputCls}`}
                        >
                          <option value="">Select model...</option>
                          {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                        </select>
                      </div>

                      <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-28 flex-shrink-0">
                          <p className="text-xs font-medium text-highlight">Semantic Parser</p>
                          <p className="text-[10px] text-muted">Tool call parsing</p>
                        </div>
                        <select
                          value={featureAssignments.semanticParser || ''}
                          onChange={e => handleFeatureChange('semanticParser', e.target.value || null)}
                          className={`flex-1 ${inputCls}`}
                        >
                          <option value="">Select model...</option>
                          {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                        </select>
                      </div>

                      <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-28 flex-shrink-0">
                          <p className="text-xs font-medium text-highlight">Traffic Parser</p>
                          <p className="text-[10px] text-muted">Summarization</p>
                        </div>
                        <select
                          value={featureAssignments.trafficParser || ''}
                          onChange={e => handleFeatureChange('trafficParser', e.target.value || null)}
                          className={`flex-1 ${inputCls}`}
                        >
                          <option value="">Select model...</option>
                          {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                        </select>
                      </div>
                    </div>
                  )}
                </div>
              )}

            </div>
          )}

          {/*
          //
          // Agents tab.
          //
          */}

          {activeTab === 'agents' && (
            <div className="flex flex-col flex-1 min-h-0 p-5 pb-0">
              <div className="mb-5 flex-shrink-0">
                <h3 className="text-xs font-semibold text-highlight tracking-wider mb-0.5">PRAXIS AGENT</h3>
                <p className="text-[10px] text-muted mb-3">Native agent connector exposed by nodes when enabled.</p>

                {modelDefinitions.length === 0 ? (
                  <div className="p-6 text-center text-muted border border-dashed border-subtle">
                    <Key size={24} className="mx-auto mb-2 opacity-50" />
                    <p className="text-xs">No model definitions available</p>
                    <p className="text-[10px] mt-0.5">
                      <button onClick={() => { setActiveTab('llm'); setLlmView('models'); }} className="text-[var(--accent-info)] hover:underline">
                        Add model definitions
                      </button> to configure the Praxis Agent
                    </p>
                  </div>
                ) : (
                  <div className="space-y-3">
                    <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                      <div className="w-32 flex-shrink-0">
                        <p className="text-xs font-medium text-highlight">Enabled</p>
                        <p className="text-[10px] text-muted">Expose connector</p>
                      </div>
                      <button
                        onClick={() => setPraxisEnabled(v => !v)}
                        className="flex items-center gap-1.5 text-xs text-muted hover:text-highlight transition-colors"
                      >
                        {praxisEnabled
                          ? <CircleCheck size={14} className="text-[var(--accent-success)]" />
                          : <Circle size={14} className="text-[var(--text-secondary)]" />}
                        {praxisEnabled ? 'Enabled' : 'Disabled'}
                      </button>
                    </div>

                    <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                      <div className="w-32 flex-shrink-0">
                        <p className="text-xs font-medium text-highlight">Model</p>
                        <p className="text-[10px] text-muted">Session backend</p>
                      </div>
                      <select
                        value={praxisModelRef}
                        onChange={e => setPraxisModelRef(e.target.value)}
                        className={`flex-1 ${inputCls}`}
                      >
                        <option value="">Select model...</option>
                        {modelDefinitions.map(m => <option key={m.name} value={m.name}>{m.name}</option>)}
                      </select>
                    </div>

                    <div className="flex items-center gap-3 p-2.5 bg-[var(--bg-secondary)] border border-dim">
                      <div className="w-32 flex-shrink-0">
                        <p className="text-xs font-medium text-highlight">Thinking Effort</p>
                        <p className="text-[10px] text-muted">Model-specific</p>
                      </div>
                      <input
                        type="text"
                        value={praxisThinkingEffort}
                        onChange={e => setPraxisThinkingEffort(e.target.value)}
                        placeholder="low, medium, high"
                        className={`flex-1 ${inputCls}`}
                      />
                    </div>

                    <div className="p-2.5 bg-[var(--bg-secondary)] border border-dim space-y-2">
                      <div>
                        <p className="text-xs font-medium text-highlight">System Prompt</p>
                        <p className="text-[10px] text-muted">Prompt sent to the model for Praxis agent sessions.</p>
                      </div>
                      <textarea
                        value={praxisSystemPrompt}
                        onChange={e => setPraxisSystemPrompt(e.target.value)}
                        rows={4}
                        placeholder="You are Praxis, an autonomous agent running on the target system..."
                        className={`${inputCls} resize-y font-mono min-h-[6rem]`}
                      />
                    </div>

                    <div className="flex justify-end">
                      <button onClick={savePraxisSettings} className={btnSave}>
                        <Save size={12} /> Save Praxis Agent
                      </button>
                    </div>
                  </div>
                )}
              </div>

              <div className="flex items-center justify-between mb-3 flex-shrink-0">
                <div>
                  <h3 className="text-xs font-semibold text-highlight tracking-wider mb-0.5">AGENT DEFINITIONS</h3>
                  <p className="text-[10px] text-muted">Lua agent connector scripts</p>
                </div>
                <div className="flex gap-1.5">
                  <label className={`${btnGreen} cursor-pointer`}>
                    <Upload size={12} />
                    Upload
                    <input type="file" accept=".lua" onChange={handleFileUpload} className="hidden" />
                  </label>
                  <button
                    onClick={() => setShowResetModal(true)}
                    className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors"
                    title="Reset all scripts to built-in defaults"
                  >
                    <RotateCcw size={12} />
                    Reset
                  </button>
                </div>
              </div>

              <div className="flex flex-1 gap-3 min-h-0 pb-5">

                {/*
                //
                // Script list.
                //
                */}

                <div className="w-44 flex-shrink-0 border border-dim overflow-y-auto">
                  {state.luaAgentScripts.length === 0 ? (
                    <div className="p-3 text-center text-muted text-[10px]">No scripts</div>
                  ) : (
                    state.luaAgentScripts.map(script => (
                      <div
                        key={script.id}
                        onClick={() => handleSelectScript(script.id)}
                        className={`group flex items-center justify-between px-2.5 py-1.5 cursor-pointer transition-colors ${
                          selectedScriptId === script.id
                            ? 'bg-[var(--highlight)] text-highlight'
                            : script.disabled
                              ? 'hover:bg-[var(--bg-tertiary)] text-muted opacity-50'
                              : 'hover:bg-[var(--bg-tertiary)] text-muted'
                        }`}
                      >
                        <div className="flex items-center gap-1 min-w-0">
                          <span className="text-[11px] truncate">{script.name}</span>
                          {script.is_builtin && (
                            <span className="text-[7px] leading-tight px-0.5 rounded bg-[var(--accent-info)]/15 text-[var(--accent-info)]/70 flex-shrink-0">
                              builtin
                            </span>
                          )}
                        </div>
                        <div className="flex items-center gap-0 flex-shrink-0">
                          <button
                            onClick={e => { e.stopPropagation(); toggleLuaAgentScriptDisabled(script.id, !script.disabled); }}
                            className={`p-0.5 transition-colors ${
                              script.disabled
                                ? 'text-[var(--accent-warning)]'
                                : 'text-muted hover:text-[var(--accent-success)] opacity-0 group-hover:opacity-100'
                            }`}
                            title={script.disabled ? 'Enable' : 'Disable'}
                          >
                            {script.disabled ? <Circle size={12} /> : <CircleCheck size={12} />}
                          </button>
                          <button
                            onClick={e => { e.stopPropagation(); handleDeleteScript(script.id); }}
                            className={`p-0.5 text-muted hover:text-[var(--accent-error)] transition-colors ${
                              selectedScriptId === script.id ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
                            }`}
                            title="Delete"
                          >
                            <Trash2 size={12} />
                          </button>
                        </div>
                      </div>
                    ))
                  )}
                </div>

                {/*
                //
                // Editor panel.
                //
                */}

                <div className="flex-1 flex flex-col border border-dim min-h-0">
                  {(selectedScriptId || isAddingScript) ? (
                    <>
                      <div className="flex items-center justify-between gap-2 px-3 py-1.5 border-b border-dim bg-[var(--bg-secondary)] flex-shrink-0">
                        {isEditingScript ? (
                          (() => {
                            const script = state.luaAgentScripts.find(s => s.id === selectedScriptId);
                            return script?.is_builtin ? (
                              <span className="text-[11px] font-medium text-highlight">{editingScriptName}</span>
                            ) : (
                              <input
                                type="text"
                                value={editingScriptName}
                                onChange={e => setEditingScriptName(e.target.value)}
                                placeholder="Script name"
                                className="bg-[var(--bg-primary)] border border-dim px-2 py-0.5 text-[11px] text-highlight focus:outline-none focus:border-subtle w-48"
                              />
                            );
                          })()
                        ) : (
                          <div className="flex items-center gap-1.5">
                            <span className="text-[11px] font-medium text-highlight">{editingScriptName}</span>
                            {(() => {
                              const script = state.luaAgentScripts.find(s => s.id === selectedScriptId);
                              return (
                                <>
                                  {script?.is_builtin && (
                                    <span className="text-[7px] leading-tight px-0.5 rounded bg-[var(--accent-info)]/15 text-[var(--accent-info)]/70">builtin</span>
                                  )}
                                  {script?.disabled && (
                                    <span className="text-[7px] leading-tight px-0.5 rounded bg-[var(--accent-warning)]/15 text-[var(--accent-warning)]/70">disabled</span>
                                  )}
                                </>
                              );
                            })()}
                          </div>
                        )}
                        <div className="flex gap-1.5">
                          {isEditingScript ? (
                            <>
                              <button
                                onClick={handleSaveScript}
                                disabled={!editingScriptName.trim()}
                                className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
                              >
                                <Save size={10} /> Save
                              </button>
                              <button
                                onClick={() => {
                                  if (isAddingScript) {
                                    setIsAddingScript(false);
                                    setIsEditingScript(false);
                                    setEditingScriptName('');
                                    setEditingScriptContent('');
                                  } else {
                                    const script = state.luaAgentScripts.find(s => s.id === selectedScriptId);
                                    if (script) {
                                      setEditingScriptName(script.name);
                                      setEditingScriptContent(script.script);
                                    }
                                    setIsEditingScript(false);
                                  }
                                }}
                                className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted hover:text-highlight transition-colors"
                              >
                                <X size={10} /> Cancel
                              </button>
                            </>
                          ) : (
                            <button
                              onClick={() => {
                                const script = state.luaAgentScripts.find(s => s.id === selectedScriptId);
                                if (script?.is_builtin) {
                                  setShowBuiltinWarning(true);
                                } else {
                                  setIsEditingScript(true);
                                }
                              }}
                              className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors"
                            >
                              <Edit2 size={10} /> Edit
                            </button>
                          )}
                        </div>
                      </div>
                      <LuaCodeEditor
                        value={editingScriptContent}
                        onChange={setEditingScriptContent}
                        readOnly={!isEditingScript}
                      />
                    </>
                  ) : (
                    <div className="flex-1 flex items-center justify-center text-muted text-xs">
                      Select a script or upload a new one
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}

          {/*
          //
          // Intercept tab — manages the intercept-target list (URLs +
          // filters) the service pushes to nodes.
          //
          */}

          {activeTab === 'intercept' && (
            <div className="space-y-5">
              <div>
                <h3 className="text-xs font-semibold text-highlight tracking-wider mb-0.5">INTERCEPT TARGETS</h3>
                <p className="text-[10px] text-muted">
                  Domains and URL filters captured by the node-level proxy. Built-ins ship with default values; edit, disable, or add your own.
                </p>
              </div>

              {!isEditingTarget && (
                <div className="space-y-1">
                  {state.interceptTargets.length === 0 && (
                    <div className="text-[11px] text-muted py-3">
                      No intercept targets configured.
                    </div>
                  )}
                  {state.interceptTargets.map(target => (
                    <div
                      key={target.id}
                      className="flex items-start gap-2 px-2 py-1.5 border border-subtle hover:border-dim transition-colors"
                    >
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 text-[11px]">
                          <span className={target.disabled ? 'text-dim line-through' : 'text-highlight'}>
                            {target.name}
                          </span>
                          <span className="text-dim">agent={target.agent_short_name}</span>
                          {target.is_builtin && (
                            <span className="text-[9px] uppercase tracking-wider text-[var(--accent-info)]">builtin</span>
                          )}
                          {target.disabled && (
                            <span className="text-[9px] uppercase tracking-wider text-[var(--accent-error)]">disabled</span>
                          )}
                        </div>
                        <div className="text-[10px] text-muted mt-0.5 break-all">
                          {target.domains.join(', ')}
                          {target.url_pattern ? <span className="text-dim"> · /{target.url_pattern}/</span> : null}
                        </div>
                      </div>
                      <div className="flex items-center gap-1">
                        <button
                          onClick={() => toggleInterceptTargetDisabled(target.id, !target.disabled)}
                          className="p-1 hover:bg-[var(--bg-tertiary)] text-muted hover:text-highlight"
                          title={target.disabled ? 'Enable' : 'Disable'}
                        >
                          {target.disabled ? <Circle size={12} /> : <CircleCheck size={12} />}
                        </button>
                        <button
                          onClick={() => {
                            setIsEditingTarget(true);
                            setEditingTargetId(target.id);
                            setTargetForm({
                              name: target.name,
                              agent_short_name: target.agent_short_name,
                              domains: target.domains.join(', '),
                              url_pattern: target.url_pattern ?? '',
                            });
                            setTargetFormError(null);
                          }}
                          className="p-1 hover:bg-[var(--bg-tertiary)] text-muted hover:text-highlight"
                          title="Edit"
                        >
                          <Edit2 size={12} />
                        </button>
                        <button
                          onClick={() => setConfirmDeleteTargetId(target.id)}
                          className="p-1 hover:bg-[var(--bg-tertiary)] text-muted hover:text-[var(--accent-error)]"
                          title="Delete"
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    </div>
                  ))}
                  <button
                    onClick={() => {
                      setIsEditingTarget(true);
                      setEditingTargetId(null);
                      setTargetForm({ name: '', agent_short_name: '', domains: '', url_pattern: '' });
                      setTargetFormError(null);
                    }}
                    className={`${btnGreen} mt-2`}
                  >
                    <Plus size={12} />
                    Add intercept target
                  </button>
                </div>
              )}

              {isEditingTarget && (
                <div className="space-y-3 border border-subtle p-3">
                  <div className="text-xs font-semibold text-highlight">
                    {editingTargetId ? 'Edit intercept target' : 'Add intercept target'}
                  </div>
                  <div className="space-y-2">
                    <label className="block text-[10px] text-muted">
                      Name
                      <input
                        type="text"
                        value={targetForm.name}
                        onChange={e => setTargetForm({ ...targetForm, name: e.target.value })}
                        className={inputCls + ' mt-0.5'}
                        placeholder="e.g. My Custom Agent"
                      />
                    </label>
                    <label className="block text-[10px] text-muted">
                      Agent short name
                      <input
                        type="text"
                        value={targetForm.agent_short_name}
                        onChange={e => setTargetForm({ ...targetForm, agent_short_name: e.target.value })}
                        className={inputCls + ' mt-0.5'}
                        placeholder="e.g. claudecode"
                      />
                    </label>
                    <label className="block text-[10px] text-muted">
                      Domains (comma- or newline-separated)
                      <textarea
                        rows={2}
                        value={targetForm.domains}
                        onChange={e => setTargetForm({ ...targetForm, domains: e.target.value })}
                        className={inputCls + ' mt-0.5 font-mono'}
                        placeholder="api.example.com, api2.example.com"
                      />
                    </label>
                    <label className="block text-[10px] text-muted">
                      URL pattern (regex, optional)
                      <input
                        type="text"
                        value={targetForm.url_pattern}
                        onChange={e => setTargetForm({ ...targetForm, url_pattern: e.target.value })}
                        className={inputCls + ' mt-0.5 font-mono'}
                        placeholder="messages"
                      />
                    </label>
                  </div>
                  {targetFormError && (
                    <div className="text-[10px] text-[var(--accent-error)]">{targetFormError}</div>
                  )}
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => {
                        const name = targetForm.name.trim();
                        const agent = targetForm.agent_short_name.trim();
                        const domains = Array.from(new Set(
                          targetForm.domains.split(/[,\n]/).map(s => s.trim()).filter(Boolean)
                        ));
                        const urlPattern = targetForm.url_pattern.trim() || null;
                        if (!name) { setTargetFormError('Name is required'); return; }
                        if (!agent) { setTargetFormError('Agent short name is required'); return; }
                        if (domains.length === 0) { setTargetFormError('At least one domain is required'); return; }
                        if (editingTargetId) {
                          updateInterceptTarget(editingTargetId, name, agent, domains, urlPattern);
                        } else {
                          addInterceptTarget(name, agent, domains, urlPattern);
                        }
                        setIsEditingTarget(false);
                        setEditingTargetId(null);
                      }}
                      className={btnSave}
                    >
                      <Save size={12} />
                      Save
                    </button>
                    <button
                      onClick={() => {
                        setIsEditingTarget(false);
                        setEditingTargetId(null);
                        setTargetFormError(null);
                      }}
                      className="px-2.5 py-1 text-xs text-muted hover:text-highlight transition-colors"
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              )}

              {confirmDeleteTargetId && (
                <Modal
                  isOpen={true}
                  onClose={() => setConfirmDeleteTargetId(null)}
                  title="Delete intercept target"
                  size="sm"
                >
                  <div className="space-y-3">
                    <p className="text-xs">
                      Delete{' '}
                      <span className="text-highlight font-medium">
                        {state.interceptTargets.find(t => t.id === confirmDeleteTargetId)?.name ?? '?'}
                      </span>
                      ? This stops capturing the listed domains until you re-add the target.
                    </p>
                    <div className="flex items-center justify-end gap-2">
                      <button
                        onClick={() => setConfirmDeleteTargetId(null)}
                        className="px-2.5 py-1 text-xs text-muted hover:text-highlight"
                      >
                        Cancel
                      </button>
                      <button
                        onClick={() => {
                          if (confirmDeleteTargetId) {
                            deleteInterceptTarget(confirmDeleteTargetId);
                          }
                          setConfirmDeleteTargetId(null);
                        }}
                        className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs bg-[var(--accent-error)]/10 text-[var(--accent-error)] border border-[var(--accent-error)]/30 hover:bg-[var(--accent-error)]/20 transition-colors"
                      >
                        <Trash2 size={12} />
                        Delete
                      </button>
                    </div>
                  </div>
                </Modal>
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

              <div className="pt-4 border-t border-subtle">
                <h4 className="text-xs font-semibold text-highlight mb-1">Claude Bridge</h4>
                <p className="text-[10px] text-muted mb-2">Bridge protocols for Claude SDK connections</p>

                <div className="space-y-3">
                  <div className="space-y-2">
                    <button onClick={handleCcrV1Toggle} className="flex items-center gap-1.5 text-xs text-muted hover:text-highlight transition-colors">
                      {ccrV1Enabled
                        ? <CircleCheck size={14} className="text-[var(--accent-success)]" />
                        : <Circle size={14} className="text-[var(--text-secondary)]" />}
                      <span>CCRv1 (WebSocket) {ccrV1Enabled ? 'Enabled' : 'Disabled'}</span>
                    </button>

                    {ccrV1Enabled && (
                      <div className="flex items-center gap-2 pl-5">
                        <label className="text-[10px] text-muted">Port</label>
                        <input
                          type="number"
                          value={ccrV1Port}
                          onChange={e => setCcrV1Port(e.target.value)}
                          onBlur={handleCcrV1PortSave}
                          min="1"
                          max="65535"
                          className={`w-20 ${inputCls}`}
                        />
                        <span className="text-[10px] text-muted font-mono">ws://localhost:{ccrV1Port}</span>
                      </div>
                    )}
                  </div>

                  <div className="space-y-2">
                    <button onClick={handleCcrV2Toggle} className="flex items-center gap-1.5 text-xs text-muted hover:text-highlight transition-colors">
                      {ccrV2Enabled
                        ? <CircleCheck size={14} className="text-[var(--accent-success)]" />
                        : <Circle size={14} className="text-[var(--text-secondary)]" />}
                      <span>CCRv2 (HTTP/SSE) {ccrV2Enabled ? 'Enabled' : 'Disabled'}</span>
                    </button>

                    {ccrV2Enabled && (
                      <div className="flex items-center gap-2 pl-5">
                        <label className="text-[10px] text-muted">Port</label>
                        <input
                          type="number"
                          value={ccrV2Port}
                          onChange={e => setCcrV2Port(e.target.value)}
                          onBlur={handleCcrV2PortSave}
                          min="1"
                          max="65535"
                          className={`w-20 ${inputCls}`}
                        />
                        <span className="text-[10px] text-muted font-mono">http://localhost:{ccrV2Port}</span>
                      </div>
                    )}
                  </div>
                </div>
              </div>

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
                    value={logQueryRowLimit}
                    onChange={e => setLogQueryRowLimit(e.target.value)}
                    onBlur={() => {
                      const n = parseInt(logQueryRowLimit, 10);
                      if (n > 0) setConfig({ log_query_row_limit: logQueryRowLimit });
                    }}
                    min="1"
                    className={`w-28 ${inputCls}`}
                  />
                  <span className="text-[10px] text-muted">per table</span>
                </div>
              </div>

              <div className="pt-4 border-t border-subtle">
                <h4 className="text-xs font-semibold text-highlight mb-1">Prompt Timeout</h4>
                <p className="text-[10px] text-muted mb-2">Maximum time for LLM prompt execution</p>

                <div className="flex items-center gap-2">
                  <label className="text-[10px] text-muted">Prompt Timeout (secs)</label>
                  <input
                    type="number"
                    value={promptTimeoutSecs}
                    onChange={e => setPromptTimeoutSecs(e.target.value)}
                    onBlur={() => {
                      const n = parseInt(promptTimeoutSecs, 10);
                      if (n > 0) setConfig({ prompt_timeout_secs: promptTimeoutSecs });
                    }}
                    min="1"
                    className={`w-28 ${inputCls}`}
                  />
                </div>
              </div>

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

      {/*
      //
      // Agent script confirmation modals.
      //
      */}

      <Modal
        isOpen={showDeleteModal}
        onClose={() => { setShowDeleteModal(false); setDeletingScriptId(null); }}
        title="Delete Agent Script"
        size="sm"
      >
        <div className="space-y-4">
          <div className="flex gap-3 p-3 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/20">
            <AlertTriangle size={18} className="text-[var(--accent-error)] flex-shrink-0 mt-0.5" />
            <div className="text-xs">
              <p className="text-[var(--accent-error)] font-medium mb-1">Delete this agent script?</p>
              <p className="text-muted">This will permanently remove the script.</p>
            </div>
          </div>
          <div className="flex justify-end gap-2">
            <button onClick={() => { setShowDeleteModal(false); setDeletingScriptId(null); }} className="px-3 py-1 text-xs text-muted hover:text-highlight transition-colors">Cancel</button>
            <button onClick={handleConfirmDelete} className="px-3 py-1 text-xs bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors">Delete</button>
          </div>
        </div>
      </Modal>

      <Modal
        isOpen={showResetModal}
        onClose={() => setShowResetModal(false)}
        title="Reset Agent Scripts"
        size="sm"
      >
        <div className="space-y-4">
          <div className="flex gap-3 p-3 bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/20">
            <AlertTriangle size={18} className="text-[var(--accent-warning)] flex-shrink-0 mt-0.5" />
            <div className="text-xs">
              <p className="text-[var(--accent-warning)] font-medium mb-1">This action cannot be undone</p>
              <p className="text-muted">All custom scripts will be replaced with built-in defaults.</p>
            </div>
          </div>
          <div className="flex justify-end gap-2">
            <button onClick={() => setShowResetModal(false)} className="px-3 py-1 text-xs text-muted hover:text-highlight transition-colors">Cancel</button>
            <button onClick={handleConfirmReset} className="px-3 py-1 text-xs bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors">Reset to Defaults</button>
          </div>
        </div>
      </Modal>

      <Modal
        isOpen={showBuiltinWarning}
        onClose={() => setShowBuiltinWarning(false)}
        title="Editing Built-in Script"
        size="sm"
      >
        <div className="space-y-4">
          <div className="flex gap-3 p-3 bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/20">
            <AlertTriangle size={18} className="text-[var(--accent-warning)] flex-shrink-0 mt-0.5" />
            <div className="text-xs">
              <p className="text-[var(--accent-warning)] font-medium mb-1">This is a built-in script</p>
              <p className="text-muted">Changes may be overwritten on update. Consider creating a new script instead.</p>
            </div>
          </div>
          <div className="flex justify-end gap-2">
            <button onClick={() => setShowBuiltinWarning(false)} className="px-3 py-1 text-xs text-muted hover:text-highlight transition-colors">Cancel</button>
            <button onClick={() => { setShowBuiltinWarning(false); setIsEditingScript(true); }} className="px-3 py-1 text-xs bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors">Edit Anyway</button>
          </div>
        </div>
      </Modal>
    </Modal>
  );
}
