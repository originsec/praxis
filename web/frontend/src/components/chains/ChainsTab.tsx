import { useState, useEffect, useMemo } from 'react';
import { Play, Trash2, Clock, Edit2 } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { ChainBuilder } from './ChainBuilder';
import { Modal } from '../common/Modal';
import { RunModal } from '../common/RunModal';
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
    getConfig,
  } = useApp();

  const { chains, currentChain, chainError, chainSuccess } = state.chains;
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
      getConfig(['llm_model_definitions']);
    }
  }, [showBuilder, send, getConfig]);

  //
  // Handle success/error messages.
  //
  useEffect(() => {
    if (chainSuccess || chainError) {
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

  const handleRunFromModal = (chainId: string, nodeId: string, agentName: string) => {
    runChain(chainId, nodeId, agentName);
  };

  const handleSave = (definition: ChainDefinitionInput) => {
    if (editingChainId) {
      updateChain(editingChainId, definition);
    } else {
      createChain(definition);
    }
    setShowBuilder(false);
    setEditingChainId(null);
  };

  const handleCancel = () => {
    setShowBuilder(false);
    setEditingChainId(null);
  };

  if (showBuilder) {
    return (
      <div className="h-[calc(100vh-280px)] min-h-[300px] border border-subtle ascii-box">
        <ChainBuilder
          chain={editingChainId ? currentChain : null}
          onSave={handleSave}
          onCancel={handleCancel}
          operationDefs={operationDefs}
          modelDefs={modelDefs}
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
      {chains.length === 0 ? (
        <div className="text-center text-muted py-8">
          No chains defined. Create your first chain to get started.
        </div>
      ) : (
        <div className="border border-subtle ascii-box">
          <table className="w-full text-xs">
            <thead>
              <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                <th className="text-left px-4 py-2 text-muted tracking-wider">NAME</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">OPERATIONS</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">TIMEOUT</th>
                <th className="px-4 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {chains.map((chain) => (
                <tr
                  key={chain.id}
                  className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors"
                >
                  <td className="px-4 py-3">
                    <button
                      onClick={() => handleEdit(chain)}
                      className="text-left hover:text-[var(--accent-info)] transition-colors"
                    >
                      <p className="font-medium text-highlight">{chain.name}</p>
                      {chain.description && (
                        <p className="text-muted text-xs">{chain.description}</p>
                      )}
                    </button>
                  </td>
                  <td className="px-4 py-3">{chain.operation_count}</td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-1 text-muted">
                      <Clock size={12} />
                      {chain.timeout || 300}s
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center justify-end gap-2">
                      <button
                        onClick={() => handleRun(chain)}
                        className="p-2 hover:bg-[var(--accent-success)]/10 text-muted hover:text-[var(--accent-success)] transition-colors"
                        title="Run chain"
                        disabled={chain.disabled}
                      >
                        <Play size={14} />
                      </button>
                      <button
                        onClick={() => handleEdit(chain)}
                        className="p-2 hover:bg-[var(--accent-info)]/10 text-muted hover:text-[var(--accent-info)] transition-colors"
                        title="Edit chain"
                      >
                        <Edit2 size={14} />
                      </button>
                      <button
                        onClick={() => handleDeleteClick(chain)}
                        className="p-2 hover:bg-[var(--accent-error)]/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                        title="Delete chain"
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

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
        items={chains.filter(c => !c.disabled).map(chain => ({
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
