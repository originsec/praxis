import { useState, useEffect } from 'react';
import {
  Loader2,
  Wrench,
  RefreshCw,
  ChevronRight,
  ChevronDown,
  FolderOpen,
} from 'lucide-react';
import { Modal } from '../common/Modal';
import { useApp } from '../../context/AppContext';
import type { ReconResult } from '../../api/types';

interface ReconModalProps {
  nodeId: string;
  agentShortName: string;
  onClose: () => void;
}

export function ReconModal({ nodeId, agentShortName, onClose }: ReconModalProps) {
  const { send, sendCommand } = useApp();
  const [reconResult, setReconResult] = useState<ReconResult | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [activeTab, setActiveTab] = useState<'tools' | 'config'>('tools');
  const [selectedServer, setSelectedServer] = useState<number | null>(null);

  //
  // Fetch recon from service, trigger node recon if needed.
  //
  useEffect(() => {
    let cancelled = false;
    let pollInterval: ReturnType<typeof setInterval> | null = null;
    let reconTriggered = false;

    const requestRecon = () => {
      send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
    };

    const handleWsMessage = (event: Event) => {
      if (cancelled) return;
      const message = (event as CustomEvent).detail;
      if (message.type === 'recon_get_response' &&
          message.node_id === nodeId &&
          message.agent_short_name === agentShortName) {
        if (message.recon_result) {
          setReconResult(message.recon_result);
          setIsLoading(false);
          if (pollInterval) clearInterval(pollInterval);
          window.removeEventListener('ws-message', handleWsMessage);
        } else if (!reconTriggered) {
          reconTriggered = true;
          sendCommand(nodeId, { Agent: 'Recon' }).catch(() => {});
          pollInterval = setInterval(() => {
            if (!cancelled) requestRecon();
          }, 1000);
        }
      }
    };

    window.addEventListener('ws-message', handleWsMessage);
    requestRecon();

    return () => {
      cancelled = true;
      window.removeEventListener('ws-message', handleWsMessage);
      if (pollInterval) clearInterval(pollInterval);
    };
  }, [nodeId, agentShortName, send, sendCommand]);

  const handleRefresh = () => {
    setIsLoading(true);
    setReconResult(null);
    sendCommand(nodeId, { Agent: 'Recon' }).catch(() => {});
    setTimeout(() => {
      send({ type: 'recon_get', node_id: nodeId, agent_short_name: agentShortName });
    }, 500);
  };

  const tabs = [
    { id: 'tools' as const, label: 'Tools' },
    { id: 'config' as const, label: 'Config' },
  ];

  return (
    <Modal
      isOpen={true}
      onClose={onClose}
      title={`Recon: ${agentShortName}`}
      size="xl"
      headerActions={
        <button
          onClick={handleRefresh}
          disabled={isLoading}
          className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors disabled:opacity-50"
          title="Refresh recon"
        >
          <RefreshCw size={14} className={isLoading ? 'animate-spin' : ''} />
        </button>
      }
    >
      {isLoading ? (
        <div className="flex items-center justify-center py-12">
          <Loader2 size={24} className="animate-spin text-muted" />
          <span className="ml-3 text-muted text-sm">Loading recon data...</span>
        </div>
      ) : reconResult ? (
        <div className="space-y-4">
          {/*
          //
          // Tabs.
          //
          */}
          <div className="flex gap-1 border-b border-subtle">
            {tabs.map(tab => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`px-3 py-2 text-xs font-medium border-b-2 transition-colors ${
                  activeTab === tab.id
                    ? 'border-[var(--accent-info)] text-title'
                    : 'border-transparent text-muted hover:text-[var(--text-primary)]'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          {activeTab === 'tools' && (
            <div className="space-y-3">
              {/*
              //
              // MCP Servers.
              //
              */}
              {reconResult.tools.mcp_servers.length > 0 && (
                <div>
                  <h3 className="text-xs font-medium text-muted tracking-wider mb-2">MCP SERVERS ({reconResult.tools.mcp_servers.length})</h3>
                  <div className="space-y-1">
                    {reconResult.tools.mcp_servers.map((server, idx) => (
                      <div key={idx}>
                        <button
                          onClick={() => setSelectedServer(selectedServer === idx ? null : idx)}
                          className="w-full flex items-center gap-2 px-3 py-2 bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] transition-colors text-left"
                        >
                          {selectedServer === idx ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                          <Wrench size={12} className="text-[var(--accent-info)]" />
                          <span className="text-xs text-highlight font-medium">{server.name}</span>
                          <span className="text-[10px] text-muted ml-auto">{server.tools.length} tools</span>
                          <span className="text-[10px] text-muted">{server.transport}</span>
                        </button>
                        {selectedServer === idx && (
                          <div className="ml-6 mt-1 space-y-1">
                            {server.tools.map((tool, tidx) => (
                              <div key={tidx} className="px-3 py-1.5 bg-[var(--bg-primary)] border border-subtle text-xs">
                                <span className="text-highlight font-mono">{tool.name}</span>
                                {tool.description && (
                                  <p className="text-muted text-[10px] mt-0.5">{tool.description}</p>
                                )}
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/*
              //
              // Skills.
              //
              */}
              {reconResult.tools.skills.length > 0 && (
                <div>
                  <h3 className="text-xs font-medium text-muted tracking-wider mb-2">SKILLS ({reconResult.tools.skills.length})</h3>
                  <div className="grid grid-cols-2 gap-1">
                    {reconResult.tools.skills.map((skill, idx) => (
                      <div key={idx} className="px-3 py-2 bg-[var(--bg-secondary)] text-xs">
                        <span className="text-highlight font-mono">{skill.name}</span>
                        {skill.description && (
                          <p className="text-muted text-[10px] mt-0.5 line-clamp-2">{skill.description}</p>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/*
              //
              // Internal tools.
              //
              */}
              {reconResult.tools.internal_tools.length > 0 && (
                <div>
                  <h3 className="text-xs font-medium text-muted tracking-wider mb-2">INTERNAL TOOLS ({reconResult.tools.internal_tools.length})</h3>
                  <div className="grid grid-cols-2 gap-1">
                    {reconResult.tools.internal_tools.map((tool, idx) => (
                      <div key={idx} className="px-3 py-2 bg-[var(--bg-secondary)] text-xs">
                        <span className="text-highlight font-mono">{tool.name}</span>
                        {tool.description && (
                          <p className="text-muted text-[10px] mt-0.5 line-clamp-2">{tool.description}</p>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {reconResult.tools.mcp_servers.length === 0 &&
               reconResult.tools.skills.length === 0 &&
               reconResult.tools.internal_tools.length === 0 && (
                <div className="text-center py-8 text-muted text-sm">No tools discovered</div>
              )}
            </div>
          )}

          {activeTab === 'config' && (
            <div className="space-y-2">
              {reconResult.config.length === 0 ? (
                <div className="text-center py-8 text-muted text-sm">No config files discovered</div>
              ) : (
                reconResult.config.map((item, idx) => (
                  <div key={idx} className="px-3 py-2 bg-[var(--bg-secondary)] border border-subtle">
                    <div className="flex items-center gap-2">
                      <FolderOpen size={12} className="text-muted" />
                      <span className="text-xs text-highlight font-mono truncate">{item.path}</span>
                      <span className="text-[10px] text-muted ml-auto">{item.config_type}</span>
                    </div>
                  </div>
                ))
              )}

              {/*
              //
              // Metadata.
              //
              */}
              {reconResult.metadata && (
                <div className="mt-4 space-y-2">
                  {reconResult.metadata.user_identities && reconResult.metadata.user_identities.length > 0 && (
                    <div>
                      <h3 className="text-xs font-medium text-muted tracking-wider mb-1">IDENTITIES</h3>
                      <div className="flex flex-wrap gap-1">
                        {reconResult.metadata.user_identities.map((id, idx) => (
                          <span key={idx} className="px-2 py-0.5 bg-[var(--bg-secondary)] text-[10px] text-highlight font-mono">{id}</span>
                        ))}
                      </div>
                    </div>
                  )}
                  {reconResult.metadata.api_keys && reconResult.metadata.api_keys.length > 0 && (
                    <div>
                      <h3 className="text-xs font-medium text-muted tracking-wider mb-1">API KEYS</h3>
                      <div className="flex flex-wrap gap-1">
                        {reconResult.metadata.api_keys.map((key, idx) => (
                          <span key={idx} className="px-2 py-0.5 bg-[var(--accent-warning)]/10 text-[10px] text-[var(--accent-warning)] font-mono">{key}</span>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      ) : (
        <div className="text-center py-12 text-muted text-sm">Failed to load recon data</div>
      )}
    </Modal>
  );
}
