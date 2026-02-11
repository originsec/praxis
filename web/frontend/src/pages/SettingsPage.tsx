import { useState, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Server, Save, Check, List, Loader2, X, Cpu, Plus, Trash2, Edit2, Key, Info, ExternalLink, Download, Monitor, ToggleLeft, ToggleRight, FileCode, Upload, RotateCcw, AlertTriangle } from 'lucide-react';
import { useApp } from '../context/AppContext';
import { getFeatureFlags } from '../utils/featureFlags';
import { Modal } from '../components/common/Modal';
import { LuaCodeEditor } from '../components/common/LuaCodeEditor';

type Tab = 'llm_providers' | 'agents' | 'service' | 'about';
type LLMTab = 'model_definitions' | 'feature_selection';

//
// Provider info from API.
//
interface ProviderOption {
  value: string;
  label: string;
}

//
// Model definition stored in config.
//
interface ModelDefinition {
  //
  // provider::model format.
  //
  name: string;
  provider: string;
  model: string;
  apiKey: string;
}

//
// Feature assignments.
//
interface FeatureAssignments {
  orchestrator: string | null;
  semanticOps: string | null;
  semanticParser: string | null;
  trafficParser: string | null;
}

//
// Feature-specific settings.
//
interface FeatureSettings {
  orchestratorMaxTokens: string;
}

//
// Node download info from API.
//
interface NodeDownloadInfo {
  platform: string;
  filename: string;
  available: boolean;
  size: number | null;
}

