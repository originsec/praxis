import { useState, useEffect, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Server, Save, Check, List, Loader2, X, Cpu, Upload, Plus, Trash2, Edit2, Key, Info, ExternalLink, Download, Monitor } from 'lucide-react';
import { useApp } from '../context/AppContext';

type Tab = 'llm_providers' | 'service' | 'about';
type LLMTab = 'model_definitions' | 'feature_selection';
type FeatureId = 'nexus' | 'semanticOps' | 'semanticParser' | 'trafficParser';

const providers = [
  { value: 'anthropic', label: 'Anthropic (Claude)' },
  { value: 'cerebras', label: 'Cerebras' },
  { value: 'gemini', label: 'Google Gemini' },
  { value: 'groq', label: 'Groq' },
  { value: 'mistral', label: 'Mistral' },
  { value: 'ollama', label: 'Ollama (Local)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'xai', label: 'xAI (Grok)' },
];

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
  nexus: string | null;
  semanticOps: string | null;
  semanticParser: string | null;
  trafficParser: string | null;
}

//
// Feature-specific settings.
//
interface FeatureSettings {
  nexusPrompt: string;
  nexusMaxTokens: string;
  semanticOpPrompt: string;
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
  const { state, getConfig, setConfig } = useApp();
  const [searchParams, setSearchParams] = useSearchParams();

