import { useState, useEffect, useMemo, useRef } from 'react';
import { Play, Trash2, Edit2, Zap, GitBranch, Download, Upload, Search, Plus, ChevronDown, Loader2, ToggleLeft, ToggleRight, Save } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { ChainBuilder } from '../chains/ChainBuilder';
import { Modal } from '../common/Modal';
import { RunModal } from '../common/RunModal';
import { ImportModal } from './ImportModal';
import type { LibraryItem, LibraryItemType, OperationDefinitionInfo, ChainDefinitionInput, NodeState } from '../../api/types';

//
// Model definition type for dropdown.
//
interface ModelDefinition {
  name: string;
  provider: string;
  model: string;
  apiKey: string;
}

interface LibraryTabProps {
  nodes: NodeState[];
}

type FilterType = 'all' | 'operation' | 'chain';

export function LibraryTab({ nodes }: LibraryTabProps) {
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
    clearOpDefStatus,
    getConfig,
  } = useApp();

  const { chains, currentChain, chainError, chainSuccess } = state.chains;
  const operationDefs = state.operationDefs;
  const opDefError = state.opDefError;
  const opDefSuccess = state.opDefSuccess;

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
  const [filter, setFilter] = useState<FilterType>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [showAddMenu, setShowAddMenu] = useState(false);
  const [showImportModal, setShowImportModal] = useState(false);

  //
  // Chain builder state.
  //
  const [showChainBuilder, setShowChainBuilder] = useState(false);
  const [editingChainId, setEditingChainId] = useState<string | null>(null);

  //
  // Operation edit modal state.
  //
  const [showEditOpModal, setShowEditOpModal] = useState(false);
  const [editDef, setEditDef] = useState<OperationDefinitionInfo | null>(null);
  const [isNewOperation, setIsNewOperation] = useState(false);
  const [isEditing, setIsEditing] = useState(false);

  //
  // Run modal state.
  //
  const [showRunModal, setShowRunModal] = useState(false);
  const [runModalVariant, setRunModalVariant] = useState<'operation' | 'chain'>('operation');
  const [preSelectedItem, setPreSelectedItem] = useState<{ id: string; name: string; description: string; badge: string } | null>(null);

  //
  // Delete confirmation modal state.
  //
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [itemToDelete, setItemToDelete] = useState<LibraryItem | null>(null);

  const addMenuRef = useRef<HTMLDivElement>(null);

  //
  // Fetch data on mount.
  //
  useEffect(() => {
    send({ type: 'op_def_list' });
    requestChainDefList();
  }, [send, requestChainDefList]);

  //
  // Fetch config when chain builder opens.
  //
  useEffect(() => {
    if (showChainBuilder) {
      getConfig(['llm_model_definitions']);
    }
  }, [showChainBuilder, getConfig]);

  //
  // Load chain for editing.
  //
  useEffect(() => {
    if (editingChainId) {
      requestChain(editingChainId);
    }
  }, [editingChainId, requestChain]);

  //
  // Show builder once chain is loaded.
  //
  useEffect(() => {
    if (editingChainId && currentChain && currentChain.id === editingChainId) {
      setShowChainBuilder(true);
    }
  }, [editingChainId, currentChain]);

  //
  // Handle success/error for chains.
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
  // Handle success/error for operations.
  //
  useEffect(() => {
    if (opDefSuccess && isEditing) {
      setIsEditing(false);
      setShowEditOpModal(false);
      setEditDef(null);
      setIsNewOperation(false);
      clearOpDefStatus();
      send({ type: 'op_def_list' });
    }
  }, [opDefSuccess, isEditing, clearOpDefStatus, send]);

  useEffect(() => {
    if (opDefError && isEditing) {
      setIsEditing(false);
    }
  }, [opDefError, isEditing]);

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
  // Transform operations and chains into unified library items.
  //
  const libraryItems = useMemo<LibraryItem[]>(() => {
    const opItems: LibraryItem[] = operationDefs.map((op) => ({
      id: op.full_name,
      type: 'operation' as LibraryItemType,
      name: op.name,
      description: op.description,
      category: op.category,
      shortName: op.short_name,
      disabled: op.disabled,
      mode: op.mode,
      timeout: op.timeout,
      yoloMode: op.yolo_mode,
    }));

    const chainItems: LibraryItem[] = chains.map((chain) => ({
      id: chain.id,
      type: 'chain' as LibraryItemType,
      name: chain.name,
      description: chain.description,
      category: chain.category,
      disabled: chain.disabled,
      timeout: chain.timeout,
      elementCount: chain.element_count,
      operationCount: chain.operation_count,
    }));

    return [...opItems, ...chainItems];
  }, [operationDefs, chains]);

  //
  // Filter and search items.
  //
  const filteredItems = useMemo(() => {
    let items = libraryItems;

    //
    // Filter by type.
    //
    if (filter !== 'all') {
      items = items.filter((item) => item.type === filter);
    }

    //
    // Search by name, shortName, or category.
    //
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      items = items.filter(
        (item) =>
          item.name.toLowerCase().includes(query) ||
          item.category.toLowerCase().includes(query) ||
          (item.shortName && item.shortName.toLowerCase().includes(query))
      );
    }

    //
    // Sort: operations first, then chains, alphabetically within each.
    //
    return items.sort((a, b) => {
      if (a.type !== b.type) {
        return a.type === 'operation' ? -1 : 1;
      }
      return a.name.localeCompare(b.name);
    });
  }, [libraryItems, filter, searchQuery]);

  //
  // Handlers.
  //
  const handleAddOperation = () => {
    setShowAddMenu(false);
    const newDef: OperationDefinitionInfo = {
      name: '',
      short_name: '',
      category: 'custom',
      full_name: '',
      description: '',
      agent_info: '',
      timeout: 60,
      mode: 'one-shot',
      agent_iterations: 5,
      operation_prompt: '',
      operation_chain: [],
      disabled: false,
      yolo_mode: false,
    };
    setEditDef(newDef);
    setIsNewOperation(true);
    clearOpDefStatus();
    setShowEditOpModal(true);
  };

  const handleAddChain = () => {
    setShowAddMenu(false);
    setEditingChainId(null);
    setShowChainBuilder(true);
  };

  const handleImport = () => {
    setShowAddMenu(false);
    setShowImportModal(true);
  };

  const handleEditItem = (item: LibraryItem) => {
    if (item.type === 'operation') {
      const op = operationDefs.find((o) => o.full_name === item.id);
      if (op) {
        setEditDef({ ...op });
        setIsNewOperation(false);
        clearOpDefStatus();
        setShowEditOpModal(true);
      }
    } else {
      setEditingChainId(item.id);
    }
  };

  const handleDeleteClick = (item: LibraryItem) => {
    setItemToDelete(item);
    setShowDeleteModal(true);
  };

  const handleDeleteConfirm = () => {
    if (!itemToDelete) return;

    if (itemToDelete.type === 'operation') {
      send({ type: 'op_def_delete', full_name: itemToDelete.id });
      window.setTimeout(() => send({ type: 'op_def_list' }), 500);
    } else {
      deleteChain(itemToDelete.id);
    }

    setShowDeleteModal(false);
    setItemToDelete(null);
  };

  const handleRunItem = (item: LibraryItem) => {
    if (item.type === 'operation') {
      const op = operationDefs.find((o) => o.full_name === item.id);
      if (op) {
        setRunModalVariant('operation');
        setPreSelectedItem({
          id: op.full_name,
          name: op.name,
          description: op.description,
          badge: op.category,
        });
        setShowRunModal(true);
      }
    } else {
      const chain = chains.find((c) => c.id === item.id);
      if (chain) {
        setRunModalVariant('chain');
        setPreSelectedItem({
          id: chain.id,
          name: chain.name,
          description: chain.description,
          badge: `${chain.element_count} elements`,
        });
        setShowRunModal(true);
      }
    }
  };

  const handleExportItem = (item: LibraryItem) => {
    let content: string;
    let filename: string;

    if (item.type === 'operation') {
      const op = operationDefs.find((o) => o.full_name === item.id);
      if (!op) return;

      const exportData = {
        item_type: 'operation',
        name: op.name,
        short_name: op.short_name,
        category: op.category,
        description: op.description,
        agent_info: op.agent_info,
        timeout: op.timeout,
        operation_prompt: op.operation_prompt,
        mode: op.mode,
        agent_iterations: op.agent_iterations,
        disabled: op.disabled,
        yolo_mode: op.yolo_mode,
        ...(op.model_ref && { model_ref: op.model_ref }),
      };
      content = JSON.stringify(exportData, null, 2);
      filename = `${op.category}_${op.short_name}.json`;
    } else {
      //
      // For chains, request full definition and export.
      //
      const chain = chains.find((c) => c.id === item.id);
      if (!chain) return;

      //
      // We need the full chain definition. Request it and wait for it to load.
      // For now, export the summary info.
      //
      const exportData = {
        item_type: 'chain',
        id: chain.id,
        name: chain.name,
        description: chain.description,
        category: chain.category,
        disabled: chain.disabled,
        timeout: chain.timeout,
        //
        // Note: Full elements/connections require fetching the full chain.
        //
      };
      content = JSON.stringify(exportData, null, 2);
      filename = `chain_${chain.name.toLowerCase().replace(/\s+/g, '_')}.json`;
    }

    //
    // Download the file.
    //
    const blob = new Blob([content], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  const handleRunFromModal = (itemId: string, nodeId: string, agentName: string) => {
    if (runModalVariant === 'operation') {
      send({
        type: 'semantic_op_run',
        node_id: nodeId,
        agent_short_name: agentName,
        operation_name: itemId,
      });
    } else {
      runChain(itemId, nodeId, agentName);
    }
    setShowRunModal(false);
    setPreSelectedItem(null);
  };

  const handleSaveOp = () => {
    if (!editDef) return;

    //
    // Build JSON content for the operation.
    //
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
    setIsEditing(true);
    send({ type: 'op_def_add', content: JSON.stringify(opData) });
  };

  const updateEditDef = (field: keyof OperationDefinitionInfo, value: string | number | boolean | string[]) => {
    if (!editDef) return;
    setEditDef({ ...editDef, [field]: value });
  };

  const handleSaveChain = (definition: ChainDefinitionInput) => {
    if (editingChainId) {
      updateChain(editingChainId, definition);
    } else {
      createChain(definition);
    }
    setShowChainBuilder(false);
    setEditingChainId(null);
  };

  const handleCancelChain = () => {
    setShowChainBuilder(false);
    setEditingChainId(null);
  };

  //
  // If chain builder is open, show it full screen.
  //
  if (showChainBuilder) {
    return (
      <div className="h-[calc(100vh-280px)] min-h-[300px] border border-subtle ascii-box">
        <ChainBuilder
          chain={editingChainId ? currentChain : null}
          onSave={handleSaveChain}
          onCancel={handleCancelChain}
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
      {(chainError || opDefError) && (
        <div className="ascii-box bg-[var(--accent-error)]/20 border-[var(--accent-error)] p-3 text-sm">
          {chainError || opDefError}
        </div>
      )}
      {(chainSuccess || opDefSuccess) && (
        <div className="ascii-box bg-[var(--accent-success)]/20 border-[var(--accent-success)] p-3 text-sm">
          {chainSuccess || opDefSuccess}
        </div>
      )}

      {/*
      //
      // Toolbar: Filter, Search, Add.
      //
      */}
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          {/*
          //
          // Type filter.
          //
          */}
          <div className="flex gap-1">
            {[
              { value: 'all', label: 'All' },
              { value: 'operation', label: 'Operations' },
              { value: 'chain', label: 'Chains' },
            ].map((f) => (
              <button
                key={f.value}
                onClick={() => setFilter(f.value as FilterType)}
                className={`px-3 py-1.5 text-sm transition-colors ${
                  filter === f.value
                    ? 'bg-[var(--accent-info)]/20 text-[var(--accent-info)] border border-[var(--accent-info)]/50'
                    : 'text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
                }`}
              >
                {f.label}
              </button>
            ))}
          </div>

          {/*
          //
          // Search.
          //
          */}
          <div className="relative">
            <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
            <input
              type="text"
              placeholder="Search..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-9 pr-3 py-1.5 text-sm bg-[var(--bg-secondary)] border border-subtle focus:outline-none focus:border-[var(--border-active)] w-48"
            />
          </div>
        </div>

        {/*
        //
        // Add button with dropdown.
        //
        */}
        <div className="relative" ref={addMenuRef}>
          <button
            onClick={() => setShowAddMenu(!showAddMenu)}
            className="inline-flex items-center gap-2 px-3 py-1.5 text-sm bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors"
          >
            <Plus size={14} />
            Add
            <ChevronDown size={14} />
          </button>

          {showAddMenu && (
            <div className="absolute right-0 mt-1 w-48 bg-[var(--bg-secondary)] border border-subtle shadow-lg z-50">
              <button
                onClick={handleAddOperation}
                className="flex items-center gap-2 w-full px-3 py-2 text-sm text-left hover:bg-[var(--bg-tertiary)] transition-colors"
              >
                <Zap size={14} />
                New Operation
              </button>
              <button
                onClick={handleAddChain}
                className="flex items-center gap-2 w-full px-3 py-2 text-sm text-left hover:bg-[var(--bg-tertiary)] transition-colors"
              >
                <GitBranch size={14} />
                New Chain
              </button>
              <div className="border-t border-subtle" />
              <button
                onClick={handleImport}
                className="flex items-center gap-2 w-full px-3 py-2 text-sm text-left hover:bg-[var(--bg-tertiary)] transition-colors"
              >
                <Upload size={14} />
                Import JSON
              </button>
            </div>
          )}
        </div>
      </div>

      {/*
      //
      // Library table.
      //
      */}
      {filteredItems.length === 0 ? (
        <div className="text-center text-muted py-8 border border-subtle ascii-box">
          {searchQuery
            ? 'No items match your search.'
            : filter === 'all'
            ? 'No operations or chains defined. Click "Add" to create one.'
            : filter === 'operation'
            ? 'No operations defined.'
            : 'No chains defined.'}
        </div>
      ) : (
        <div className="border border-subtle ascii-box">
          <table className="w-full text-xs">
            <thead>
              <tr className="border-b border-subtle bg-[var(--bg-tertiary)]">
                <th className="text-left px-4 py-2 text-muted tracking-wider w-8">TYPE</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">NAME</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">CATEGORY</th>
                <th className="text-left px-4 py-2 text-muted tracking-wider">DETAILS</th>
                <th className="px-4 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {filteredItems.map((item) => (
                <tr
                  key={`${item.type}-${item.id}`}
                  className="border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors cursor-pointer"
                  onClick={() => handleEditItem(item)}
                >
                  <td className="px-4 py-3">
                    <span title={item.type === 'operation' ? 'Operation' : 'Chain'}>
                      {item.type === 'operation' ? (
                        <Zap size={14} className="text-[var(--accent-purple)]" />
                      ) : (
                        <GitBranch size={14} className="text-[var(--accent-info)]" />
                      )}
                    </span>
                  </td>
                  <td className="px-4 py-3">
                    <div className={item.disabled ? 'opacity-50' : ''}>
                      <p className="font-medium text-highlight flex items-center gap-2">
                        {item.name}
                        {item.disabled && (
                          <span className="px-1.5 py-0.5 bg-[var(--bg-tertiary)] text-muted text-xs">
                            Disabled
                          </span>
                        )}
                      </p>
                      {item.description && (
                        <p className="text-muted text-xs truncate max-w-md">{item.description}</p>
                      )}
                    </div>
                  </td>
                  <td className={`px-4 py-3 ${item.disabled ? 'opacity-50' : ''}`}>
                    {item.category}
                  </td>
                  <td className={`px-4 py-3 text-muted ${item.disabled ? 'opacity-50' : ''}`}>
                    {item.type === 'operation' ? (
                      <span>{item.mode} | {item.timeout}s</span>
                    ) : (
                      <span>{item.elementCount} elements | {item.operationCount} ops</span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
                      <button
                        onClick={() => handleRunItem(item)}
                        disabled={item.disabled}
                        className={`p-2 transition-colors ${
                          item.disabled
                            ? 'opacity-30 cursor-not-allowed text-muted'
                            : 'hover:bg-green-500/10 text-muted hover:text-[var(--accent-success)]'
                        }`}
                        title="Run"
                      >
                        <Play size={14} />
                      </button>
                      <button
                        onClick={() => handleEditItem(item)}
                        className="p-2 hover:bg-blue-500/10 text-muted hover:text-[var(--accent-info)] transition-colors"
                        title="Edit"
                      >
                        <Edit2 size={14} />
                      </button>
                      <button
                        onClick={() => handleExportItem(item)}
                        className="p-2 hover:bg-purple-500/10 text-muted hover:text-[var(--accent-purple)] transition-colors"
                        title="Export JSON"
                      >
                        <Download size={14} />
                      </button>
                      <button
                        onClick={() => handleDeleteClick(item)}
                        className="p-2 hover:bg-red-500/10 text-muted hover:text-[var(--accent-error)] transition-colors"
                        title="Delete"
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
      // Run Modal.
      //
      */}
      <RunModal
        isOpen={showRunModal}
        onClose={() => {
          setShowRunModal(false);
          setPreSelectedItem(null);
        }}
        onRun={handleRunFromModal}
        title={runModalVariant === 'operation' ? 'Run Operation' : 'Run Chain'}
        items={
          runModalVariant === 'operation'
            ? operationDefs.filter((d) => !d.disabled).map((def) => ({
                id: def.full_name,
                name: def.name,
                description: def.description,
                badge: def.category,
              }))
            : chains.filter((c) => !c.disabled).map((chain) => ({
                id: chain.id,
                name: chain.name,
                description: chain.description,
                badge: `${chain.element_count} elements`,
              }))
        }
        nodes={nodes}
        variant={runModalVariant}
        preSelectedItem={preSelectedItem}
      />

      {/*
      //
      // Delete Confirmation Modal.
      //
      */}
      <Modal
        isOpen={showDeleteModal}
        title={`Delete ${itemToDelete?.type === 'operation' ? 'Operation' : 'Chain'}`}
        onClose={() => {
          setShowDeleteModal(false);
          setItemToDelete(null);
        }}
      >
        <div className="space-y-4">
          <p className="text-sm">
            Are you sure you want to delete{' '}
            <span className="font-medium text-[var(--accent-error)]">"{itemToDelete?.name}"</span>?
          </p>
          <p className="text-xs text-muted">This action cannot be undone.</p>

          <div className="flex justify-end gap-3 pt-2">
            <button
              onClick={() => {
                setShowDeleteModal(false);
                setItemToDelete(null);
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

      {/*
      //
      // Edit Operation Modal.
      //
      */}
      <Modal
        isOpen={showEditOpModal}
        onClose={() => {
          setShowEditOpModal(false);
          setEditDef(null);
          setIsEditing(false);
          setIsNewOperation(false);
          clearOpDefStatus();
        }}
        title={isNewOperation ? 'New Operation' : `Edit: ${editDef?.name ?? 'Operation'}`}
        size="xl"
      >
        {editDef && (
          <div className="space-y-4">
            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="block text-sm font-medium mb-1">Name</label>
                <input
                  type="text"
                  value={editDef.name}
                  onChange={(e) => updateEditDef('name', e.target.value)}
                  disabled={isEditing}
                  className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
                />
              </div>
              <div>
                <label className="block text-sm font-medium mb-1">Short Name</label>
                <input
                  type="text"
                  value={editDef.short_name}
                  onChange={(e) => updateEditDef('short_name', e.target.value)}
                  disabled={!isNewOperation || isEditing}
                  className={`w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)] ${
                    !isNewOperation ? 'opacity-50 cursor-not-allowed' : ''
                  } disabled:opacity-50`}
                />
                {!isNewOperation && <p className="text-xs text-muted mt-1">Cannot be changed</p>}
              </div>
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="block text-sm font-medium mb-1">Category</label>
                <input
                  type="text"
                  value={editDef.category}
                  onChange={(e) => updateEditDef('category', e.target.value)}
                  disabled={!isNewOperation || isEditing}
                  className={`w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)] ${
                    !isNewOperation ? 'opacity-50 cursor-not-allowed' : ''
                  } disabled:opacity-50`}
                />
                {!isNewOperation && <p className="text-xs text-muted mt-1">Cannot be changed</p>}
              </div>
              <div>
                <label className="block text-sm font-medium mb-1">Mode</label>
                <select
                  value={editDef.mode}
                  onChange={(e) => updateEditDef('mode', e.target.value)}
                  disabled={isEditing}
                  className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
                >
                  <option value="one-shot">one-shot</option>
                  <option value="agent">agent</option>
                </select>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="block text-sm font-medium mb-1">Timeout (seconds)</label>
                <input
                  type="number"
                  value={editDef.timeout}
                  onChange={(e) => updateEditDef('timeout', parseInt(e.target.value) || 60)}
                  disabled={isEditing}
                  className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
                />
              </div>
              <div>
                <label className="block text-sm font-medium mb-1">Agent Iterations</label>
                <input
                  type="number"
                  value={editDef.agent_iterations}
                  onChange={(e) => updateEditDef('agent_iterations', parseInt(e.target.value) || 5)}
                  disabled={isEditing}
                  className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
                />
              </div>
            </div>

            <div>
              <label className="block text-sm font-medium mb-1">Description</label>
              <input
                type="text"
                value={editDef.description}
                onChange={(e) => updateEditDef('description', e.target.value)}
                disabled={isEditing}
                className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
              />
            </div>

            <div>
              <label className="block text-sm font-medium mb-1">Agent Info</label>
              <textarea
                value={editDef.agent_info}
                onChange={(e) => updateEditDef('agent_info', e.target.value)}
                disabled={isEditing}
                rows={3}
                className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm font-mono focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
              />
            </div>

            <div>
              <label className="block text-sm font-medium mb-1">Operation Prompt</label>
              <textarea
                value={editDef.operation_prompt}
                onChange={(e) => updateEditDef('operation_prompt', e.target.value)}
                disabled={isEditing}
                rows={6}
                className="w-full bg-[var(--bg-secondary)] border border-subtle px-3 py-2 text-sm font-mono focus:outline-none focus:border-[var(--border-active)] disabled:opacity-50"
              />
            </div>

            <div className="flex items-center gap-6">
              <button
                onClick={() => updateEditDef('yolo_mode', !editDef.yolo_mode)}
                disabled={isEditing}
                className="flex items-center gap-2 disabled:opacity-50"
                type="button"
              >
                {editDef.yolo_mode ? (
                  <ToggleRight size={24} className="text-[var(--accent-warning)]" />
                ) : (
                  <ToggleLeft size={24} className="text-muted" />
                )}
                <span className={`text-sm ${editDef.yolo_mode ? 'text-[var(--accent-warning)] font-medium' : 'text-muted'}`}>
                  YOLO Mode
                </span>
              </button>

              <button
                onClick={() => updateEditDef('disabled', !editDef.disabled)}
                disabled={isEditing}
                className="flex items-center gap-2 disabled:opacity-50"
                type="button"
              >
                {editDef.disabled ? (
                  <ToggleRight size={24} className="text-[var(--accent-error)]" />
                ) : (
                  <ToggleLeft size={24} className="text-muted" />
                )}
                <span className={`text-sm ${editDef.disabled ? 'text-[var(--accent-error)] font-medium' : 'text-muted'}`}>
                  Disabled
                </span>
              </button>
            </div>

            {opDefError && (
              <div className="p-3 bg-[var(--accent-error)]/10 text-[var(--accent-error)] text-sm">
                {opDefError}
              </div>
            )}

            <div className="flex justify-end gap-3 pt-2">
              <button
                onClick={() => {
                  setShowEditOpModal(false);
                  setEditDef(null);
                  setIsEditing(false);
                  setIsNewOperation(false);
                  clearOpDefStatus();
                }}
                disabled={isEditing}
                className="px-4 py-2 text-sm border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={handleSaveOp}
                disabled={isEditing || (isNewOperation && (!editDef?.short_name || !editDef?.category))}
                className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors disabled:opacity-50"
              >
                {isEditing && <Loader2 size={16} className="animate-spin" />}
                <Save size={16} />
                {isEditing ? 'Saving...' : isNewOperation ? 'Create' : 'Save'}
              </button>
            </div>
          </div>
        )}
      </Modal>

      {/*
      //
      // Import Modal.
      //
      */}
      <ImportModal
        isOpen={showImportModal}
        onClose={() => setShowImportModal(false)}
      />
    </div>
  );
}
