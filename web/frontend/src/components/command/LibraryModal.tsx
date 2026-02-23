import { useState, useEffect } from 'react';
import { Zap, GitBranch, Play, Pencil, Trash2 } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { Modal } from '../common/Modal';
import { RunModal, type RunItem } from '../common/RunModal';
import { useApp } from '../../context/AppContext';

interface LibraryModalProps {
  onClose: () => void;
}

export function LibraryModal({ onClose }: LibraryModalProps) {
  const { state, send, requestChainDefList, runOperation, runChain, deleteChain } = useApp();
  const navigate = useNavigate();
  const [activeTab, setActiveTab] = useState<'operations' | 'chains'>('operations');

  //
  // Run modal state — pre-selects the item, user picks node/agent.
  //
  const [runModalItem, setRunModalItem] = useState<{ item: RunItem; variant: 'operation' | 'chain' } | null>(null);

  useEffect(() => {
    send({ type: 'op_def_list' });
    requestChainDefList();
  }, [send, requestChainDefList]);

  const ops = state.operationDefs;
  const chains = state.chains.chains;
  const nodes = state.systemState?.nodes ?? [];

  const handleRunOp = (opFullName: string, opName: string) => {
    setRunModalItem({
      item: { id: opFullName, name: opName },
      variant: 'operation',
    });
  };

  const handleRunChain = (chainId: string, chainName: string) => {
    setRunModalItem({
      item: { id: chainId, name: chainName },
      variant: 'chain',
    });
  };

  const handleEditChain = (chainId: string) => {
    onClose();
    navigate(`/operations?tab=chains&edit=${chainId}`);
  };

  const handleDeleteChain = (chainId: string, chainName: string) => {
    if (confirm(`Delete chain "${chainName}"?`)) {
      deleteChain(chainId);
    }
  };

  const handleEditOp = () => {
    onClose();
    navigate('/operations');
  };

  return (
    <>
      <Modal
        isOpen={true}
        onClose={onClose}
        title="Library"
        size="xl"
      >
        <div className="space-y-3">
          {/*
          //
          // Tabs.
          //
          */}
          <div className="flex gap-1 border-b border-subtle">
            <button
              onClick={() => setActiveTab('operations')}
              className={`flex items-center gap-2 px-3 py-2 text-xs font-medium border-b-2 transition-colors ${
                activeTab === 'operations'
                  ? 'border-[var(--accent-purple)] text-title'
                  : 'border-transparent text-muted hover:text-[var(--text-primary)]'
              }`}
            >
              <Zap size={14} /> Operations ({ops.length})
            </button>
            <button
              onClick={() => setActiveTab('chains')}
              className={`flex items-center gap-2 px-3 py-2 text-xs font-medium border-b-2 transition-colors ${
                activeTab === 'chains'
                  ? 'border-[var(--accent-info)] text-title'
                  : 'border-transparent text-muted hover:text-[var(--text-primary)]'
              }`}
            >
              <GitBranch size={14} /> Chains ({chains.length})
            </button>
          </div>

          {/*
          //
          // Operations table.
          //
          */}
          {activeTab === 'operations' && (
            <div className="max-h-[60vh] overflow-auto border border-subtle">
              {ops.length === 0 ? (
                <div className="text-center py-8 text-muted text-sm">No operations defined</div>
              ) : (
                <table className="w-full text-xs">
                  <thead className="sticky top-0 bg-[var(--bg-tertiary)]">
                    <tr>
                      <th className="px-3 py-2 text-left text-muted font-medium border-b border-subtle">Name</th>
                      <th className="px-3 py-2 text-left text-muted font-medium border-b border-subtle">Category</th>
                      <th className="px-3 py-2 text-left text-muted font-medium border-b border-subtle hidden md:table-cell">Description</th>
                      <th className="px-3 py-2 text-right text-muted font-medium border-b border-subtle w-20">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {ops.map(op => (
                      <tr key={op.full_name} className="hover:bg-[var(--highlight)] transition-colors group">
                        <td className="px-3 py-2 border-b border-subtle">
                          <div className="flex items-center gap-2">
                            <Zap size={11} className="text-[var(--accent-purple)] flex-shrink-0" />
                            <span className="text-highlight font-medium">{op.name}</span>
                          </div>
                        </td>
                        <td className="px-3 py-2 border-b border-subtle text-muted">{op.category || '—'}</td>
                        <td className="px-3 py-2 border-b border-subtle text-muted truncate max-w-[300px] hidden md:table-cell">{op.description || '—'}</td>
                        <td className="px-3 py-2 border-b border-subtle">
                          <div className="flex items-center gap-1 justify-end opacity-0 group-hover:opacity-100 transition-opacity">
                            <button
                              onClick={() => handleRunOp(op.full_name, op.name)}
                              className="p-1 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20 transition-colors"
                              title="Run"
                            >
                              <Play size={12} />
                            </button>
                            <button
                              onClick={handleEditOp}
                              className="p-1 text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] transition-colors"
                              title="Edit in Operations page"
                            >
                              <Pencil size={12} />
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          )}

          {/*
          //
          // Chains table.
          //
          */}
          {activeTab === 'chains' && (
            <div className="max-h-[60vh] overflow-auto border border-subtle">
              {chains.length === 0 ? (
                <div className="text-center py-8 text-muted text-sm">No chains defined</div>
              ) : (
                <table className="w-full text-xs">
                  <thead className="sticky top-0 bg-[var(--bg-tertiary)]">
                    <tr>
                      <th className="px-3 py-2 text-left text-muted font-medium border-b border-subtle">Name</th>
                      <th className="px-3 py-2 text-left text-muted font-medium border-b border-subtle">Steps</th>
                      <th className="px-3 py-2 text-left text-muted font-medium border-b border-subtle hidden md:table-cell">Description</th>
                      <th className="px-3 py-2 text-right text-muted font-medium border-b border-subtle w-24">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {chains.map(chain => (
                      <tr key={chain.id} className="hover:bg-[var(--highlight)] transition-colors group">
                        <td className="px-3 py-2 border-b border-subtle">
                          <div className="flex items-center gap-2">
                            <GitBranch size={11} className="text-[var(--accent-info)] flex-shrink-0" />
                            <span className="text-highlight font-medium">{chain.name}</span>
                          </div>
                        </td>
                        <td className="px-3 py-2 border-b border-subtle text-muted">{chain.element_count}</td>
                        <td className="px-3 py-2 border-b border-subtle text-muted truncate max-w-[300px] hidden md:table-cell">{chain.description || '—'}</td>
                        <td className="px-3 py-2 border-b border-subtle">
                          <div className="flex items-center gap-1 justify-end opacity-0 group-hover:opacity-100 transition-opacity">
                            <button
                              onClick={() => handleRunChain(chain.id, chain.name)}
                              className="p-1 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/20 transition-colors"
                              title="Run"
                            >
                              <Play size={12} />
                            </button>
                            <button
                              onClick={() => handleEditChain(chain.id)}
                              className="p-1 text-muted hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] transition-colors"
                              title="Edit chain"
                            >
                              <Pencil size={12} />
                            </button>
                            <button
                              onClick={() => handleDeleteChain(chain.id, chain.name)}
                              className="p-1 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/20 transition-colors"
                              title="Delete chain"
                            >
                              <Trash2 size={12} />
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          )}
        </div>
      </Modal>

      {/*
      //
      // Run modal — opens when user clicks play on an item.
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
          onRun={(itemId, nodeId, agentName) => {
            if (runModalItem.variant === 'operation') {
              runOperation(nodeId, agentName, itemId);
            } else {
              runChain(itemId, nodeId, agentName);
            }
            setRunModalItem(null);
          }}
        />
      )}
    </>
  );
}
