import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { Play, Trash2, Clock, Edit2, Zap } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { ChainBuilder } from './ChainBuilder';
import { Modal } from '../common/Modal';
import { RunModal } from '../common/RunModal';
import { DataTable, type ColumnDef, type RowAction } from '../common/DataTable';
import type { ChainDefinitionInfo, ChainDefinitionInput, NodeState } from '../../api/types';

//
// Model definition type for dropdown.
//
interface ModelDefinition {
  name: string;
  provider: string;
  model: string;
  apiKey: string;
}

interface ChainsTabProps {
  nodes: NodeState[];
  triggerNew?: boolean;
  onNewHandled?: () => void;
  triggerEdit?: string | null;
  onEditHandled?: () => void;
}

export function ChainsTab({ nodes, triggerNew, onNewHandled, triggerEdit, onEditHandled }: ChainsTabProps) {
  const {
    state,
    send,
    requestChainDefList,
    requestChain,
    createChain,
    updateChain,
    deleteChain,
    runChain,
    clearChainStatus,
    clearLastCreatedChain,
    clearOpDefStatus,
    getConfig,
  } = useApp();

  const { chains, currentChain, chainError, chainSuccess, lastCreatedChainId } = state.chains;
  const operationDefs = state.operationDefs;

  //
  // Parse model definitions from config.
  //
  const modelDefs = useMemo<ModelDefinition[]>(() => {
    const raw = state.config.llm_model_definitions;
    if (!raw) return [];
    try {
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }, [state.config.llm_model_definitions]);

  //
  // Local state.
  //
  const [showBuilder, setShowBuilder] = useState(false);
  const [editingChainId, setEditingChainId] = useState<string | null>(null);
  const [showRunModal, setShowRunModal] = useState(false);
  const [preSelectedChain, setPreSelectedChain] = useState<ChainDefinitionInfo | null>(null);

  //
  // Delete confirmation modal state.
  //
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [chainToDelete, setChainToDelete] = useState<ChainDefinitionInfo | null>(null);
  const pendingSaveCallback = useRef<((result: 'saved' | 'error') => void) | null>(null);


  //
  // Fetch chains on mount.
  //
  useEffect(() => {
    requestChainDefList();
  }, [requestChainDefList]);

  //
  // Fetch operation definitions and model config when builder opens.
  //
  useEffect(() => {
    if (showBuilder) {
      send({ type: 'op_def_list' });
      send({ type: 'toolkit_list' });
      send({ type: 'payload_list' });
      getConfig(['llm_model_definitions']);
    }
  }, [showBuilder, send, getConfig]);

  //
  // Handle success/error messages.
  //
  useEffect(() => {
    if (chainSuccess || chainError) {
      if (pendingSaveCallback.current) {
        pendingSaveCallback.current(chainError ? 'error' : 'saved');
        pendingSaveCallback.current = null;
      }
      const timer = setTimeout(() => {
        clearChainStatus();
      }, 3000);
      return () => clearTimeout(timer);
    }
  }, [chainSuccess, chainError, clearChainStatus]);

  //
  // Load chain for editing - request it when editingChainId changes.
  //
  useEffect(() => {
    if (editingChainId) {
      requestChain(editingChainId);
    }
  }, [editingChainId, requestChain]);

  //
  // Show builder once chain is loaded for editing.
  //
  useEffect(() => {
    if (editingChainId && currentChain && currentChain.id === editingChainId) {
      setShowBuilder(true);
    }
  }, [editingChainId, currentChain]);

  //
  // After creating a new chain, transition to editing it so subsequent saves
  // update the same instance instead of creating duplicates.
  //
  useEffect(() => {
    if (lastCreatedChainId && showBuilder && !editingChainId) {
      setEditingChainId(lastCreatedChainId);
      clearLastCreatedChain();
    }
  }, [lastCreatedChainId, showBuilder, editingChainId, clearLastCreatedChain]);

  //
  // Handle external trigger to create new chain.
  //
  useEffect(() => {
    if (triggerNew) {
      setEditingChainId(null);
      setShowBuilder(true);
      onNewHandled?.();
    }
  }, [triggerNew, onNewHandled]);

  //
  // Handle external trigger to edit specific chain.
  //
  useEffect(() => {
    if (triggerEdit) {
      setEditingChainId(triggerEdit);
      onEditHandled?.();
    }
  }, [triggerEdit, onEditHandled]);

  const handleEdit = (chain: ChainDefinitionInfo) => {
    //
    // Set the editing ID - this will trigger the useEffect to load the chain.
    //
    setEditingChainId(chain.id);
  };

  const handleDeleteClick = (chain: ChainDefinitionInfo) => {
    setChainToDelete(chain);
    setShowDeleteModal(true);
  };

  const handleDeleteConfirm = () => {
    if (chainToDelete) {
      deleteChain(chainToDelete.id);
      setShowDeleteModal(false);
      setChainToDelete(null);
    }
  };

  const handleRun = (chain: ChainDefinitionInfo) => {
    setPreSelectedChain(chain);
    setShowRunModal(true);
  };

  const handleRunFromModal = (chainId: string, targetSpec: import('../../api/types').TargetSpec) => {
    const allNodes = nodes;
    const filteredNodes = targetSpec.node_ids.length > 0
      ? allNodes.filter(n => targetSpec.node_ids.includes(n.node_id))
      : targetSpec.os_filter
        ? allNodes.filter(n => n.os_details.toLowerCase().includes(targetSpec.os_filter!.toLowerCase()))
        : allNodes;
    const primaryNode = filteredNodes[0];
    if (!primaryNode) return;
    const agentName = targetSpec.agent_short_names.length > 0
      ? targetSpec.agent_short_names[0]
      : primaryNode.selected_agent?.short_name || primaryNode.discovered_agents?.[0]?.short_name || '';
    runChain(chainId, primaryNode.node_id, agentName, undefined, targetSpec);
  };

  const handleSave = useCallback((definition: ChainDefinitionInput, onResult?: (result: 'saved' | 'error') => void) => {
    if (onResult) pendingSaveCallback.current = onResult;
    clearChainStatus();

    if (editingChainId) {
      updateChain(editingChainId, definition);
    } else {

      //
      // If a chain with this name already exists, update it instead of
      // creating a duplicate.
      //

      const existing = chains.find(
        c => c.name.toLowerCase() === definition.name.trim().toLowerCase()
      );
      if (existing) {
        setEditingChainId(existing.id);
        updateChain(existing.id, definition);
        return;
      }
      createChain(definition);
    }
  }, [editingChainId, chains, updateChain, createChain, clearChainStatus]);

  const handleDuplicate = (definition: ChainDefinitionInput) => {
    createChain(definition);
    setShowBuilder(false);
    setEditingChainId(null);
  };

  const handleCancel = () => {
    setShowBuilder(false);
    setEditingChainId(null);
  };

  const chainColumns: ColumnDef<ChainDefinitionInfo>[] = [
    {
      key: 'name',
      header: 'Name',
      sortable: false,
      render: (_: unknown, chain: ChainDefinitionInfo) => (
        <div>
          <p className="font-medium text-highlight">
            {chain.name}
            {(chain.trigger_count ?? 0) > 0 && (
              <span className="ml-2 inline-flex items-center gap-0.5 text-[10px] text-[var(--accent-warning)]">
                <Zap size={10} />
                {chain.trigger_count}
              </span>
            )}
          </p>
          {chain.description && (
            <p className="text-muted text-xs">{chain.description}</p>
          )}
        </div>
      ),
    },
    {
      key: 'operation_count',
      header: 'Operations',
      sortable: false,
    },
    {
      key: 'timeout',
      header: 'Timeout',
      sortable: false,
      render: (_: unknown, chain: ChainDefinitionInfo) => (
        <div className="flex items-center gap-1 text-muted">
          <Clock size={12} />
          {chain.timeout || 300}s
        </div>
      ),
    },
  ];

  const chainActions: RowAction<ChainDefinitionInfo>[] = [
    {
      icon: <Play size={14} />,
      label: 'Run chain',
      onClick: (chain) => handleRun(chain),
      disabled: (chain) => !!chain.disabled,
      hoverColor: 'var(--accent-success)',
    },
    {
      icon: <Edit2 size={14} />,
      label: 'Edit chain',
      onClick: (chain) => handleEdit(chain),
      hoverColor: 'var(--accent-info)',
    },
    {
      icon: <Trash2 size={14} />,
      label: 'Delete chain',
      onClick: (chain) => handleDeleteClick(chain),
      hoverColor: 'var(--accent-error)',
    },
  ];

  if (showBuilder) {
    return (
      <div className="flex-1 min-h-[400px] border border-subtle" style={{ height: 'calc(100vh - 160px)' }}>
        <ChainBuilder
          chain={editingChainId ? currentChain : null}
          onSave={handleSave}
          onDuplicate={handleDuplicate}
          onExport={editingChainId ? (definition) => {
            const exportData = {
              item_type: 'chain',
              name: definition.name,
              description: definition.description,
              category: definition.category,
              elements: definition.elements,
              connections: definition.connections,
              disabled: definition.disabled,
              timeout: definition.timeout,
              positions: definition.positions,
            };
            const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `chain_${definition.name.toLowerCase().replace(/\s+/g, '_')}.json`;
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            URL.revokeObjectURL(url);
          } : undefined}
          onCancel={handleCancel}
          operationDefs={operationDefs}
          modelDefs={modelDefs}
          nodes={nodes}
          toolkitTools={state.toolkit.tools}
          payloads={state.payloads}
          send={send}
          saveStatus={chainSuccess}
          saveError={chainError}
          opDefSuccess={state.opDefSuccess}
          opDefError={state.opDefError}
          clearOpDefStatus={clearOpDefStatus}
        />
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/*
      //
      // Status messages.
      //
      */}
      {chainError && (
        <div className="ascii-box bg-[var(--accent-error)]/20 border-[var(--accent-error)] p-3 text-sm">
          {chainError}
        </div>
      )}
      {chainSuccess && (
        <div className="ascii-box bg-[var(--accent-success)]/20 border-[var(--accent-success)] p-3 text-sm">
          {chainSuccess}
        </div>
      )}

      {/*
      //
      // Chains list.
      //
      */}
      <div className="border border-subtle ascii-box">
        <DataTable
          data={chains}
          columns={chainColumns}
          getRowKey={c => c.id}
          actions={chainActions}
          pinnedActions
          emptyMessage="No chains defined. Create your first chain to get started."
        />
      </div>

      {/*
      //
      // Run Chain Modal.
      //
      */}
      <RunModal
        isOpen={showRunModal}
        onClose={() => {
          setShowRunModal(false);
          setPreSelectedChain(null);
        }}
        onRun={handleRunFromModal}
        title="Run Chain"
        items={chains.filter(c => !c.disabled).sort((a, b) => a.name.localeCompare(b.name)).map(chain => ({
          id: chain.id,
          name: chain.name,
          description: chain.description,
          badge: `${chain.element_count} elements`,
        }))}
        nodes={nodes}
        variant="chain"
        preSelectedItem={preSelectedChain ? {
          id: preSelectedChain.id,
          name: preSelectedChain.name,
          description: preSelectedChain.description,
          badge: `${preSelectedChain.element_count} elements`,
        } : null}
      />

      {/*
      //
      // Delete Confirmation Modal.
      //
      */}
      <Modal
        isOpen={showDeleteModal}
        title="Delete Chain"
        onClose={() => {
          setShowDeleteModal(false);
          setChainToDelete(null);
        }}
      >
        <div className="space-y-4">
          <p className="text-sm">
            Are you sure you want to delete the chain{' '}
            <span className="font-medium text-[var(--accent-error)]">"{chainToDelete?.name}"</span>?
          </p>
          <p className="text-xs text-muted">
            This action cannot be undone.
          </p>

          <div className="flex justify-end gap-3 pt-2">
            <button
              onClick={() => {
                setShowDeleteModal(false);
                setChainToDelete(null);
              }}
              className="px-4 py-2 text-sm border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleDeleteConfirm}
              className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
            >
              <Trash2 size={16} />
              Delete
            </button>
          </div>
        </div>
      </Modal>

    </div>
  );
}
