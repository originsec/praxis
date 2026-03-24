import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import {
  Zap, GitBranch, Play, Pencil, Trash2, Search, Plus, ChevronDown,
  Upload, CircleCheck, Ban,
} from 'lucide-react';
import { Modal } from '../common/Modal';
import { RunModal, type RunItem } from '../common/RunModal';
import { EditOpForm } from '../common/EditOpForm';
import { ChainBuilder } from '../chains/ChainBuilder';
import { ImportModal } from '../library/ImportModal';
import { useApp } from '../../context/AppContext';
import type { OperationDefinitionInfo, ChainDefinitionInput, ChainDefinitionInfo, TargetSpec } from '../../api/types';

interface ModelDefinition {
  name: string;
  provider: string;
  model: string;
  apiKey: string;
}

type FilterType = 'all' | 'operation' | 'chain';

interface LibraryModalProps {
  onClose: () => void;
}

export function LibraryModal({ onClose }: LibraryModalProps) {
  const {
    state, send,
    requestChainDefList, requestChain, createChain, updateChain, deleteChain,
    runOperation, runChain,
    getConfig, clearOpDefStatus, clearChainStatus, clearLastCreatedChain,
  } = useApp();

  const ops = state.operationDefs;
  const { chains, currentChain, chainError, chainSuccess, lastCreatedChainId } = state.chains;
  const opDefError = state.opDefError;
  const opDefSuccess = state.opDefSuccess;
  const nodes = state.systemState?.nodes ?? [];

  //
  // Top-level UI state.
  //

  const [filter, setFilter] = useState<FilterType>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [showAddMenu, setShowAddMenu] = useState(false);
  const addMenuRef = useRef<HTMLDivElement>(null);

  //
  // Run modal state.
  //

  const [runModalItem, setRunModalItem] = useState<{
    item: RunItem; variant: 'operation' | 'chain';
  } | null>(null);

  //
  // Delete confirmation modal state.
  //

  const [deleteTarget, setDeleteTarget] = useState<{
    id: string; name: string; type: 'operation' | 'chain';
  } | null>(null);

  //
  // Operation edit modal state.
  //

  const [editDef, setEditDef] = useState<OperationDefinitionInfo | null>(null);
  const [isNewOp, setIsNewOp] = useState(false);
  const [isSavingOp, setIsSavingOp] = useState(false);

  //
  // Chain builder modal state.
  //

  const [showChainBuilder, setShowChainBuilder] = useState(false);
  const [editingChainId, setEditingChainId] = useState<string | null>(null);
  const pendingSaveCallback = useRef<((result: 'saved' | 'error') => void) | null>(null);

  //
  // Import modal state.
  //

  const [showImportModal, setShowImportModal] = useState(false);

  //
  // Parse model definitions from config for chain builder.
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
  // Fetch ops and chains on mount.
  //

  useEffect(() => {
    send({ type: 'op_def_list' });
    requestChainDefList();
  }, [send, requestChainDefList]);

  //
  // Fetch config, toolkit, payloads when chain builder opens.
  //

  useEffect(() => {
    if (showChainBuilder) {
      getConfig(['llm_model_definitions']);
      send({ type: 'toolkit_list' });
      send({ type: 'payload_list' });
    }
  }, [showChainBuilder, send, getConfig]);

  //
  // Load chain definition for editing.
  //

  useEffect(() => {
    if (editingChainId) {
      requestChain(editingChainId);
    }
  }, [editingChainId, requestChain]);

  //
  // Show chain builder once chain is loaded.
  //

  useEffect(() => {
    if (editingChainId && currentChain && currentChain.id === editingChainId) {
      setShowChainBuilder(true);
    }
  }, [editingChainId, currentChain]);

  useEffect(() => {
    if (lastCreatedChainId && showChainBuilder && !editingChainId) {
      setEditingChainId(lastCreatedChainId);
      clearLastCreatedChain();
    }
  }, [lastCreatedChainId, showChainBuilder, editingChainId, clearLastCreatedChain]);

  //
  // Handle op save success/error.
  //

  useEffect(() => {
    if (opDefSuccess && isSavingOp) {
      setIsSavingOp(false);
      setEditDef(null);
      setIsNewOp(false);
      clearOpDefStatus();
      send({ type: 'op_def_list' });
    }
  }, [opDefSuccess, isSavingOp, clearOpDefStatus, send]);

  useEffect(() => {
    if (opDefError && isSavingOp) {
      setIsSavingOp(false);
    }
  }, [opDefError, isSavingOp]);

  //
  // Auto-clear chain status messages.
  //

  useEffect(() => {
    if (chainSuccess || chainError) {
      if (pendingSaveCallback.current) {
        pendingSaveCallback.current(chainError ? 'error' : 'saved');
        pendingSaveCallback.current = null;
      }
      const timer = setTimeout(() => clearChainStatus(), 3000);
      return () => clearTimeout(timer);
    }
  }, [chainSuccess, chainError, clearChainStatus]);

  //
  // Close add menu on outside click.
  //

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (addMenuRef.current && !addMenuRef.current.contains(event.target as Node)) {
        setShowAddMenu(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  //
  // Filtered and sorted items.
  //

  const filteredOps = useMemo(() => {
    if (filter === 'chain') return [];
    const q = searchQuery.toLowerCase().trim();
    let items = ops;
    if (q) {
      items = items.filter(
        op => op.name.toLowerCase().includes(q) ||
              op.category.toLowerCase().includes(q) ||
              op.short_name.toLowerCase().includes(q)
      );
    }
    return items.sort((a, b) => a.name.localeCompare(b.name));
  }, [ops, filter, searchQuery]);

  const filteredChains = useMemo(() => {
    if (filter === 'operation') return [];
    const q = searchQuery.toLowerCase().trim();
    let items = chains;
    if (q) {
      items = items.filter(
        c => c.name.toLowerCase().includes(q) ||
             c.category.toLowerCase().includes(q)
      );
    }
    return items.sort((a, b) => a.name.localeCompare(b.name));
  }, [chains, filter, searchQuery]);

  const totalCount = filteredOps.length + filteredChains.length;

  //
  // Handlers: Run.
  //

  const handleRun = useCallback((id: string, name: string, variant: 'operation' | 'chain') => {
    setRunModalItem({ item: { id, name }, variant });
  }, []);

  const handleRunAdvanced = useCallback((itemId: string, targetSpec: TargetSpec) => {
    if (!runModalItem) return;
    const allNodes = nodes;

    if (runModalItem.variant === 'operation') {
      const filteredNodes = targetSpec.node_ids.length > 0
        ? allNodes.filter(n => targetSpec.node_ids.includes(n.node_id))
        : targetSpec.os_filter
          ? allNodes.filter(n => n.os_details.toLowerCase().includes(targetSpec.os_filter!.toLowerCase()))
          : allNodes;

      for (const node of filteredNodes) {
        const agents = targetSpec.agent_short_names.length > 0
          ? node.discovered_agents.filter(a => targetSpec.agent_short_names.includes(a.short_name))
          : node.discovered_agents;

        for (const agent of agents) {
          runOperation(node.node_id, agent.short_name, itemId);
        }
      }
    } else {
      const filteredNodes = targetSpec.node_ids.length > 0
        ? allNodes.filter(n => targetSpec.node_ids.includes(n.node_id))
        : targetSpec.os_filter
          ? allNodes.filter(n => n.os_details.toLowerCase().includes(targetSpec.os_filter!.toLowerCase()))
          : allNodes;

      const primaryNode = filteredNodes[0];
      if (!primaryNode) return;

      const agentName = targetSpec.agent_short_names.length > 0
        ? targetSpec.agent_short_names[0]
        : primaryNode.discovered_agents[0]?.short_name || '';
      runChain(itemId, primaryNode.node_id, agentName, undefined, targetSpec);
    }

    setRunModalItem(null);
  }, [runModalItem, nodes, runOperation, runChain]);

  //
  // Handlers: Delete.
  //

  const handleDeleteClick = useCallback((id: string, name: string, type: 'operation' | 'chain') => {
    setDeleteTarget({ id, name, type });
  }, []);

  const handleDeleteConfirm = useCallback(() => {
    if (!deleteTarget) return;
    if (deleteTarget.type === 'operation') {
      send({ type: 'op_def_delete', full_name: deleteTarget.id });
      window.setTimeout(() => send({ type: 'op_def_list' }), 500);
    } else {
      deleteChain(deleteTarget.id);
    }
    setDeleteTarget(null);
  }, [deleteTarget, send, deleteChain]);

  //
  // Handlers: Toggle disable.
  //

  const handleToggleOpDisabled = useCallback((op: OperationDefinitionInfo) => {
    send({ type: 'op_def_set_disabled', full_name: op.full_name, disabled: !op.disabled });
  }, [send]);

  const handleToggleChainDisabled = useCallback((chain: ChainDefinitionInfo) => {
    send({ type: 'chain_set_disabled', chain_id: chain.id, disabled: !chain.disabled });
  }, [send]);

  //
  // Handlers: Edit operation.
  //

  const handleEditOp = useCallback((op: OperationDefinitionInfo) => {
    setEditDef({ ...op });
    setIsNewOp(false);
    clearOpDefStatus();
  }, [clearOpDefStatus]);

  const handleNewOp = useCallback(() => {
    setShowAddMenu(false);
    setEditDef({
      name: '', short_name: '', category: '', full_name: '',
      description: '', agent_info: '', timeout: 60, mode: 'one-shot',
      agent_iterations: 5, operation_prompt: '', operation_chain: [],
      disabled: false, yolo_mode: false,
    });
    setIsNewOp(true);
    clearOpDefStatus();
  }, [clearOpDefStatus]);

  const handleSaveOp = useCallback(() => {
    if (!editDef) return;
    const opData = {
      item_type: 'operation',
      name: editDef.name,
      short_name: editDef.short_name,
      category: editDef.category,
      description: editDef.description,
      agent_info: editDef.agent_info,
      timeout: editDef.timeout,
      operation_prompt: editDef.operation_prompt,
      mode: editDef.mode,
      agent_iterations: editDef.agent_iterations,
      disabled: editDef.disabled,
      yolo_mode: editDef.yolo_mode,
      ...(editDef.model_ref && { model_ref: editDef.model_ref }),
    };
    clearOpDefStatus();
    setIsSavingOp(true);
    send({ type: 'op_def_add', content: JSON.stringify(opData) });
  }, [editDef, clearOpDefStatus, send]);

  const updateEditDef = useCallback((field: keyof OperationDefinitionInfo, value: string | number | boolean | string[]) => {
    setEditDef(prev => prev ? { ...prev, [field]: value } : prev);
  }, []);

  const closeEditOp = useCallback(() => {
    setEditDef(null);
    setIsNewOp(false);
    setIsSavingOp(false);
    clearOpDefStatus();
  }, [clearOpDefStatus]);

  //
  // Handlers: Chain builder.
  //

  const handleEditChain = useCallback((chainId: string) => {
    setEditingChainId(chainId);
  }, []);

  const handleNewChain = useCallback(() => {
    setShowAddMenu(false);
    setEditingChainId(null);
    setShowChainBuilder(true);
  }, []);

  const handleSaveChain = useCallback((definition: ChainDefinitionInput, onResult?: (result: 'saved' | 'error') => void) => {
    if (onResult) pendingSaveCallback.current = onResult;
    clearChainStatus();

    if (editingChainId) {
      updateChain(editingChainId, definition);
    } else {
      createChain(definition);
    }
  }, [editingChainId, updateChain, createChain, clearChainStatus]);

  const handleCancelChain = useCallback(() => {
    setShowChainBuilder(false);
    setEditingChainId(null);
  }, []);

  //
  // Handlers: Import.
  //

  const handleImport = useCallback(() => {
    setShowAddMenu(false);
    setShowImportModal(true);
  }, []);

  return (
    <>
      <Modal
        isOpen={true}
        onClose={onClose}
        title="Library"
        size="lg"
        noPadding
        resizable
        storageKey="cmd-library"
        defaultWidth={672}
        defaultHeight={Math.round(window.innerHeight * 0.5)}
      >
        <div className="flex flex-col h-full">

          {/*
          //
          // Top bar: filters + search + add button.
          //
          */}
          <div className="flex items-center gap-1.5 px-2.5 py-1.5 border-b border-subtle bg-[var(--bg-tertiary)]">
            <div className="flex gap-0.5">
              {([
                { value: 'all' as FilterType, label: 'All' },
                { value: 'operation' as FilterType, label: 'Ops' },
                { value: 'chain' as FilterType, label: 'Chains' },
              ]).map(f => (
                <button
                  key={f.value}
                  onClick={() => setFilter(f.value)}
                  className={`px-2 py-0.5 text-[10px] transition-colors ${
                    filter === f.value
                      ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-[var(--accent-info)]/50'
                      : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--highlight)]'
                  }`}
                >
                  {f.label}
                </button>
              ))}
            </div>

            <div className="relative flex-1 max-w-[180px]">
              <Search size={11} className="absolute left-2 top-1/2 -translate-y-1/2 text-muted" />
              <input
                type="text"
                placeholder="Search..."
                value={searchQuery}
                onChange={e => setSearchQuery(e.target.value)}
                className="w-full pl-6 pr-2 py-0.5 text-[10px] bg-[var(--bg-primary)] border border-dim focus:outline-none focus:border-subtle"
              />
            </div>

            <div className="relative ml-auto" ref={addMenuRef}>
              <button
                onClick={() => setShowAddMenu(!showAddMenu)}
                className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-[var(--accent-success)]/20 text-[var(--accent-success)] border border-dim hover:border-[var(--accent-success)] transition-colors"
              >
                <Plus size={11} />
                Add
                <ChevronDown size={10} />
              </button>

              {showAddMenu && (
                <div className="absolute right-0 mt-1 w-36 bg-[var(--bg-secondary)] border border-dim z-50">
                  <button
                    onClick={handleNewOp}
                    className="flex items-center gap-1.5 w-full px-2.5 py-1.5 text-[10px] text-highlight hover:bg-[var(--highlight)] transition-colors text-left"
                  >
                    <Zap size={10} className="text-[var(--accent-purple)]" />
                    New Operation
                  </button>
                  <button
                    onClick={handleNewChain}
                    className="flex items-center gap-1.5 w-full px-2.5 py-1.5 text-[10px] text-highlight hover:bg-[var(--highlight)] transition-colors text-left"
                  >
                    <GitBranch size={10} className="text-[var(--accent-info)]" />
                    New Chain
                  </button>
                  <div className="border-t border-dim" />
                  <button
                    onClick={handleImport}
                    className="flex items-center gap-1.5 w-full px-2.5 py-1.5 text-[10px] text-highlight hover:bg-[var(--highlight)] transition-colors text-left"
                  >
                    <Upload size={10} className="text-muted" />
                    Import JSON
                  </button>
                </div>
              )}
            </div>
          </div>

          {/*
          //
          // Scrollable item list.
          //
          */}
          <div className="flex-1 overflow-y-auto">
            {totalCount === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-muted">
                <Search size={18} className="mb-1.5 opacity-40" />
                <p className="text-[10px]">
                  {searchQuery ? 'No items match your search.' : 'No operations or chains defined.'}
                </p>
              </div>
            ) : (
              <div className="divide-y divide-[var(--border-dim)]">
                {filteredOps.map(op => (
                  <OpRow
                    key={op.full_name}
                    op={op}
                    onRun={() => handleRun(op.full_name, op.name, 'operation')}
                    onEdit={() => handleEditOp(op)}
                    onToggleDisabled={() => handleToggleOpDisabled(op)}
                    onDelete={() => handleDeleteClick(op.full_name, op.name, 'operation')}
                  />
                ))}
                {filteredChains.map(chain => (
                  <ChainRow
                    key={chain.id}
                    chain={chain}
                    onRun={() => handleRun(chain.id, chain.name, 'chain')}
                    onEdit={() => handleEditChain(chain.id)}
                    onToggleDisabled={() => handleToggleChainDisabled(chain)}
                    onDelete={() => handleDeleteClick(chain.id, chain.name, 'chain')}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      </Modal>

      {/*
      //
      // Run modal.
      //
      */}
      {runModalItem && (
        <RunModal
          isOpen={true}
          onClose={() => setRunModalItem(null)}
          title={`Run ${runModalItem.variant === 'operation' ? 'Operation' : 'Chain'}`}
          items={[runModalItem.item]}
          preSelectedItem={runModalItem.item}
          variant={runModalItem.variant}
          nodes={nodes}
          onRun={(itemId, targetSpec) => {
            handleRunAdvanced(itemId, targetSpec);
          }}
        />
      )}

      {/*
      //
      // Delete confirmation modal.
      //
      */}
      {deleteTarget && (
        <Modal
          isOpen={true}
          onClose={() => setDeleteTarget(null)}
          title={`Delete ${deleteTarget.type === 'operation' ? 'Operation' : 'Chain'}`}
        >
          <div className="space-y-3">
            <p className="text-xs">
              Are you sure you want to delete{' '}
              <span className="font-medium text-[var(--accent-error)]">"{deleteTarget.name}"</span>?
            </p>
            <p className="text-[10px] text-muted">This action cannot be undone.</p>
            <div className="flex justify-end gap-2 pt-1">
              <button
                onClick={() => setDeleteTarget(null)}
                className="px-3 py-1.5 text-[10px] tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleDeleteConfirm}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-[10px] tracking-wider bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors"
              >
                <Trash2 size={11} />
                Delete
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/*
      //
      // Edit operation modal.
      //
      */}
      {editDef && (
        <Modal
          isOpen={true}
          onClose={closeEditOp}
          title={isNewOp ? 'New Operation' : `Edit: ${editDef.name || 'Operation'}`}
          size="lg"
          noPadding
          resizable
          storageKey="cmd-edit-op"
          defaultWidth={672}
          defaultHeight={Math.round(window.innerHeight * 0.75)}
        >
          <EditOpForm
            editDef={editDef}
            isNewOp={isNewOp}
            isSaving={isSavingOp}
            error={opDefError}
            onUpdate={updateEditDef}
            onSave={handleSaveOp}
            onExport={!isNewOp ? () => {
              const exportData = {
                item_type: 'operation',
                name: editDef.name,
                short_name: editDef.short_name,
                category: editDef.category,
                description: editDef.description,
                agent_info: editDef.agent_info,
                timeout: editDef.timeout,
                operation_prompt: editDef.operation_prompt,
                mode: editDef.mode,
                agent_iterations: editDef.agent_iterations,
                disabled: editDef.disabled,
                yolo_mode: editDef.yolo_mode,
                ...(editDef.model_ref && { model_ref: editDef.model_ref }),
              };
              const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
              const url = URL.createObjectURL(blob);
              const a = document.createElement('a');
              a.href = url;
              a.download = `${editDef.category}_${editDef.short_name}.json`;
              document.body.appendChild(a);
              a.click();
              document.body.removeChild(a);
              URL.revokeObjectURL(url);
            } : undefined}
            onCancel={closeEditOp}
          />
        </Modal>
      )}

      {/*
      //
      // Chain builder modal.
      //
      */}
      {showChainBuilder && (
        <Modal
          isOpen={true}
          onClose={handleCancelChain}
          title={editingChainId ? 'Edit Chain' : 'New Chain'}
          size="full"
          noPadding
          resizable
          storageKey="cmd-chain-builder"
          defaultWidth={Math.round(window.innerWidth * 0.9)}
          defaultHeight={Math.round(window.innerHeight * 0.85)}
        >
          <div className="h-full">
            <ChainBuilder
              chain={editingChainId ? currentChain : null}
              onSave={handleSaveChain}
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
              onCancel={handleCancelChain}
              operationDefs={ops}
              modelDefs={modelDefs}
              toolkitTools={state.toolkit.tools}
              payloads={state.payloads}
              send={send}
              opDefSuccess={opDefSuccess}
              opDefError={opDefError}
              clearOpDefStatus={clearOpDefStatus}
            />
          </div>
        </Modal>
      )}

      {/*
      //
      // Import modal.
      //
      */}
      {showImportModal && (
        <ImportModal
          isOpen={true}
          onClose={() => setShowImportModal(false)}
        />
      )}
    </>
  );
}

//
// Operation row component.
//

function OpRow({ op, onRun, onEdit, onToggleDisabled, onDelete }: {
  op: OperationDefinitionInfo;
  onRun: () => void;
  onEdit: () => void;
  onToggleDisabled: () => void;
  onDelete: () => void;
}) {
  return (
    <div className={`group flex items-center gap-2 px-2.5 py-1.5 hover:bg-[var(--highlight)] transition-colors ${op.disabled ? 'opacity-50' : ''}`}>
      <Zap size={10} className="text-[var(--accent-purple)] flex-shrink-0" />
      {op.disabled && <Ban size={9} className="text-[var(--accent-error)]/60 flex-shrink-0" />}

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="text-[11px] font-medium text-highlight truncate">{op.name}</span>
        </div>
        <div className="flex items-center gap-1.5 text-[9px] text-muted">
          <span>{op.category}</span>
          <span className="text-[var(--border-subtle)]">·</span>
          <span>{op.mode}</span>
          {op.description && (
            <>
              <span className="text-[var(--border-subtle)]">·</span>
              <span className="truncate max-w-[220px]">{op.description}</span>
            </>
          )}
        </div>
      </div>

      <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0">
        <button
          onClick={e => { e.stopPropagation(); onRun(); }}
          disabled={op.disabled}
          className="p-1 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20 transition-colors disabled:opacity-30"
          title="Run"
        >
          <Play size={10} />
        </button>
        <button
          onClick={e => { e.stopPropagation(); onToggleDisabled(); }}
          className={`p-1 transition-colors ${op.disabled ? 'text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20' : 'text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/20'}`}
          title={op.disabled ? 'Enable' : 'Disable'}
        >
          {op.disabled ? <CircleCheck size={10} /> : <Ban size={10} />}
        </button>
        <button
          onClick={e => { e.stopPropagation(); onEdit(); }}
          className="p-1 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/20 transition-colors"
          title="Edit"
        >
          <Pencil size={10} />
        </button>
        <button
          onClick={e => { e.stopPropagation(); onDelete(); }}
          className="p-1 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
          title="Delete"
        >
          <Trash2 size={10} />
        </button>
      </div>
    </div>
  );
}

//
// Chain row component.
//

function ChainRow({ chain, onRun, onEdit, onToggleDisabled, onDelete }: {
  chain: ChainDefinitionInfo;
  onRun: () => void;
  onEdit: () => void;
  onToggleDisabled: () => void;
  onDelete: () => void;
}) {
  return (
    <div className={`group flex items-center gap-2 px-2.5 py-1.5 hover:bg-[var(--highlight)] transition-colors ${chain.disabled ? 'opacity-50' : ''}`}>
      <GitBranch size={10} className="text-[var(--accent-info)] flex-shrink-0" />
      {chain.disabled && <Ban size={9} className="text-[var(--accent-error)]/60 flex-shrink-0" />}

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="text-[11px] font-medium text-highlight truncate">{chain.name}</span>
        </div>
        <div className="flex items-center gap-1.5 text-[9px] text-muted">
          <span>{chain.category || 'uncategorized'}</span>
          <span className="text-[var(--border-subtle)]">·</span>
          <span>{chain.element_count} elements</span>
          {chain.description && (
            <>
              <span className="text-[var(--border-subtle)]">·</span>
              <span className="truncate max-w-[220px]">{chain.description}</span>
            </>
          )}
        </div>
      </div>

      <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0">
        <button
          onClick={e => { e.stopPropagation(); onRun(); }}
          disabled={chain.disabled}
          className="p-1 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20 transition-colors disabled:opacity-30"
          title="Run"
        >
          <Play size={10} />
        </button>
        <button
          onClick={e => { e.stopPropagation(); onToggleDisabled(); }}
          className={`p-1 transition-colors ${chain.disabled ? 'text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20' : 'text-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/20'}`}
          title={chain.disabled ? 'Enable' : 'Disable'}
        >
          {chain.disabled ? <CircleCheck size={10} /> : <Ban size={10} />}
        </button>
        <button
          onClick={e => { e.stopPropagation(); onEdit(); }}
          className="p-1 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/20 transition-colors"
          title="Edit"
        >
          <Pencil size={10} />
        </button>
        <button
          onClick={e => { e.stopPropagation(); onDelete(); }}
          className="p-1 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
          title="Delete"
        >
          <Trash2 size={10} />
        </button>
      </div>
    </div>
  );
}

//
// Operation edit form — same fields as LibraryTab's edit modal.
//

