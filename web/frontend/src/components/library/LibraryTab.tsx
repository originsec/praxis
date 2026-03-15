import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { Play, Trash2, Edit2, Zap, GitBranch, Download, Upload, Search, Plus, ChevronDown, Loader2, Circle, CircleCheck, Save } from 'lucide-react';
import { useApp } from '../../context/AppContext';
import { ChainBuilder } from '../chains/ChainBuilder';
import { Modal } from '../common/Modal';
import { RunModal } from '../common/RunModal';
import { DataTable, type ColumnDef, type RowAction } from '../common/DataTable';
import { ImportModal } from './ImportModal';
import type { LibraryItem, LibraryItemType, OperationDefinitionInfo, ChainDefinitionInput, NodeState, TargetSpec } from '../../api/types';

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
    runOperation,
    clearChainStatus,
    clearLastCreatedChain,
    clearOpDefStatus,
    getConfig,
  } = useApp();

  const { chains, currentChain, chainError, chainSuccess, lastCreatedChainId } = state.chains;
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

  //
  // Chain export state - track which chain is being exported.
  //
  const [exportingChainId, setExportingChainId] = useState<string | null>(null);
  const pendingSaveCallback = useRef<((result: 'saved' | 'error') => void) | null>(null);

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
      send({ type: 'toolkit_list' });
      send({ type: 'payload_list' });
    }
  }, [showChainBuilder, send, getConfig]);

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

  useEffect(() => {
    if (lastCreatedChainId && showChainBuilder && !editingChainId) {
      setEditingChainId(lastCreatedChainId);
      clearLastCreatedChain();
    }
  }, [lastCreatedChainId, showChainBuilder, editingChainId, clearLastCreatedChain]);

  //
  // Export chain once full definition is loaded.
  //
  useEffect(() => {
    if (exportingChainId && currentChain && currentChain.id === exportingChainId) {
      const exportData = {
        item_type: 'chain',
        name: currentChain.name,
        description: currentChain.description,
        category: currentChain.category,
        elements: currentChain.elements,
        connections: currentChain.connections,
        disabled: currentChain.disabled,
        timeout: currentChain.timeout,
        positions: currentChain.positions,
      };
      const content = JSON.stringify(exportData, null, 2);
      const filename = `chain_${currentChain.name.toLowerCase().replace(/\s+/g, '_')}.json`;

      const blob = new Blob([content], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);

      setExportingChainId(null);
    }
  }, [exportingChainId, currentChain]);

  //
  // Handle success/error for chains.
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
      const content = JSON.stringify(exportData, null, 2);
      const filename = `${op.category}_${op.short_name}.json`;

      const blob = new Blob([content], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } else {
      //
      // For chains, request full definition and export via useEffect.
      //
      setExportingChainId(item.id);
      requestChain(item.id);
    }
  };

  const handleRunFromModal = (itemId: string, targetSpec: TargetSpec) => {
    const allNodes = nodes;
    const filteredNodes = targetSpec.node_ids.length > 0
      ? allNodes.filter(n => targetSpec.node_ids.includes(n.node_id))
      : targetSpec.os_filter
        ? allNodes.filter(n => n.os_details.toLowerCase().includes(targetSpec.os_filter!.toLowerCase()))
        : allNodes;

    if (runModalVariant === 'operation') {
      for (const node of filteredNodes) {
        const agents = targetSpec.agent_short_names.length > 0
          ? node.discovered_agents.filter(a => targetSpec.agent_short_names.includes(a.short_name))
          : node.discovered_agents;
        for (const agent of agents) {
          runOperation(node.node_id, agent.short_name, itemId);
        }
      }
    } else {
      const primaryNode = filteredNodes[0];
      if (!primaryNode) return;
      const agentName = targetSpec.agent_short_names.length > 0
        ? targetSpec.agent_short_names[0]
        : primaryNode.discovered_agents?.[0]?.short_name || '';
      runChain(itemId, primaryNode.node_id, agentName, undefined, targetSpec);
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

  const handleSaveChain = (definition: ChainDefinitionInput, onResult?: (result: 'saved' | 'error') => void) => {
    if (onResult) pendingSaveCallback.current = onResult;
    clearChainStatus();

    if (editingChainId) {
      updateChain(editingChainId, definition);
    } else {
      createChain(definition);
    }
  };

  const handleDuplicateChain = (definition: ChainDefinitionInput) => {
    createChain(definition);
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
  //
  // Dynamic height for chain builder: measure container top offset and fill
  // to bottom of viewport.
  //
  const chainBuilderRef = useRef<HTMLDivElement>(null);
  const [chainBuilderHeight, setChainBuilderHeight] = useState<number | null>(null);

  const updateChainBuilderHeight = useCallback(() => {
    if (chainBuilderRef.current) {
      const top = chainBuilderRef.current.getBoundingClientRect().top;
      setChainBuilderHeight(window.innerHeight - top - 16);
    }
  }, []);

  useEffect(() => {
    if (showChainBuilder) {
      updateChainBuilderHeight();
      window.addEventListener('resize', updateChainBuilderHeight);
      return () => window.removeEventListener('resize', updateChainBuilderHeight);
    }
  }, [showChainBuilder, updateChainBuilderHeight]);

  const libraryColumns: ColumnDef<LibraryItem>[] = [
    {
      key: 'name',
      header: 'Name',
      sortable: false,
      render: (_: unknown, item: LibraryItem) => (
        <div className={`flex items-start gap-3 ${item.disabled ? 'opacity-50' : ''}`}>
          <span className="flex-shrink-0 mt-0.5" title={item.type === 'operation' ? 'Operation' : 'Chain'}>
            {item.type === 'operation'
              ? <Zap size={14} className="text-[var(--accent-purple)]" />
              : <GitBranch size={14} className="text-[var(--accent-info)]" />}
          </span>
          <div>
            <p className="font-medium text-highlight flex items-center gap-2">
              {item.name}
              {item.disabled && (
                <span className="px-1.5 py-0.5 bg-[var(--bg-tertiary)] text-muted text-xs">Disabled</span>
              )}
            </p>
            {item.description && (
              <p className="text-muted text-xs truncate max-w-md">{item.description}</p>
            )}
          </div>
        </div>
      ),
    },
    {
      key: 'category',
      header: 'Category',
      sortable: false,
      render: (_: unknown, item: LibraryItem) => (
        <span className={item.disabled ? 'opacity-50' : ''}>{item.category}</span>
      ),
    },
    {
      key: 'details',
      header: 'Details',
      sortable: false,
      render: (_: unknown, item: LibraryItem) => (
        <span className={`text-muted ${item.disabled ? 'opacity-50' : ''}`}>
          {item.type === 'operation'
            ? `${item.mode} | ${item.timeout}s`
            : `${item.elementCount} elements | ${item.operationCount} ops`}
        </span>
      ),
    },
  ];

  const libraryActions: RowAction<LibraryItem>[] = [
    {
      icon: <Play size={14} />,
      label: 'Run',
      onClick: (item) => handleRunItem(item),
      disabled: (item) => !!item.disabled,
      hoverColor: 'var(--accent-success)',
    },
    {
      icon: <Edit2 size={14} />,
      label: 'Edit',
      onClick: (item) => handleEditItem(item),
      hoverColor: 'var(--accent-info)',
    },
    {
      icon: <Download size={14} />,
      label: 'Export JSON',
      onClick: (item) => handleExportItem(item),
      hoverColor: 'var(--accent-purple)',
    },
    {
      icon: <Trash2 size={14} />,
      label: 'Delete',
      onClick: (item) => handleDeleteClick(item),
      hoverColor: 'var(--accent-error)',
    },
  ];

  if (showChainBuilder) {
    return (
      <div
        ref={chainBuilderRef}
        className="border border-subtle"
        style={{ height: chainBuilderHeight ? `${chainBuilderHeight}px` : 'calc(100vh - 200px)', minHeight: 400 }}
      >
        <ChainBuilder
          chain={editingChainId ? currentChain : null}
          onSave={handleSaveChain}
          onDuplicate={handleDuplicateChain}
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
          operationDefs={operationDefs}
          modelDefs={modelDefs}
          toolkitTools={state.toolkit.tools}
          payloads={state.payloads}
          send={send}
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
      <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-3">
        <div className="flex items-center gap-2 md:gap-3 min-w-0 flex-wrap">
          {/*
          //
          // Type filter.
          //
          */}
          <div className="flex gap-1 overflow-x-auto">
            {[
              { value: 'all', label: 'All' },
              { value: 'operation', label: 'Operations' },
              { value: 'chain', label: 'Chains' },
            ].map((f) => (
              <button
                key={f.value}
                onClick={() => setFilter(f.value as FilterType)}
                className={`px-2.5 md:px-3 py-1.5 text-xs md:text-sm whitespace-nowrap transition-colors ${
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
          <div className="relative min-w-0 flex-1 md:flex-none">
            <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
            <input
              type="text"
              placeholder="Search..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-9 pr-3 py-1.5 text-sm bg-[var(--bg-secondary)] border border-subtle focus:outline-none focus:border-[var(--border-active)] w-full md:w-48"
            />
          </div>
        </div>

        {/*
        //
        // Add button with dropdown.
        //
        */}
        <div className="relative self-start md:self-auto" ref={addMenuRef}>
          <button
            onClick={() => setShowAddMenu(!showAddMenu)}
            className="inline-flex items-center gap-1.5 px-2.5 md:px-3 py-1.5 text-xs tracking-wider bg-[var(--accent-success)]/20 text-[var(--accent-success)] border border-dim hover:border-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors"
          >
            <Plus size={14} />
            Add
            <ChevronDown size={14} />
          </button>

          {showAddMenu && (
            <div className="absolute left-0 md:left-auto md:right-0 mt-1 w-52 max-w-[calc(100vw-2rem)] bg-[var(--bg-secondary)] border border-dim z-50">
              <button
                onClick={handleAddOperation}
                className="flex items-center gap-2 w-full px-3 py-2.5 text-xs tracking-wider text-highlight border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors text-left"
              >
                <Zap size={14} className="text-[var(--accent-purple)]" />
                New Operation
              </button>
              <button
                onClick={handleAddChain}
                className="flex items-center gap-2 w-full px-3 py-2.5 text-xs tracking-wider text-highlight border-b border-dim last:border-0 hover:bg-[var(--highlight)] transition-colors text-left"
              >
                <GitBranch size={14} className="text-[var(--accent-info)]" />
                New Chain
              </button>
              <div className="border-t border-dim" />
              <button
                onClick={handleImport}
                className="flex items-center gap-2 w-full px-3 py-2.5 text-xs tracking-wider text-highlight hover:bg-[var(--highlight)] transition-colors text-left"
              >
                <Upload size={14} className="text-muted" />
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
      <div className="border border-subtle ascii-box overflow-x-auto">
        <DataTable
          data={filteredItems}
          columns={libraryColumns}
          getRowKey={item => `${item.type}-${item.id}`}
          actions={libraryActions}
          pinnedActions
          onRowClick={(item) => handleEditItem(item)}
          emptyMessage={
            searchQuery
              ? 'No items match your search.'
              : filter === 'all'
              ? 'No operations or chains defined. Click "Add" to create one.'
              : filter === 'operation'
              ? 'No operations defined.'
              : 'No chains defined.'
          }
        />
      </div>

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
            ? operationDefs.filter((d) => !d.disabled).sort((a, b) => (a.category || '').localeCompare(b.category || '') || a.name.localeCompare(b.name)).map((def) => ({
                id: def.full_name,
                name: def.name,
                description: def.description,
                badge: def.category,
              }))
            : chains.filter((c) => !c.disabled).sort((a, b) => a.name.localeCompare(b.name)).map((chain) => ({
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
          <div className="space-y-0">
            {/*
            //
            // Basic Information.
            //
            */}
            <div className="space-y-3 p-4 bg-[var(--bg-secondary)]">
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">
                    Name {isNewOperation && <span className="text-[var(--accent-error)]/70">*</span>}
                  </label>
                  <input
                    type="text"
                    value={editDef.name}
                    onChange={(e) => updateEditDef('name', e.target.value)}
                    disabled={isEditing}
                    className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
                    placeholder="Display name for operation"
                  />
                </div>
                <div>
                  <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">
                    Short Name {isNewOperation && <span className="text-[var(--accent-error)]/70">*</span>}
                  </label>
                  <input
                    type="text"
                    value={editDef.short_name}
                    onChange={(e) => updateEditDef('short_name', e.target.value)}
                    disabled={!isNewOperation || isEditing}
                    className={`w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle ${
                      !isNewOperation ? 'opacity-50 cursor-not-allowed' : ''
                    } disabled:opacity-50 transition-colors`}
                    placeholder="unique_identifier"
                  />
                  {!isNewOperation && <p className="text-xs text-muted mt-1.5">Cannot be changed</p>}
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">
                    Category {isNewOperation && <span className="text-[var(--accent-error)]/70">*</span>}
                  </label>
                  <input
                    type="text"
                    value={editDef.category}
                    onChange={(e) => updateEditDef('category', e.target.value)}
                    disabled={!isNewOperation || isEditing}
                    className={`w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle ${
                      !isNewOperation ? 'opacity-50 cursor-not-allowed' : ''
                    } disabled:opacity-50 transition-colors`}
                    placeholder="recon, exfiltration, etc."
                  />
                  {!isNewOperation && <p className="text-xs text-muted mt-1.5">Cannot be changed</p>}
                </div>
                <div>
                  <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Mode</label>
                  <select
                    value={editDef.mode}
                    onChange={(e) => updateEditDef('mode', e.target.value)}
                    disabled={isEditing}
                    className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
                  >
                    <option value="one-shot">one-shot</option>
                    <option value="agent">agent</option>
                  </select>
                </div>
              </div>

              <div className={`grid ${editDef.mode === 'agent' ? 'grid-cols-2' : 'grid-cols-1'} gap-3`}>
                <div>
                  <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Timeout (seconds)</label>
                  <input
                    type="number"
                    value={editDef.timeout}
                    onChange={(e) => updateEditDef('timeout', parseInt(e.target.value) || 60)}
                    disabled={isEditing}
                    className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
                  />
                </div>
                {editDef.mode === 'agent' && (
                  <div>
                    <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Agent Iterations</label>
                    <input
                      type="number"
                      value={editDef.agent_iterations}
                      onChange={(e) => updateEditDef('agent_iterations', parseInt(e.target.value) || 5)}
                      disabled={isEditing}
                      className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
                    />
                  </div>
                )}
              </div>

              <div>
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Description</label>
                <input
                  type="text"
                  value={editDef.description}
                  onChange={(e) => updateEditDef('description', e.target.value)}
                  disabled={isEditing}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors"
                  placeholder="Brief description of what this operation does"
                />
              </div>
            </div>

            {/*
            //
            // Divider.
            //
            */}
            <div className="border-t border-dim"></div>

            {/*
            //
            // Prompt Configuration.
            //
            */}
            <div className="space-y-3 p-4 bg-[var(--bg-secondary)]">
              <div>
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">Agent Info</label>
                <p className="text-xs mb-2 leading-relaxed" style={{ color: 'var(--text-muted)' }}>
                  Optional. Technical context for AI agents to understand when and how to use this operation. Used by autonomous agents like Orchestrator for decision-making.
                </p>
                <textarea
                  value={editDef.agent_info}
                  onChange={(e) => updateEditDef('agent_info', e.target.value)}
                  disabled={isEditing}
                  rows={3}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm font-mono text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors resize-none"
                  placeholder="e.g., Searches for emails through communication channels, contact lists, and directory services. Useful for mapping organizational structure."
                />
              </div>

              <div>
                <label className="block text-xs tracking-wider text-[var(--text-secondary)] mb-1.5">
                  Operation Prompt <span className="text-[var(--accent-error)]/70">*</span>
                </label>
                <textarea
                  value={editDef.operation_prompt}
                  onChange={(e) => updateEditDef('operation_prompt', e.target.value)}
                  disabled={isEditing}
                  rows={6}
                  className="w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm font-mono text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors resize-none"
                  placeholder="The actual instructions given to the agent when executing this operation"
                />
              </div>
            </div>

            {/*
            //
            // Divider.
            //
            */}
            <div className="border-t border-dim"></div>

            {/*
            //
            // Toggles & Actions.
            //
            */}
            <div className="p-4 bg-[var(--bg-secondary)]">
              <div className="flex items-center gap-6 mb-4">
                <button
                  onClick={() => updateEditDef('yolo_mode', !editDef.yolo_mode)}
                  disabled={isEditing}
                  className="flex items-center gap-2 disabled:opacity-50 hover:opacity-80 transition-opacity"
                  type="button"
                >
                  {editDef.yolo_mode ? (
                    <CircleCheck size={16} className="text-[var(--accent-error)]" />
                  ) : (
                    <Circle size={16} className="text-[var(--text-secondary)]" />
                  )}
                  <span className={`text-xs tracking-wider ${editDef.yolo_mode ? 'text-[var(--accent-error)]' : 'text-[var(--text-secondary)]'}`}>
                    YOLO Mode
                  </span>
                </button>

                <button
                  onClick={() => updateEditDef('disabled', !editDef.disabled)}
                  disabled={isEditing}
                  className="flex items-center gap-2 disabled:opacity-50 hover:opacity-80 transition-opacity"
                  type="button"
                >
                  {editDef.disabled ? (
                    <CircleCheck size={16} className="text-[var(--accent-error)]" />
                  ) : (
                    <Circle size={16} className="text-[var(--text-secondary)]" />
                  )}
                  <span className={`text-xs tracking-wider ${editDef.disabled ? 'text-[var(--accent-error)]' : 'text-[var(--text-secondary)]'}`}>
                    Disabled
                  </span>
                </button>
              </div>

              {opDefError && (
                <div className="mb-4 p-3 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 text-[var(--accent-error)] text-xs">
                  {opDefError}
                </div>
              )}

              <div className="flex justify-end gap-2">
                {!isNewOperation && editDef && (
                  <button
                    onClick={() => {
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
                    }}
                    className="inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider text-muted border border-dim hover:border-[var(--accent-purple)] hover:text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/10 transition-colors"
                  >
                    <Download size={14} />
                    Export
                  </button>
                )}
                <button
                  onClick={() => {
                    setShowEditOpModal(false);
                    setEditDef(null);
                    setIsEditing(false);
                    setIsNewOperation(false);
                    clearOpDefStatus();
                  }}
                  disabled={isEditing}
                  className="px-4 py-2 text-xs tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors disabled:opacity-50"
                >
                  Cancel
                </button>
                <button
                  onClick={handleSaveOp}
                  disabled={isEditing || (isNewOperation && (!editDef?.short_name || !editDef?.category))}
                  className="inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] border border-dim hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20 transition-colors disabled:opacity-50"
                >
                  {isEditing && <Loader2 size={14} className="animate-spin" />}
                  <Save size={14} />
                  {isEditing ? 'Saving...' : isNewOperation ? 'Create' : 'Save'}
                </button>
              </div>
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