  //
  // Tab from URL or default.
  //
  const tabParam = searchParams.get('tab');
  const activeTab: Tab = tabParam === 'service' || tabParam === 'about' ? tabParam : 'llm_providers';
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
    nexus: null,
    semanticOps: null,
    semanticParser: null,
    trafficParser: null,
  });

  //
  // Feature-specific settings.
  //
  const [featureSettings, setFeatureSettings] = useState<FeatureSettings>({
    nexusPrompt: '',
    nexusMaxTokens: '25000',
    semanticOpPrompt: '',
  });

  //
  // Default prompts from server.
  //
  const [defaultPrompts, setDefaultPrompts] = useState<{ nexus: string; semantic_op: string } | null>(null);

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
  // File input refs.
  //
  const nexusFileInputRef = useRef<HTMLInputElement>(null);
  const semanticOpFileInputRef = useRef<HTMLInputElement>(null);

  //
  // Selected feature in feature configuration tab.
  //
  const [selectedFeature, setSelectedFeature] = useState<FeatureId>('semanticOps');

  //
  // Node downloads state.
  //
  const [nodeDownloads, setNodeDownloads] = useState<NodeDownloadInfo[]>([]);
  const [isLoadingDownloads, setIsLoadingDownloads] = useState(false);

  //
  // Feature definitions for the list.
  //
  const features: { id: FeatureId; label: string; description: string }[] = [
    // { id: 'nexus', label: 'Nexus', description: 'Interactive AI assistant' },  // Hidden - feature not ready
    { id: 'semanticOps', label: 'Semantic Operations', description: 'Default model for ops' },
    { id: 'semanticParser', label: 'Semantic Parser', description: 'Tool call parsing' },
    { id: 'trafficParser', label: 'Traffic Parser', description: 'Traffic summarization' },
  ];

  //
  // Load config on mount
  // All llm_* keys go to Service (not starting with nexus_).
  //
  useEffect(() => {
    getConfig([
      'llm_model_definitions',
      'llm_feature_nexus',
      'llm_feature_semantic_ops',
      'llm_feature_semantic_parser',
      'llm_feature_traffic_parser',
      'llm_nexus_prompt',
      'llm_nexus_max_tokens',
      'llm_semantic_op_prompt',
    ]);

    //
    // Fetch default prompts.
    //
    fetch('/api/prompts/defaults')
      .then(res => res.json())
      .then(data => setDefaultPrompts(data))
      .catch(err => console.error('Failed to fetch default prompts:', err));
  }, [getConfig]);

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
      nexus: cfg.llm_feature_nexus || null,
      semanticOps: cfg.llm_feature_semantic_ops || null,
      semanticParser: cfg.llm_feature_semantic_parser || null,
      trafficParser: cfg.llm_feature_traffic_parser || null,
    });

    //
    // Load feature settings (all stored on Service via llm_* keys)
    // Use default prompts if no config value is set.
    //
    setFeatureSettings({
      nexusPrompt: cfg.llm_nexus_prompt || (defaultPrompts?.nexus ?? ''),
      nexusMaxTokens: cfg.llm_nexus_max_tokens || '25000',
      semanticOpPrompt: cfg.llm_semantic_op_prompt || (defaultPrompts?.semantic_op ?? ''),
    });
  }, [state.config, defaultPrompts]);

  //
  // Generate model definition name.
  //
  const generateModelName = (provider: string, model: string): string => {
    return `${provider}::${model}`;
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
      if (updatedAssignments.nexus === oldName) updatedAssignments.nexus = newName;
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
    if (updatedAssignments.nexus === name) updatedAssignments.nexus = null;
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
      llm_feature_nexus: featureAssignments.nexus || '',
      llm_feature_semantic_ops: featureAssignments.semanticOps || '',
      llm_feature_semantic_parser: featureAssignments.semanticParser || '',
      llm_feature_traffic_parser: featureAssignments.trafficParser || '',
      llm_nexus_prompt: featureSettings.nexusPrompt,
      llm_nexus_max_tokens: featureSettings.nexusMaxTokens,
      llm_semantic_op_prompt: featureSettings.semanticOpPrompt,
    });
    setTimeout(() => {
      setIsSavingFeatures(false);
      setShowFeaturesSaved(true);
      //
      // Re-fetch config to ensure banner and other components see the update.
      //
      getConfig([
        'llm_model_definitions',
        'llm_feature_nexus',
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

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'llm_providers', label: 'LLM Providers', icon: <Cpu size={18} /> },
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

      <div className="flex gap-6">
        {/*
        //
        // Sidebar tabs.
        //
        */}
        <div className="w-52 space-y-1">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              style={{ cursor: 'pointer' }}
              className={`w-full flex items-center gap-3 px-4 py-3 text-left transition-colors ${
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
        <div className="flex-1 bg-card ascii-box border border-subtle p-6">
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
              <div className="flex gap-2 border-b border-subtle">
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
                  <div className="flex items-center justify-between">
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

                      <div className="grid grid-cols-2 gap-4">
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
                              <div className="grid grid-cols-2 gap-4">
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
                            <div className="flex items-center justify-between">
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
                    <div className="flex gap-4">
                      {/*
                      //
                      // Left pane - Feature list.
                      //
                      */}
                      <div className="w-56 space-y-1">
                        {features.map((feature) => (
                          <button
                            key={feature.id}
                            onClick={() => setSelectedFeature(feature.id)}
                            className={`w-full text-left px-3 py-2.5 rounded transition-colors ${
                              selectedFeature === feature.id
                                ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-[var(--accent-info)]/30'
                                : 'hover:bg-[var(--bg-tertiary)] border border-transparent'
                            }`}
                          >
                            <p className="text-sm font-medium">{feature.label}</p>
                            <p className="text-xs text-muted">{feature.description}</p>
                          </button>
                        ))}
                      </div>

                      {/*
                      //
                      // Right pane - Feature configuration.
                      //
                      */}
                      <div className="flex-1 p-4 bg-[var(--bg-secondary)] border border-dim">
                        {/*
                        //
                        // Nexus config.
                        //
                        */}
                        {selectedFeature === 'nexus' && (
                          <div className="space-y-4">
                            <div>
                              <h4 className="font-semibold text-highlight">Nexus</h4>
                              <p className="text-xs text-muted">Interactive AI assistant for red teaming orchestration</p>
                            </div>

                            <div className="space-y-4">
                              <div className="grid grid-cols-2 gap-4">
                                <div>
                                  <label className="block text-xs tracking-wider text-muted mb-1.5">Model Definition</label>
                                  <select
                                    value={featureAssignments.nexus || ''}
                                    onChange={(e) => setFeatureAssignments(a => ({ ...a, nexus: e.target.value || null }))}
                                    className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                                  >
                                    <option value="">Select a model...</option>
                                    {modelDefinitions.map((m) => (
                                      <option key={m.name} value={m.name}>{m.name}</option>
                                    ))}
                                  </select>
                                </div>

                                <div>
                                  <label className="block text-xs tracking-wider text-muted mb-1.5">Max Tokens</label>
                                  <input
                                    type="number"
                                    value={featureSettings.nexusMaxTokens}
                                    onChange={(e) => setFeatureSettings(s => ({ ...s, nexusMaxTokens: e.target.value }))}
                                    placeholder="25000"
                                    min="1000"
                                    max="100000"
                                    className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                                  />
                                </div>
                              </div>

                              <div>
                                <div className="flex items-center justify-between mb-1">
                                  <label className="block text-xs font-medium text-muted">System Prompt</label>
                                  <div>
                                    <input
                                      type="file"
                                      ref={nexusFileInputRef}
                                      accept=".txt,.md,.prompt"
                                      onChange={(e) => {
                                        const file = e.target.files?.[0];
                                        if (file) {
                                          const reader = new FileReader();
                                          reader.onload = (event) => {
                                            setFeatureSettings(s => ({ ...s, nexusPrompt: event.target?.result as string || '' }));
                                          };
                                          reader.readAsText(file);
                                        }
                                      }}
                                      className="hidden"
                                    />
                                    <button
                                      onClick={() => nexusFileInputRef.current?.click()}
                                      className="inline-flex items-center gap-1 px-2 py-1 text-xs tracking-wider bg-[var(--bg-primary)] border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
                                    >
                                      <Upload size={12} />
                                      Load from file
                                    </button>
                                  </div>
                                </div>
                                <textarea
                                  value={featureSettings.nexusPrompt}
                                  onChange={(e) => setFeatureSettings(s => ({ ...s, nexusPrompt: e.target.value }))}
                                  placeholder="Enter the system prompt for Nexus..."
                                  rows={10}
                                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm font-mono text-highlight focus:outline-none focus:border-subtle transition-colors resize-y"
                                />
                                <p className="text-xs text-muted mt-1">
                                  {featureSettings.nexusPrompt.length} characters
                                </p>
                              </div>
                            </div>
                          </div>
                        )}

                        {/*
                        //
                        // Semantic Operations config.
                        //
                        */}
                        {selectedFeature === 'semanticOps' && (
                          <div className="space-y-4">
                            <div>
                              <h4 className="font-semibold text-highlight">Semantic Operations</h4>
                              <p className="text-xs text-muted">Default model for ops</p>
                            </div>

                            <div className="space-y-4">
                              <div>
                                <label className="block text-xs tracking-wider text-muted mb-1.5">Model Definition</label>
                                <select
                                  value={featureAssignments.semanticOps || ''}
                                  onChange={(e) => setFeatureAssignments(a => ({ ...a, semanticOps: e.target.value || null }))}
                                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                                >
                                  <option value="">Select a model...</option>
                                  {modelDefinitions.map((m) => (
                                    <option key={m.name} value={m.name}>{m.name}</option>
                                  ))}
                                </select>
                              </div>

                              <div>
                                <div className="flex items-center justify-between mb-1">
                                  <label className="block text-xs font-medium text-muted">System Prompt</label>
                                  <div>
                                    <input
                                      type="file"
                                      ref={semanticOpFileInputRef}
                                      accept=".txt,.md,.prompt"
                                      onChange={(e) => {
                                        const file = e.target.files?.[0];
                                        if (file) {
                                          const reader = new FileReader();
                                          reader.onload = (event) => {
                                            setFeatureSettings(s => ({ ...s, semanticOpPrompt: event.target?.result as string || '' }));
                                          };
                                          reader.readAsText(file);
                                        }
                                      }}
                                      className="hidden"
                                    />
                                    <button
                                      onClick={() => semanticOpFileInputRef.current?.click()}
                                      className="inline-flex items-center gap-1 px-2 py-1 text-xs tracking-wider bg-[var(--bg-primary)] border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
                                    >
                                      <Upload size={12} />
                                      Load from file
                                    </button>
                                  </div>
                                </div>
                                <textarea
                                  value={featureSettings.semanticOpPrompt}
                                  onChange={(e) => setFeatureSettings(s => ({ ...s, semanticOpPrompt: e.target.value }))}
                                  placeholder="Enter the system prompt for semantic operations..."
                                  rows={10}
                                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm font-mono text-highlight focus:outline-none focus:border-subtle transition-colors resize-y"
                                />
                                <p className="text-xs text-muted mt-1">
                                  {featureSettings.semanticOpPrompt.length} characters
                                </p>
                              </div>
                            </div>
                          </div>
                        )}

                        {/*
                        //
                        // Semantic Parser config.
                        //
                        */}
                        {selectedFeature === 'semanticParser' && (
                          <div className="space-y-4">
                            <div>
                              <h4 className="font-semibold text-highlight">Semantic Parser</h4>
                              <p className="text-xs text-muted">Natural language parsing for tool calls</p>
                            </div>

                            <div>
                              <label className="block text-xs tracking-wider text-muted mb-1.5">Model Definition</label>
                              <select
                                value={featureAssignments.semanticParser || ''}
                                onChange={(e) => setFeatureAssignments(a => ({ ...a, semanticParser: e.target.value || null }))}
                                className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
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
                        // Traffic Parser config.
                        //
                        */}
                        {selectedFeature === 'trafficParser' && (
                          <div className="space-y-4">
                            <div>
                              <h4 className="font-semibold text-highlight">Traffic Parser</h4>
                              <p className="text-xs text-muted">Model used for traffic match rules with summarization prompts</p>
                            </div>

                            <div>
                              <label className="block text-xs tracking-wider text-muted mb-1.5">Model Definition</label>
                              <select
                                value={featureAssignments.trafficParser || ''}
                                onChange={(e) => setFeatureAssignments(a => ({ ...a, trafficParser: e.target.value || null }))}
                                className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle transition-colors"
                              >
                                <option value="">Select a model...</option>
                                {modelDefinitions.map((m) => (
                                  <option key={m.name} value={m.name}>{m.name}</option>
                                ))}
                              </select>
                            </div>
                          </div>
                        )}
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

          {activeTab === 'service' && (
            <div className="space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-highlight mb-1">Service Configuration</h2>
                <p className="text-sm text-muted">Connection and service settings</p>
              </div>

              <div className="space-y-4 max-w-md">
                <div className="p-4 bg-[var(--bg-secondary)]">
                  <div className="flex items-center gap-4 mb-2">
                    <span className="text-sm font-medium w-32">Connection Status</span>
                    <span
                      className={`text-sm ${
                        state.connected ? 'status-online' : 'status-offline'
                      }`}
                    >
                      {state.connected ? 'Connected' : 'Disconnected'}
                    </span>
                  </div>
                  {state.clientId && (
                    <div className="flex items-center gap-4">
                      <span className="text-sm text-muted w-32">Client ID</span>
                      <span className="text-xs font-mono text-muted">{state.clientId}</span>
                    </div>
                  )}
                </div>

                <div>
                  <label className="block text-sm font-medium mb-2">WebSocket URL</label>
                  <input
                    type="text"
                    value={`${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`}
                    disabled
                    className="w-full bg-[var(--bg-secondary)] border border-subtle px-4 py-2.5 text-muted"
                  />
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
                              {node.size && (
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
                <div className="p-6 pt-2">
                  <h3 className="text-md font-semibold text-[var(--accent-success)] mb-4">Praxis by [0] Origin</h3>
                  <p className="text-sm text-muted mb-6">
                    <a href="https://originhq.com" target="_blank" rel="noopener noreferrer" className="text-[var(--accent-info)]/70 hover:text-[var(--accent-info)] hover:underline">Origin</a> is an endpoint security company building protection for the semantic era of computing. As AI agents become integral to enterprise workflows, Origin provides the visibility and control organizations need to safely grant agents the permissions they require.
                  </p>
                  <p className="text-sm text-muted mb-8">
                    <a href="https://github.com/originsec/praxis" target="_blank" rel="noopener noreferrer" className="text-[var(--accent-info)]/70 hover:text-[var(--accent-info)] hover:underline">Praxis</a> is Origin's experimental research platform for exploring the adversarial boundaries of legitimate semantic tools. By understanding how computer-use agents and their underlying capabilities can be leveraged offensively, we build better defenses for the endpoints they operate on.
                  </p>
                  <div className="flex gap-4">
                    <a
                      href="https://originhq.com"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors"
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