export function SettingsPage() {
  const { state, getConfig, setConfig, listLuaAgentScripts, addLuaAgentScript, updateLuaAgentScript, deleteLuaAgentScript, resetLuaAgentScriptDefaults, toggleLuaAgentScriptDisabled } = useApp();
  const [searchParams, setSearchParams] = useSearchParams();

  //
  // Tab from URL or default.
  //
  const tabParam = searchParams.get('tab');
  const activeTab: Tab = tabParam === 'agents' || tabParam === 'service' || tabParam === 'about' ? tabParam : 'llm_providers';
  const setActiveTab = (tab: Tab) => {
    const newParams: Record<string, string> = { tab };
    if (tab === 'llm_providers') {
      const sub = searchParams.get('sub');
      if (sub) newParams.sub = sub;
    }
    setSearchParams(newParams, { replace: true });
  };

  //
  // LLM sub-tab from URL or default.
  //
  const subParam = searchParams.get('sub');
  const activeLLMTab: LLMTab = subParam === 'feature_selection' ? subParam : 'model_definitions';
  const setActiveLLMTab = (sub: LLMTab) => {
    setSearchParams({ tab: activeTab, sub }, { replace: true });
  };

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

  //
  // Feature assignments state.
  //
  const [featureAssignments, setFeatureAssignments] = useState<FeatureAssignments>({
    orchestrator: null,
    semanticOps: null,
    semanticParser: null,
    trafficParser: null,
  });

  //
  // Feature-specific settings.
  //
  const [featureSettings, setFeatureSettings] = useState<FeatureSettings>({
    orchestratorMaxTokens: '25000',
  });

  //
  // Save states.
  //
  const [isSavingModels, setIsSavingModels] = useState(false);
  const [showModelsSaved, setShowModelsSaved] = useState(false);
  const [isSavingFeatures, setIsSavingFeatures] = useState(false);
  const [showFeaturesSaved, setShowFeaturesSaved] = useState(false);

  //
  // Model chooser state.
  //
  const [showModelChooser, setShowModelChooser] = useState(false);
  const [modelChooserTarget, setModelChooserTarget] = useState<'new' | 'edit' | null>(null);
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [isLoadingModels, setIsLoadingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);

  //
  // Node downloads state.
  //
  const [nodeDownloads, setNodeDownloads] = useState<NodeDownloadInfo[]>([]);
  const [isLoadingDownloads, setIsLoadingDownloads] = useState(false);

  //
  // Provider options fetched from API.
  //
  const [providers, setProviders] = useState<ProviderOption[]>([]);

  //
  // Event logging toggle.
  //
  const [eventLoggingEnabled, setEventLoggingEnabled] = useState(false);

  //
  // MCP Server settings.
  //
  const [mcpServerEnabled, setMcpServerEnabled] = useState(false);
  const [mcpServerPort, setMcpServerPort] = useState('8585');

  //
  // Agent script editor state.
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
  // Load config on mount
  // All llm_* keys go to Service (not starting with orchestrator_).
  //
  useEffect(() => {
    getConfig([
      'llm_model_definitions',
      'llm_feature_orchestrator',
      'llm_feature_semantic_ops',
      'llm_feature_semantic_parser',
      'llm_feature_traffic_parser',
      'llm_orchestrator_max_tokens',
      'application_logs_enabled',
      'mcp_server_enabled',
      'mcp_server_port',
    ]);
  }, [getConfig]);

  //
  // Fetch providers from API on mount.
  //
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
  // Load agent scripts when agents tab is active.
  //
  useEffect(() => {
    if (activeTab === 'agents' && state.connected) {
      listLuaAgentScripts();
    }
  }, [activeTab, state.connected, listLuaAgentScripts]);

  //
  // Fetch downloads info when Service tab is active.
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
  // Update from config.
  //
  useEffect(() => {
    const cfg = state.config;

    //
    // Parse model definitions.
    //
    if (cfg.llm_model_definitions) {
      try {
        const defs = JSON.parse(cfg.llm_model_definitions);
        if (Array.isArray(defs)) {
          setModelDefinitions(defs);
        }
      } catch (e) {
        console.error('Failed to parse model definitions:', e);
      }
    }

    //
    // Load feature assignments (all stored on Service via llm_* keys).
    //
    setFeatureAssignments({
      orchestrator: cfg.llm_feature_orchestrator || null,
      semanticOps: cfg.llm_feature_semantic_ops || null,
      semanticParser: cfg.llm_feature_semantic_parser || null,
      trafficParser: cfg.llm_feature_traffic_parser || null,
    });

    //
    // Load feature settings (all stored on Service via llm_* keys).
    //
    setFeatureSettings({
      orchestratorMaxTokens: cfg.llm_orchestrator_max_tokens || '25000',
    });

    //
    // Load event logging setting.
    //
    if (cfg.application_logs_enabled) {
      const normalized = cfg.application_logs_enabled.toLowerCase();
      const enabled = !(normalized === 'false' || normalized === '0' || normalized === 'no');
      setEventLoggingEnabled(enabled);
    } else {
      setEventLoggingEnabled(false);
    }

    //
    // Load MCP server settings.
    //
    if (cfg.mcp_server_enabled) {
      const normalized = cfg.mcp_server_enabled.toLowerCase();
      const enabled = !(normalized === 'false' || normalized === '0' || normalized === 'no');
      setMcpServerEnabled(enabled);
    } else {
      setMcpServerEnabled(false);
    }
    setMcpServerPort(cfg.mcp_server_port || '8585');
  }, [state.config]);

  //
  // Generate model definition name.
  //
  const generateModelName = (provider: string, model: string): string => {
    return `${provider}::${model}`;
  };

  //
  // Toggle centralized event logging.
  //
  const handleEventLoggingToggle = () => {
    const next = !eventLoggingEnabled;
    setEventLoggingEnabled(next);
    setConfig({ application_logs_enabled: next ? 'true' : 'false' });
  };

  //
  // Toggle MCP server.
  //
  const handleMcpServerToggle = () => {
    const next = !mcpServerEnabled;
    setMcpServerEnabled(next);
    setConfig({ mcp_server_enabled: next ? 'true' : 'false' });
  };

  //
  // Update MCP server port.
  //
  const handleMcpPortChange = (value: string) => {
    setMcpServerPort(value);
  };

  //
  // Save MCP server port (on blur).
  //
  const handleMcpPortSave = () => {
    const port = parseInt(mcpServerPort, 10);
    if (port > 0 && port < 65536) {
      setConfig({ mcp_server_port: mcpServerPort });
    }
  };

  //
  // Add new model definition.
  //
  const handleAddModel = () => {
    if (!newModel.model.trim()) return;

    const name = generateModelName(newModel.provider, newModel.model);

    //
    // Check for duplicate.
    //
    if (modelDefinitions.some(m => m.name === name)) {
      alert(`A model definition with name "${name}" already exists.`);
      return;
    }

    const newDef: ModelDefinition = {
      name,
      ...newModel,
    };

    setModelDefinitions([...modelDefinitions, newDef]);
    setNewModel({ provider: 'anthropic', model: '', apiKey: '' });
    setIsAddingModel(false);
  };

  //
  // Update existing model definition.
  //
  const handleUpdateModel = () => {
    if (!editingModel) return;

    const newName = generateModelName(editingModel.provider, editingModel.model);
    const oldName = editingModel.name;

    //
    // Check for duplicate if name changed.
    //
    if (newName !== oldName && modelDefinitions.some(m => m.name === newName)) {
      alert(`A model definition with name "${newName}" already exists.`);
      return;
    }

    const updatedDefs = modelDefinitions.map(m => {
      if (m.name === oldName) {
        return { ...editingModel, name: newName };
      }
      return m;
    });

    //
    // Update feature assignments if the name changed.
    //
    if (newName !== oldName) {
      const updatedAssignments = { ...featureAssignments };
      if (updatedAssignments.orchestrator === oldName) updatedAssignments.orchestrator = newName;
      if (updatedAssignments.semanticOps === oldName) updatedAssignments.semanticOps = newName;
      if (updatedAssignments.semanticParser === oldName) updatedAssignments.semanticParser = newName;
      if (updatedAssignments.trafficParser === oldName) updatedAssignments.trafficParser = newName;
      setFeatureAssignments(updatedAssignments);
    }

    setModelDefinitions(updatedDefs);
    setEditingModel(null);
  };

  //
  // Delete model definition.
  //
  const handleDeleteModel = (name: string) => {
    if (!confirm(`Delete model definition "${name}"?`)) return;

    setModelDefinitions(modelDefinitions.filter(m => m.name !== name));

    //
    // Clear feature assignments using this model.
    //
    const updatedAssignments = { ...featureAssignments };
    if (updatedAssignments.orchestrator === name) updatedAssignments.orchestrator = null;
    if (updatedAssignments.semanticOps === name) updatedAssignments.semanticOps = null;
    if (updatedAssignments.semanticParser === name) updatedAssignments.semanticParser = null;
    if (updatedAssignments.trafficParser === name) updatedAssignments.trafficParser = null;
    setFeatureAssignments(updatedAssignments);
  };

  //
  // Save model definitions.
  //
  const handleSaveModels = () => {
    setIsSavingModels(true);
    setConfig({
      llm_model_definitions: JSON.stringify(modelDefinitions),
    });
    setTimeout(() => {
      setIsSavingModels(false);
      setShowModelsSaved(true);
      setTimeout(() => setShowModelsSaved(false), 2000);
    }, 500);
  };

  //
  // Save feature assignments and settings
  // All config (llm_*) goes to Service.
  //
  const handleSaveFeatures = () => {
    setIsSavingFeatures(true);
    setConfig({
      llm_feature_orchestrator: featureAssignments.orchestrator || '',
      llm_feature_semantic_ops: featureAssignments.semanticOps || '',
      llm_feature_semantic_parser: featureAssignments.semanticParser || '',
      llm_feature_traffic_parser: featureAssignments.trafficParser || '',
      llm_orchestrator_max_tokens: featureSettings.orchestratorMaxTokens,
    });
    setTimeout(() => {
      setIsSavingFeatures(false);
      setShowFeaturesSaved(true);
      //
      // Re-fetch config to ensure banner and other components see the update.
      //
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

  //
  // Fetch available models from provider.
  //
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
        //
        // Try to get error message from response body.
        //
        const text = await response.text();
        let errorMessage = `HTTP ${response.status}`;
        try {
          const errorData = JSON.parse(text);
          errorMessage = errorData.error || errorMessage;
        } catch {
          if (text) errorMessage = text;
        }
        throw new Error(errorMessage);
      }

      const data = await response.json();
      setAvailableModels(data.models || []);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error';
      setModelError(message);
    } finally {
      setIsLoadingModels(false);
    }
  };

  //
  // Handle model selection from chooser.
  //
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
  // Agent script helpers.
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

  const handleResetDefaults = () => {
    setShowResetModal(true);
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
    { id: 'llm_providers', label: 'LLM Providers', icon: <Cpu size={18} /> },
    { id: 'agents', label: 'Agents', icon: <FileCode size={18} /> },
    { id: 'service', label: 'Service', icon: <Server size={18} /> },
    { id: 'about', label: 'About', icon: <Info size={18} /> },
  ];

  return (
    <div className="space-y-6">
      {/*
      //
      // Page header.
      //
      */}
      <div>
        <h1 className="text-2xl font-bold text-highlight">Settings</h1>
        <p className="text-muted mt-1">Configure your Praxis instance</p>
      </div>

      <div className="flex flex-col md:flex-row gap-4 md:gap-6">
        {/*
        //
        // Sidebar tabs.
        //
        */}
        <div className="w-full md:w-52 flex md:block gap-2 md:gap-0 md:space-y-1 overflow-x-auto">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              style={{ cursor: 'pointer' }}
              className={`w-full md:w-full min-w-44 md:min-w-0 flex items-center gap-3 px-4 py-3 text-left transition-colors ${
                activeTab === tab.id
                  ? 'bg-[var(--highlight)] text-title border-l-2 border-[var(--border-active)]'
                  : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
              }`}
            >
              {tab.icon}
              <span className="text-sm font-medium">{tab.label}</span>
            </button>
          ))}
        </div>

        {/*
        //
        // Content.
        //
        */}
        <div className="flex-1 bg-card ascii-box border border-subtle p-4 md:p-6">
          {activeTab === 'llm_providers' && (
            <div className="space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-highlight mb-1">LLM Providers</h2>
                <p className="text-sm text-muted">Configure AI model credentials and assign them to features. Model definitions are saved to the Service.</p>
              </div>

              {/*
              //
              // LLM Subtabs.
              //
              */}
              <div className="flex gap-2 border-b border-subtle overflow-x-auto">
                {[
                  { id: 'model_definitions' as LLMTab, label: 'Model Definitions' },
                  { id: 'feature_selection' as LLMTab, label: 'Feature Configuration' },
                ].map((tab) => (
                  <button
                    key={tab.id}
                    onClick={() => setActiveLLMTab(tab.id)}
                    className={`px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px ${
                      activeLLMTab === tab.id
                        ? 'text-title border-[var(--accent-info)]'
                        : 'text-muted hover:text-[var(--text-primary)] border-transparent'
                    }`}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>

              {/*
              //
              // Model Definitions Tab.
              //
              */}
              {activeLLMTab === 'model_definitions' && (
                <div className="space-y-4">
                  <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
                    <p className="text-sm text-muted">
                      Define model credentials that can be assigned to different features.
                    </p>
                    <button
                      onClick={() => setIsAddingModel(true)}
                      className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--accent-success)]/20 text-[var(--accent-success)] rounded hover:bg-[var(--accent-success)]/30 transition-colors"
                    >
                      <Plus size={14} />
                      Add Model
                    </button>
                  </div>

                  {/*
                  //
                  // Add new model form.
                  //
                  */}
                  {isAddingModel && (
                    <div className="p-4 bg-[var(--bg-secondary)] border border-dim space-y-4">
                      <div className="flex items-center justify-between">
                        <h4 className="font-semibold text-highlight">New Model Definition</h4>
                        <button
                          onClick={() => setIsAddingModel(false)}
                          className="p-1 hover:bg-[var(--bg-tertiary)] rounded"
                        >
                          <X size={16} />
                        </button>
                      </div>

                      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                        <div>
                          <label className="block text-xs tracking-wider text-muted mb-1.5">Provider</label>
                          <select
                            value={newModel.provider}
                            onChange={(e) => setNewModel(m => ({ ...m, provider: e.target.value }))}
                            className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                          >
                            {providers.map((p) => (
                              <option key={p.value} value={p.value}>{p.label}</option>
                            ))}
                          </select>
                        </div>

                        <div>
                          <label className="block text-xs tracking-wider text-muted mb-1.5">API Key</label>
                          <input
                            type="text"
                            value={newModel.apiKey}
                            onChange={(e) => setNewModel(m => ({ ...m, apiKey: e.target.value }))}
                            placeholder="sk-..."
                            className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                          />
                        </div>

                        <div className="col-span-2">
                          <label className="block text-xs tracking-wider text-muted mb-1.5">Model</label>
                          <div className="flex gap-2">
                            <input
                              type="text"
                              value={newModel.model}
                              onChange={(e) => setNewModel(m => ({ ...m, model: e.target.value }))}
                              placeholder="e.g., claude-sonnet-4-20250514"
                              className="flex-1 bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                            />
                            <button
                              onClick={() => {
                                setModelChooserTarget('new');
                                fetchModels(newModel.provider, newModel.apiKey);
                              }}
                              disabled={!newModel.apiKey}
                              title={newModel.apiKey ? "Choose from available models" : "Enter API key first"}
                              className="px-2 py-2 bg-[var(--bg-primary)] border border-subtle rounded hover:bg-[var(--bg-tertiary)] transition-colors disabled:opacity-50"
                            >
                              <List size={16} />
                            </button>
                          </div>
                        </div>
                      </div>

                      {newModel.model && (
                        <p className="text-xs text-muted">
                          Definition name: <span className="font-mono text-highlight">{generateModelName(newModel.provider, newModel.model)}</span>
                        </p>
                      )}

                      <div className="flex gap-2">
                        <button
                          onClick={handleAddModel}
                          disabled={!newModel.model.trim()}
                          className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors disabled:opacity-50"
                        >
                          <Plus size={14} />
                          Add
                        </button>
                        <button
                          onClick={() => setIsAddingModel(false)}
                          className="px-3 py-1.5 text-sm text-muted hover:text-title transition-colors"
                        >
                          Cancel
                        </button>
                      </div>
                    </div>
                  )}

                  {/*
                  //
                  // Model definitions list.
                  //
                  */}
                  {modelDefinitions.length === 0 && !isAddingModel ? (
                    <div className="p-8 text-center text-muted border border-dashed border-subtle rounded">
                      <Key size={32} className="mx-auto mb-2 opacity-50" />
                      <p>No model definitions yet.</p>
                      <p className="text-xs mt-1">Add a model definition to get started.</p>
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {modelDefinitions.map((model) => (
                        <div
                          key={model.name}
                          className="p-4 bg-[var(--bg-secondary)] border border-dim"
                        >
                          {editingModel?.name === model.name ? (
                            //
                            // Editing mode.
                            //
                            <div className="space-y-4">
                              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                <div>
                                  <label className="block text-xs tracking-wider text-muted mb-1.5">Provider</label>
                                  <select
                                    value={editingModel.provider}
                                    onChange={(e) => setEditingModel({ ...editingModel, provider: e.target.value })}
                                    className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                                  >
                                    {providers.map((p) => (
                                      <option key={p.value} value={p.value}>{p.label}</option>
                                    ))}
                                  </select>
                                </div>

                                <div>
                                  <label className="block text-xs tracking-wider text-muted mb-1.5">API Key</label>
                                  <input
                                    type="text"
                                    value={editingModel.apiKey}
                                    onChange={(e) => setEditingModel({ ...editingModel, apiKey: e.target.value })}
                                    placeholder="sk-..."
                                    className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                                  />
                                </div>

                                <div className="col-span-2">
                                  <label className="block text-xs tracking-wider text-muted mb-1.5">Model</label>
                                  <div className="flex gap-2">
                                    <input
                                      type="text"
                                      value={editingModel.model}
                                      onChange={(e) => setEditingModel({ ...editingModel, model: e.target.value })}
                                      className="flex-1 bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                                    />
                                    <button
                                      onClick={() => {
                                        setModelChooserTarget('edit');
                                        fetchModels(editingModel.provider, editingModel.apiKey);
                                      }}
                                      disabled={!editingModel.apiKey}
                                      className="px-2 py-2 bg-[var(--bg-primary)] border border-subtle rounded hover:bg-[var(--bg-tertiary)] transition-colors disabled:opacity-50"
                                    >
                                      <List size={16} />
                                    </button>
                                  </div>
                                </div>
                              </div>

                              <div className="flex gap-2">
                                <button
                                  onClick={handleUpdateModel}
                                  className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors"
                                >
                                  <Check size={14} />
                                  Update
                                </button>
                                <button
                                  onClick={() => setEditingModel(null)}
                                  className="px-3 py-1.5 text-sm text-muted hover:text-title transition-colors"
                                >
                                  Cancel
                                </button>
                              </div>
                            </div>
                          ) : (
                            //
                            // Display mode.
                            //
                            <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
                              <div>
                                <p className="font-mono text-sm text-highlight">{model.name}</p>
                                <p className="text-xs text-muted mt-1">
                                  {providers.find(p => p.value === model.provider)?.label || model.provider}
                                </p>
                              </div>
                              <div className="flex gap-2">
                                <button
                                  onClick={() => setEditingModel(model)}
                                  className="p-2 text-muted hover:text-title hover:bg-[var(--bg-tertiary)] rounded transition-colors"
                                  title="Edit"
                                >
                                  <Edit2 size={16} />
                                </button>
                                <button
                                  onClick={() => handleDeleteModel(model.name)}
                                  className="p-2 text-muted hover:text-[var(--accent-error)] hover:bg-[var(--accent-error)]/10 rounded transition-colors"
                                  title="Delete"
                                >
                                  <Trash2 size={16} />
                                </button>
                              </div>
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  )}

                  {/*
                  //
                  // Save button.
                  //
                  */}
                  {modelDefinitions.length > 0 && (
                    <button
                      onClick={handleSaveModels}
                      disabled={isSavingModels}
                      className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors disabled:opacity-50"
                    >
                      {showModelsSaved ? (
                        <>
                          <Check size={14} />
                          Saved
                        </>
                      ) : (
                        <>
                          <Save size={14} />
                          {isSavingModels ? 'Saving...' : 'Save Model Definitions'}
                        </>
                      )}
                    </button>
                  )}
                </div>
              )}

              {/*
              //
              // Feature Configuration Tab.
              //
              */}
              {activeLLMTab === 'feature_selection' && (
                <div className="space-y-4">
                  {modelDefinitions.length === 0 ? (
                    <div className="p-8 text-center text-muted border border-dashed border-subtle rounded">
                      <Key size={32} className="mx-auto mb-2 opacity-50" />
                      <p>No model definitions available.</p>
                      <p className="text-xs mt-1">
                        <button
                          onClick={() => setActiveLLMTab('model_definitions')}
                          className="text-[var(--accent-info)] hover:underline"
                        >
                          Add model definitions
                        </button>
                        {' '}to assign them to features.
                      </p>
                    </div>
                  ) : (
                    <div className="space-y-3">
                      {/*
                      //
                      // Orchestrator - enabled via devtools flag.
                      //
                      */}
                      {getFeatureFlags().orchestrator && (
                      <div className="flex flex-col md:flex-row md:items-center gap-3 md:gap-4 p-3 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-full md:w-48">
                          <p className="text-sm font-medium text-highlight">Orchestrator</p>
                          <p className="text-xs text-muted">Interactive AI assistant</p>
                        </div>
                        <select
                          value={featureAssignments.orchestrator || ''}
                          onChange={(e) => setFeatureAssignments(a => ({ ...a, orchestrator: e.target.value || null }))}
                          className="flex-1 bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                        >
                          <option value="">Select a model...</option>
                          {modelDefinitions.map((m) => (
                            <option key={m.name} value={m.name}>{m.name}</option>
                          ))}
                        </select>
                        <input
                          type="number"
                          value={featureSettings.orchestratorMaxTokens}
                          onChange={(e) => setFeatureSettings(s => ({ ...s, orchestratorMaxTokens: e.target.value }))}
                          placeholder="Max tokens"
                          min="1000"
                          max="100000"
                          className="w-full md:w-28 bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                          title="Max tokens"
                        />
                      </div>
                      )}

                      {/*
                      //
                      // Semantic Operations.
                      //
                      */}
                      <div className="flex flex-col md:flex-row md:items-center gap-3 md:gap-4 p-3 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-full md:w-48">
                          <p className="text-sm font-medium text-highlight">Semantic Operations</p>
                          <p className="text-xs text-muted">Default model for ops</p>
                        </div>
                        <select
                          value={featureAssignments.semanticOps || ''}
                          onChange={(e) => setFeatureAssignments(a => ({ ...a, semanticOps: e.target.value || null }))}
                          className="flex-1 bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                        >
                          <option value="">Select a model...</option>
                          {modelDefinitions.map((m) => (
                            <option key={m.name} value={m.name}>{m.name}</option>
                          ))}
                        </select>
                      </div>

                      {/*
                      //
                      // Semantic Parser.
                      //
                      */}
                      <div className="flex flex-col md:flex-row md:items-center gap-3 md:gap-4 p-3 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-full md:w-48">
                          <p className="text-sm font-medium text-highlight">Semantic Parser</p>
                          <p className="text-xs text-muted">Tool call parsing</p>
                        </div>
                        <select
                          value={featureAssignments.semanticParser || ''}
                          onChange={(e) => setFeatureAssignments(a => ({ ...a, semanticParser: e.target.value || null }))}
                          className="flex-1 bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                        >
                          <option value="">Select a model...</option>
                          {modelDefinitions.map((m) => (
                            <option key={m.name} value={m.name}>{m.name}</option>
                          ))}
                        </select>
                      </div>

                      {/*
                      //
                      // Traffic Parser.
                      //
                      */}
                      <div className="flex flex-col md:flex-row md:items-center gap-3 md:gap-4 p-3 bg-[var(--bg-secondary)] border border-dim">
                        <div className="w-full md:w-48">
                          <p className="text-sm font-medium text-highlight">Traffic Parser</p>
                          <p className="text-xs text-muted">Traffic summarization</p>
                        </div>
                        <select
                          value={featureAssignments.trafficParser || ''}
                          onChange={(e) => setFeatureAssignments(a => ({ ...a, trafficParser: e.target.value || null }))}
                          className="flex-1 bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                        >
                          <option value="">Select a model...</option>
                          {modelDefinitions.map((m) => (
                            <option key={m.name} value={m.name}>{m.name}</option>
                          ))}
                        </select>
                      </div>
                    </div>
                  )}

                  {/*
                  //
                  // Save button.
                  //
                  */}
                  {modelDefinitions.length > 0 && (
                    <div className="flex justify-end">
                      <button
                        onClick={handleSaveFeatures}
                        disabled={isSavingFeatures}
                        className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors disabled:opacity-50"
                      >
                        {showFeaturesSaved ? (
                          <>
                            <Check size={14} />
                            Saved
                          </>
                        ) : (
                          <>
                            <Save size={14} />
                            {isSavingFeatures ? 'Saving...' : 'Save Feature Settings'}
                          </>
                        )}
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}

          {activeTab === 'agents' && (
            <div className="flex flex-col md:h-[calc(100vh-16rem)]">
              <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-3 mb-4">
                <div>
                  <h2 className="text-lg font-semibold text-highlight mb-1">Agent Definitions</h2>
                  <p className="text-sm text-muted">Manage Lua agent connector scripts stored in the service database</p>
                </div>
                <div className="flex flex-wrap gap-2">
                  <label className="inline-flex items-center gap-2 px-3 py-1.5 text-sm rounded-md bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors cursor-pointer">
                    <Upload size={14} />
                    Upload
                    <input type="file" accept=".lua" onChange={handleFileUpload} className="hidden" />
                  </label>
                  <button
                    onClick={handleResetDefaults}
                    className="inline-flex items-center gap-2 px-3 py-1.5 text-sm rounded-md bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors"
                    title="Reset all scripts to built-in defaults"
                  >
                    <RotateCcw size={14} />
                    Reset Defaults
                  </button>
                </div>
              </div>

              <div className="flex flex-col lg:flex-row gap-4 flex-1 min-h-0">
                {/*
                //
                // Script list.
                //
                */}
                <div className="w-full lg:w-56 flex-shrink-0 border border-dim overflow-y-auto max-h-56 lg:max-h-none">
                  {state.luaAgentScripts.length === 0 ? (
                    <div className="p-4 text-center text-muted text-sm">
                      No scripts
                    </div>
                  ) : (
                    state.luaAgentScripts.map(script => (
                      <div
                        key={script.id}
                        onClick={() => handleSelectScript(script.id)}
                        className={`group flex items-center justify-between px-3 py-2 cursor-pointer transition-colors ${
                          selectedScriptId === script.id
                            ? 'bg-[var(--highlight)] text-title'
                            : script.disabled
                              ? 'hover:bg-[var(--bg-tertiary)] text-muted opacity-50'
                              : 'hover:bg-[var(--bg-tertiary)] text-muted'
                        }`}
                      >
                        <div className="flex items-center gap-1.5 min-w-0">
                          <span className="text-sm truncate">{script.name}</span>
                          {script.is_builtin && (
                            <span className="text-[8px] leading-tight px-1 rounded bg-[var(--accent-info)]/15 text-[var(--accent-info)]/70 flex-shrink-0">
                              builtin
                            </span>
                          )}
                        </div>
                        <div className="flex items-center gap-0.5 flex-shrink-0">
                          <button
                            onClick={(e) => { e.stopPropagation(); toggleLuaAgentScriptDisabled(script.id, !script.disabled); }}
                            className={`p-1 transition-colors ${
                              script.disabled
                                ? 'text-[var(--accent-warning)]'
                                : 'text-muted hover:text-[var(--accent-success)] opacity-0 group-hover:opacity-100'
                            }`}
                            title={script.disabled ? 'Enable' : 'Disable'}
                          >
                            {script.disabled ? <ToggleLeft size={14} /> : <ToggleRight size={14} />}
                          </button>
                          <button
                            onClick={(e) => { e.stopPropagation(); handleDeleteScript(script.id); }}
                            className={`p-1 text-muted hover:text-[var(--accent-error)] transition-colors ${
                              selectedScriptId === script.id ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
                            }`}
                            title="Delete"
                          >
                            <Trash2 size={14} />
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
                      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 px-4 py-2 border-b border-dim bg-[var(--bg-secondary)] flex-shrink-0">
                        {isEditingScript ? (
                          (() => {
                            const script = state.luaAgentScripts.find(s => s.id === selectedScriptId);
                            return script?.is_builtin ? (
                              <span className="text-sm font-medium text-highlight">{editingScriptName}</span>
                            ) : (
                              <input
                                type="text"
                                value={editingScriptName}
                                onChange={(e) => setEditingScriptName(e.target.value)}
                                placeholder="Script name"
                                className="bg-[var(--bg-primary)] border border-dim rounded px-2 py-1 text-sm text-highlight focus:outline-none focus:border-subtle w-full md:w-64"
                              />
                            );
                          })()
                        ) : (
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-medium text-highlight">{editingScriptName}</span>
                            {(() => {
                              const script = state.luaAgentScripts.find(s => s.id === selectedScriptId);
                              return (
                                <>
                                  {script?.is_builtin && (
                                    <span className="text-[8px] leading-tight px-1 rounded bg-[var(--accent-info)]/15 text-[var(--accent-info)]/70">builtin</span>
                                  )}
                                  {script?.disabled && (
                                    <span className="text-[8px] leading-tight px-1 rounded bg-[var(--accent-warning)]/15 text-[var(--accent-warning)]/70">disabled</span>
                                  )}
                                </>
                              );
                            })()}
                          </div>
                        )}
                        <div className="flex gap-2">
                          {isEditingScript ? (
                            <>
                              <button
                                onClick={handleSaveScript}
                                disabled={!editingScriptName.trim()}
                                className="inline-flex items-center gap-1 px-2 py-1 text-xs rounded bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors disabled:opacity-50"
                              >
                                <Save size={12} />
                                Save
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
                                className="inline-flex items-center gap-1 px-2 py-1 text-xs text-muted hover:text-title transition-colors"
                              >
                                <X size={12} />
                                Cancel
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
                              className="inline-flex items-center gap-1 px-2 py-1 text-xs rounded bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors"
                            >
                              <Edit2 size={12} />
                              Edit
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
                    <div className="flex-1 flex items-center justify-center text-muted text-sm">
                      Select a script or create a new one
                    </div>
                  )}
                </div>
              </div>

              {/*
              //
              // Reset defaults confirmation modal.
              //
              */}
              {/*
              //
              // Delete script confirmation modal.
              //
              */}
              <Modal
                isOpen={showDeleteModal}
                onClose={() => { setShowDeleteModal(false); setDeletingScriptId(null); }}
                title="Delete Agent Script"
                size="sm"
              >
                <div className="space-y-4">
                  <div className="flex gap-3 p-3 rounded bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/20">
                    <AlertTriangle size={20} className="text-[var(--accent-error)] flex-shrink-0 mt-0.5" />
                    <div className="text-sm">
                      <p className="text-[var(--accent-error)] font-medium mb-1">Delete this agent script?</p>
                      <p className="text-muted">This will permanently remove the script. This action cannot be undone.</p>
                    </div>
                  </div>
                  <div className="flex justify-end gap-2">
                    <button
                      onClick={() => { setShowDeleteModal(false); setDeletingScriptId(null); }}
                      className="px-3 py-1.5 text-sm rounded-md text-muted hover:text-title transition-colors"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={handleConfirmDelete}
                      className="px-3 py-1.5 text-sm rounded-md bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
                    >
                      Delete
                    </button>
                  </div>
                </div>
              </Modal>

              {/*
              //
              // Reset defaults confirmation modal.
              //
              */}
              <Modal
                isOpen={showResetModal}
                onClose={() => setShowResetModal(false)}
                title="Reset Agent Scripts"
                size="sm"
              >
                <div className="space-y-4">
                  <div className="flex gap-3 p-3 rounded bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/20">
                    <AlertTriangle size={20} className="text-[var(--accent-warning)] flex-shrink-0 mt-0.5" />
                    <div className="text-sm">
                      <p className="text-[var(--accent-warning)] font-medium mb-1">This action cannot be undone</p>
                      <p className="text-muted">All custom and modified agent scripts will be permanently deleted and replaced with the built-in defaults.</p>
                    </div>
                  </div>
                  <div className="flex justify-end gap-2">
                    <button
                      onClick={() => setShowResetModal(false)}
                      className="px-3 py-1.5 text-sm rounded-md text-muted hover:text-title transition-colors"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={handleConfirmReset}
                      className="px-3 py-1.5 text-sm rounded-md bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors"
                    >
                      Reset to Defaults
                    </button>
                  </div>
                </div>
              </Modal>

              {/*
              //
              // Builtin script edit warning modal.
              //
              */}
              <Modal
                isOpen={showBuiltinWarning}
                onClose={() => setShowBuiltinWarning(false)}
                title="Editing Built-in Script"
                size="sm"
              >
                <div className="space-y-4">
                  <div className="flex gap-3 p-3 rounded bg-[var(--accent-warning)]/10 border border-[var(--accent-warning)]/20">
                    <AlertTriangle size={20} className="text-[var(--accent-warning)] flex-shrink-0 mt-0.5" />
                    <div className="text-sm">
                      <p className="text-[var(--accent-warning)] font-medium mb-1">This is a built-in script</p>
                      <p className="text-muted">Changes to built-in scripts may be overwritten when Praxis is updated. Consider creating a new script with your changes and disabling this one instead.</p>
                    </div>
                  </div>
                  <div className="flex justify-end gap-2">
                    <button
                      onClick={() => setShowBuiltinWarning(false)}
                      className="px-3 py-1.5 text-sm rounded-md text-muted hover:text-title transition-colors"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={() => { setShowBuiltinWarning(false); setIsEditingScript(true); }}
                      className="px-3 py-1.5 text-sm rounded-md bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30 transition-colors"
                    >
                      Edit Anyway
                    </button>
                  </div>
                </div>
              </Modal>
            </div>
          )}

          {activeTab === 'service' && (
            <div className="space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-highlight mb-1">Service Configuration</h2>
                <p className="text-sm text-muted">Connection and service settings</p>
              </div>

              <div className="space-y-4 max-w-md">
                <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
                  <span className="text-muted">Status</span>
                  <span className={state.connected ? 'status-online' : 'status-offline'}>
                    {state.connected ? 'Connected' : 'Disconnected'}
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
                </div>
              </div>

              {/*
              //
              // Event Logging.
              //
              */}
              <div className="pt-4 border-t border-subtle">
                <div className="mb-2">
                  <h3 className="text-md font-semibold text-highlight mb-1">Event Logging</h3>
                  <p className="text-sm text-muted">Centralized application logs from service, nodes, and web</p>
                </div>
                <button
                  type="button"
                  onClick={handleEventLoggingToggle}
                  className="flex items-center gap-2 text-sm text-muted hover:text-highlight transition-colors"
                >
                  {eventLoggingEnabled ? (
                    <ToggleRight size={20} className="text-muted" />
                  ) : (
                    <ToggleLeft size={20} className="text-muted" />
                  )}
                  <span className="tracking-wider">
                    {eventLoggingEnabled ? 'Enabled' : 'Disabled'}
                  </span>
                </button>
              </div>

              {/*
              //
              // MCP Server.
              //
              */}
              <div className="pt-4 border-t border-subtle">
                <div className="mb-2">
                  <h3 className="text-md font-semibold text-highlight mb-1">MCP Server</h3>
                  <p className="text-sm text-muted">Expose Praxis tools via Model Context Protocol (SSE transport)</p>
                </div>
                <div className="space-y-3">
                  <button
                    type="button"
                    onClick={handleMcpServerToggle}
                    className="flex items-center gap-2 text-sm text-muted hover:text-highlight transition-colors"
                  >
                    {mcpServerEnabled ? (
                      <ToggleRight size={20} className="text-muted" />
                    ) : (
                      <ToggleLeft size={20} className="text-muted" />
                    )}
                    <span className="tracking-wider">
                      {mcpServerEnabled ? 'Enabled' : 'Disabled'}
                    </span>
                  </button>
                  {mcpServerEnabled && (
                    <div className="flex items-center gap-3 pl-7">
                      <label className="text-xs text-muted">Port</label>
                      <input
                        type="number"
                        value={mcpServerPort}
                        onChange={(e) => handleMcpPortChange(e.target.value)}
                        onBlur={handleMcpPortSave}
                        min="1"
                        max="65535"
                        className="w-24 bg-[var(--bg-primary)] border border-dim px-2 py-1 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                      />
                      <span className="text-xs text-muted">SSE endpoint: http://localhost:{mcpServerPort}/sse</span>
                    </div>
                  )}
                </div>
              </div>

              {/*
              //
              // Node Downloads Section.
              //
              */}
              <div className="pt-4 border-t border-subtle">
                <div className="mb-4">
                  <h3 className="text-md font-semibold text-highlight mb-1">Node Agent Downloads</h3>
                  <p className="text-sm text-muted">Download the Praxis node agent for your target machines</p>
                </div>

                {isLoadingDownloads ? (
                  <div className="flex items-center gap-2 text-muted">
                    <Loader2 size={16} className="animate-spin" />
                    <span className="text-sm">Loading...</span>
                  </div>
                ) : (
                  <div className="space-y-2 max-w-md">
                    {nodeDownloads.map((node) => (
                      <div
                        key={node.platform}
                        className="flex items-center justify-between p-3 bg-[var(--bg-secondary)]"
                      >
                        <div className="flex items-center gap-3">
                          <Monitor size={18} className="text-muted" />
                          <div>
                            <span className="font-medium capitalize">{node.platform}</span>
                            <p className="text-xs text-muted">
                              {node.filename}
                              {node.available && node.size && (
                                <span className="ml-1">
                                  ({(node.size / 1024 / 1024).toFixed(1)} MB)
                                </span>
                              )}
                            </p>
                          </div>
                        </div>
                        {node.available ? (
                          <a
                            href={`/api/downloads/node/${node.platform}`}
                            download={node.filename}
                            className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors"
                          >
                            <Download size={14} />
                            Download
                          </a>
                        ) : (
                          <span className="text-xs text-muted italic">Not available</span>
                        )}
                      </div>
                    ))}
                    {nodeDownloads.length === 0 && (
                      <div className="p-4 text-center text-muted">
                        <p className="text-sm">No node binaries available.</p>
                        <p className="text-xs mt-1">Build with Docker or run install.sh to generate binaries.</p>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          )}

          {activeTab === 'about' && (
            <div className="space-y-2">
              <div>
                <h2 className="text-lg font-semibold text-highlight mb-1">About</h2>
              </div>

              <div className="max-w-2xl">
                <div className="p-4 md:p-6 pt-2">
                  <h3 className="text-md font-semibold text-[var(--accent-success)] mb-4">Praxis by [Ø] Origin</h3>
                  <p className="text-sm text-muted mb-6">
                    <a href="https://originhq.com" target="_blank" rel="noopener noreferrer" className="text-[var(--accent-info)]/70 hover:text-[var(--accent-info)] hover:underline">Origin</a> is an endpoint security company building protection for the semantic era of computing. As AI agents become integral to enterprise workflows, Origin provides the visibility and control organizations need to safely grant agents the permissions they require.
                  </p>
                  <p className="text-sm text-muted mb-8">
                    <a href="https://github.com/originsec/praxis" target="_blank" rel="noopener noreferrer" className="text-[var(--accent-info)]/70 hover:text-[var(--accent-info)] hover:underline">Praxis</a> is Origin's experimental research platform for exploring the adversarial boundaries of legitimate semantic tools. By understanding how computer-use agents and their underlying capabilities can be leveraged offensively, we build better defenses for the endpoints they operate on.
                  </p>
                      <div className="flex flex-col sm:flex-row gap-3 sm:gap-4">
                    <a
                      href="https://originhq.com"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim rounded hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors"
                    >
                      <ExternalLink size={14} />
                      originhq.com
                    </a>
                    <a
                      href="https://praxis.originhq.com"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] rounded hover:bg-[var(--accent-purple)]/30 transition-colors"
                    >
                      <ExternalLink size={14} />
                      praxis.originhq.com
                    </a>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>

      {/*
      //
      // Model Chooser Modal.
      //
      */}
      {showModelChooser && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card border border-subtle ascii-box w-full max-w-md max-h-[80vh] flex flex-col">
            <div className="flex items-center justify-between p-4 border-b border-subtle">
              <h3 className="text-lg font-semibold text-highlight">Choose Model</h3>
              <button
                onClick={() => {
                  setShowModelChooser(false);
                  setModelChooserTarget(null);
                }}
                style={{ cursor: 'pointer' }}
                className="p-1 hover:bg-[var(--bg-tertiary)] rounded"
              >
                <X size={20} />
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-4">
              {isLoadingModels && (
                <div className="flex items-center justify-center py-8">
                  <Loader2 className="animate-spin" size={24} />
                  <span className="ml-2 text-muted">Loading models...</span>
                </div>
              )}

              {modelError && (
                <div className="p-4 bg-[var(--accent-error)]/10 text-[var(--accent-error)]">
                  {modelError}
                </div>
              )}

              {!isLoadingModels && !modelError && availableModels.length === 0 && (
                <div className="text-center text-muted py-8">
                  No models available
                </div>
              )}

              {!isLoadingModels && availableModels.length > 0 && (
                <div className="space-y-1">
                  {availableModels.map((model) => (
                    <button
                      key={model}
                      onClick={() => handleModelSelect(model)}
                      style={{ cursor: 'pointer' }}
                      className="w-full text-left px-4 py-2.5 hover:bg-[var(--bg-tertiary)] transition-colors text-sm"
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
    </div>
  );
}
