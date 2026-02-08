import { useState, useRef, useEffect, useCallback } from 'react';
import {
  MessageSquare,
  Users,
  Hash,
  Send,
  PlayCircle,
  StopCircle,
  Plus,
  GripVertical,
  AlertCircle,
  AlertTriangle,
  Loader2,
} from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useApp } from '../context/AppContext';
import type { AgentChatAgentInfo, AgentChatChannelInfo, AgentChatMessageInfo } from '../api/types';

//
// Color palette for agent nicknames (uses CSS variables for theme support).
//
const AGENT_COLORS = [
  'var(--agent-color-1)',
  'var(--agent-color-2)',
  'var(--agent-color-3)',
  'var(--agent-color-4)',
  'var(--agent-color-5)',
  'var(--agent-color-6)',
  'var(--agent-color-7)',
  'var(--agent-color-8)',
];

//
// User nickname color (distinct from agents).
//
const USER_COLOR = '#f87171'; // red
const USER_NICKNAME = 'agentChat_user';

//
// Get color for a nickname based on agent list.
//
function getNicknameColor(nickname: string, agents: AgentChatAgentInfo[]): string {
  if (nickname === USER_NICKNAME) {
    return USER_COLOR;
  }

  const agentIndex = agents.findIndex(a => a.nickname === nickname);
  if (agentIndex >= 0) {
    return AGENT_COLORS[agentIndex % AGENT_COLORS.length];
  }

  //
  // Fallback: hash the nickname to get a consistent color.
  //
  let hash = 0;
  for (let i = 0; i < nickname.length; i++) {
    hash = nickname.charCodeAt(i) + ((hash << 5) - hash);
  }
  return AGENT_COLORS[Math.abs(hash) % AGENT_COLORS.length];
}

//
// Message component for IRC-style display.
//
function AgentChatMessageItem({
  message,
  agents,
}: {
  message: AgentChatMessageInfo;
  agents: AgentChatAgentInfo[];
}) {
  const timestamp = new Date(message.timestamp).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });

  const isSystem = message.message_type === 'System';
  const isDm = message.message_type === 'DirectMessage';
  const nicknameColor = getNicknameColor(message.sender_nickname, agents);

  return (
    <div className={`font-mono text-xs py-0.5 ${isSystem ? 'text-muted italic' : ''}`}>
      <span className="text-muted">[{timestamp}]</span>{' '}
      {isDm && <span className="text-[var(--accent-purple)]">(DM)</span>}{' '}
      {!isSystem && (
        <>
          <span style={{ color: nicknameColor }}>&lt;{message.sender_nickname}&gt;</span>{' '}
        </>
      )}
      <span className={`${isSystem ? 'text-muted' : 'text-[var(--text-secondary)]'} prose prose-xs prose-invert max-w-none inline [&_p]:inline [&_p]:m-0 [&_code]:text-[var(--accent-info)] [&_code]:bg-[var(--bg-tertiary)] [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_pre]:my-1 [&_pre]:p-2 [&_pre]:bg-[var(--bg-tertiary)] [&_pre]:rounded [&_strong]:text-[var(--text-highlight)] [&_em]:text-[var(--text-secondary)]`}>
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown>
      </span>
    </div>
  );
}

//
// Agent list item with drag handle.
//
function AgentListItem({
  agent,
  agentIndex,
  onRemove,
}: {
  agent: AgentChatAgentInfo;
  agentIndex: number;
  onRemove: (id: string) => void;
}) {
  const statusColor = {
    Initializing: 'var(--accent-warning)',
    Ready: 'var(--accent-success)',
    Waiting: 'var(--accent-info)',
    Prompting: 'var(--accent-purple)',
    Disconnected: 'var(--text-muted)',
  }[agent.status] || 'var(--text-muted)';

  const nicknameColor = AGENT_COLORS[agentIndex % AGENT_COLORS.length];

  return (
    <div className="flex items-center gap-2 py-1.5 px-2 hover:bg-[var(--bg-tertiary)] group">
      <GripVertical size={12} className="text-muted cursor-grab" />
      <div
        className="w-2 h-2 rounded-full"
        style={{ backgroundColor: statusColor }}
        title={agent.status}
      />
      <span
        className="text-xs flex-1 truncate"
        style={{ color: nicknameColor }}
        title={agent.nickname}
      >
        {agent.nickname}
      </span>
      <button
        onClick={() => onRemove(agent.id)}
        className="text-muted hover:text-[var(--accent-error)] opacity-0 group-hover:opacity-100 transition-opacity"
        title="Remove agent"
      >
        &times;
      </button>
    </div>
  );
}

//
// Channel list item.
//
function ChannelListItem({
  channel,
  isSelected,
  onClick,
}: {
  channel: AgentChatChannelInfo;
  isSelected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-2 py-1.5 px-2 w-full text-left hover:bg-[var(--bg-tertiary)] ${
        isSelected ? 'bg-[var(--bg-tertiary)] text-[var(--accent-info)]' : ''
      }`}
    >
      <Hash size={12} />
      <span className="text-xs truncate">{channel.name.replace('#', '')}</span>
    </button>
  );
}

//
// Add agent modal.
//
function AddAgentModal({
  isOpen,
  onClose,
  onAdd,
}: {
  isOpen: boolean;
  onClose: () => void;
  onAdd: (nodeId: string, agentShortName: string) => void;
}) {
  const { state } = useApp();
  const [selectedNode, setSelectedNode] = useState<string>('');
  const [selectedAgent, setSelectedAgent] = useState<string>('');

  const nodes = state.systemState?.nodes || [];
  const selectedNodeData = nodes.find(n => n.node_id === selectedNode);
  const agents = selectedNodeData?.discovered_agents || [];

  const handleAdd = () => {
    if (selectedNode && selectedAgent) {
      onAdd(selectedNode, selectedAgent);
      onClose();
      setSelectedNode('');
      setSelectedAgent('');
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-[var(--bg-secondary)] border border-subtle p-4 w-[92vw] max-w-80">
        <h3 className="text-sm font-medium mb-4">Add Agent to AgentChat</h3>

        <div className="space-y-3">
          <div>
            <label className="text-xs text-muted block mb-1">Node</label>
            <select
              value={selectedNode}
              onChange={(e) => {
                setSelectedNode(e.target.value);
                setSelectedAgent('');
              }}
              className="w-full bg-[var(--bg-tertiary)] border border-subtle p-2 text-xs"
            >
              <option value="">Select node...</option>
              {nodes.map((node) => (
                <option key={node.node_id} value={node.node_id}>
                  {node.machine_name || node.node_id.slice(0, 8)}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="text-xs text-muted block mb-1">Agent</label>
            <select
              value={selectedAgent}
              onChange={(e) => setSelectedAgent(e.target.value)}
              className="w-full bg-[var(--bg-tertiary)] border border-subtle p-2 text-xs"
              disabled={!selectedNode}
            >
              <option value="">Select agent...</option>
              {agents.map((agent) => (
                <option key={agent.short_name} value={agent.short_name}>
                  {agent.name} ({agent.short_name})
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="flex justify-end gap-2 mt-4">
          <button
            onClick={onClose}
            className="px-3 py-1.5 text-xs text-muted hover:text-[var(--text-primary)]"
          >
            Cancel
          </button>
          <button
            onClick={handleAdd}
            disabled={!selectedNode || !selectedAgent}
            className="px-3 py-1.5 text-xs bg-[var(--accent-info)] text-[var(--bg-primary)] disabled:opacity-50"
          >
            Add
          </button>
        </div>
      </div>
    </div>
  );
}

//
// Main AgentChatPage component.
//
export default function AgentChatPage() {
  const {
    state,
    agentChatStart,
    agentChatStop,
    agentChatAddAgent,
    agentChatRemoveAgent,
    agentChatSendMessage,
    agentChatSetCurrentChannel,
    agentChatClearError,
  } = useApp();

  const [goalInput, setGoalInput] = useState('');
  const [messageInput, setMessageInput] = useState('');
  const [showAddAgent, setShowAddAgent] = useState(false);
  const [yoloMode, setYoloMode] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const { session, currentChannelId, messages, isLoading, error } = state.agentChat;
  const isActive = !!session;

  //
  // Scroll to bottom when messages change.
  //
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  //
  // Filter messages for current channel.
  //
  const filteredMessages = messages.filter((m) => {
    if (m.message_type === 'DirectMessage') return true;
    if (m.message_type === 'System') return true;
    return m.channel_id === currentChannelId;
  });

  //
  // Get current channel info.
  //
  const currentChannel = session?.channels.find((c) => c.id === currentChannelId);

  //
  // Handle start session.
  //
  const handleStart = useCallback(() => {
    agentChatStart(goalInput || null, yoloMode);
  }, [agentChatStart, goalInput, yoloMode]);

  //
  // Handle stop session.
  //
  const handleStop = useCallback(() => {
    agentChatStop();
  }, [agentChatStop]);

  //
  // Handle send message.
  //
  const handleSendMessage = useCallback(() => {
    if (!messageInput.trim()) return;

    //
    // Check for DM syntax: /dm <nickname> <message>
    //
    if (messageInput.startsWith('/dm ')) {
      const parts = messageInput.slice(4).split(' ');
      const recipient = parts[0];
      const content = parts.slice(1).join(' ');
      if (recipient && content) {
        agentChatSendMessage(content, undefined, recipient);
      }
    } else {
      agentChatSendMessage(messageInput);
    }

    setMessageInput('');
  }, [messageInput, agentChatSendMessage]);

  //
  // Handle add agent.
  //
  const handleAddAgent = useCallback(
    (nodeId: string, agentShortName: string) => {
      agentChatAddAgent(nodeId, agentShortName);
    },
    [agentChatAddAgent]
  );

  //
  // Handle key press in message input.
  //
  const handleKeyPress = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        handleSendMessage();
      }
    },
    [handleSendMessage]
  );

  return (
    <div className="flex flex-col h-full">
      {/*
      //
      // Header.
      //
      */}
      <div className="border-b border-subtle pb-4 md:pb-6">
        <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-3">
          <div>
            <div className="flex items-center gap-2 md:gap-3">
              <h1 className="text-xl md:text-2xl font-bold text-highlight">AgentChat</h1>
              <span className="px-2 py-0.5 text-xs font-medium bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] rounded">
                Experimental
              </span>
              {isActive && (
                <span className="text-xs text-[var(--accent-success)] bg-[var(--accent-success)]/10 px-2 py-0.5 rounded">
                  Active
                </span>
              )}
            </div>
            <p className="text-muted mt-1">
              IRC-style multi-agent collaboration
            </p>
          </div>

          <div className="flex items-center gap-2">
            {!isActive ? (
              <button
                onClick={handleStart}
                disabled={isLoading}
                className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:bg-[var(--accent-success)]/30 transition-colors text-sm disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {isLoading ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <PlayCircle size={14} />
                )}
                Start Session
              </button>
            ) : (
              <button
                onClick={handleStop}
                className="flex items-center gap-2 px-4 py-2 bg-[var(--accent-error)]/20 text-[var(--accent-error)] hover:bg-[var(--accent-error)]/30 transition-colors text-sm"
              >
                <StopCircle size={14} />
                Stop Session
              </button>
            )}
          </div>
        </div>

        {!isActive && (
          <div className="mt-4 md:mt-6 p-2 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 flex items-start gap-2">
            <AlertTriangle size={14} className="text-[var(--accent-error)] mt-0.5 flex-shrink-0" />
            <span className="text-xs text-[var(--accent-error)]">
              Agents may independently and dangerously choose to take action without confirmation.
            </span>
          </div>
        )}

        {!isActive && (
          <div className="mt-4 md:mt-6 space-y-3">
            <div>
              <label className="text-xs text-muted block mb-1">Session Goal (optional)</label>
              <textarea
                value={goalInput}
                onChange={(e) => setGoalInput(e.target.value)}
                placeholder="Describe what you want the agents to accomplish together..."
                rows={3}
                className="w-full bg-[var(--bg-tertiary)] border border-subtle px-3 py-2 text-xs resize-none"
              />
            </div>

            <div>
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={yoloMode}
                  onChange={(e) => setYoloMode(e.target.checked)}
                  className="w-4 h-4 accent-[var(--accent-error)]"
                />
                <span className="text-xs text-[var(--text-secondary)]">YOLO Mode</span>
              </label>
            </div>
          </div>
        )}

        {isActive && session?.goal && (
          <div className="text-xs text-muted bg-[var(--bg-tertiary)] p-2 border border-subtle">
            <span className="font-medium text-[var(--text-secondary)]">Goal:</span> {session.goal}
          </div>
        )}
      </div>

      {/*
      //
      // Error banner.
      //
      */}
      {error && (
        <div className="bg-[var(--accent-error)]/10 border-b border-[var(--accent-error)]/30 p-2 flex items-center gap-2">
          <AlertCircle size={14} className="text-[var(--accent-error)]" />
          <span className="text-xs text-[var(--accent-error)] flex-1">{error}</span>
          <button onClick={agentChatClearError} className="text-xs text-muted hover:text-[var(--text-primary)]">
            Dismiss
          </button>
        </div>
      )}

      {/*
      //
      // Main content.
      //
      */}
      {isActive ? (
        <div className="flex flex-col md:flex-row flex-1 min-h-0">
          {/*
          //
          // Sidebar.
          //
          */}
          <div className="w-full md:w-48 border-b md:border-b-0 md:border-r border-subtle flex flex-col md:max-h-none max-h-[45vh]">
            {/*
            //
            // Agents section.
            //
            */}
            <div className="p-2 border-b border-subtle">
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-1.5 text-xs font-medium text-muted">
                  <Users size={12} />
                  <span>Agents ({session.agents.length})</span>
                </div>
                <button
                  onClick={() => setShowAddAgent(true)}
                  className="text-muted hover:text-[var(--accent-info)]"
                  title="Add agent"
                >
                  <Plus size={12} />
                </button>
              </div>
              <div className="space-y-0.5 max-h-40 md:max-h-none overflow-y-auto">
                {session.agents.map((agent, index) => (
                  <AgentListItem
                    key={agent.id}
                    agent={agent}
                    agentIndex={index}
                    onRemove={agentChatRemoveAgent}
                  />
                ))}
                {session.agents.length === 0 && (
                  <div className="text-xs text-muted py-2 text-center">
                    No agents yet
                  </div>
                )}
              </div>
            </div>

            {/*
            //
            // Channels section.
            //
            */}
            <div className="p-2 flex-1 overflow-y-auto">
              <div className="flex items-center gap-1.5 text-xs font-medium text-muted mb-2">
                <Hash size={12} />
                <span>Channels</span>
              </div>
              <div className="space-y-0.5">
                {session.channels.map((channel) => (
                  <ChannelListItem
                    key={channel.id}
                    channel={channel}
                    isSelected={channel.id === currentChannelId}
                    onClick={() => agentChatSetCurrentChannel(channel.id)}
                  />
                ))}
              </div>
            </div>
          </div>

          {/*
          //
          // Chat area.
          //
          */}
          <div className="flex-1 flex flex-col min-w-0">
            {/*
            //
            // Channel header.
            //
            */}
            <div className="border-b border-subtle p-2">
              <div className="flex items-center gap-2">
                <Hash size={14} className="text-muted" />
                <span className="text-sm font-medium">
                  {currentChannel?.name || 'No channel selected'}
                </span>
                {currentChannel?.topic && (
                  <>
                    <span className="text-muted">|</span>
                    <span className="text-xs text-muted truncate">{currentChannel.topic}</span>
                  </>
                )}
              </div>
            </div>

            {/*
            //
            // Messages.
            //
            */}
            <div className="flex-1 overflow-y-auto p-2 font-mono">
              {filteredMessages.length === 0 ? (
                <div className="text-center text-muted text-xs py-8">
                  No messages yet. Add agents to start the conversation.
                </div>
              ) : (
                filteredMessages.map((msg) => (
                  <AgentChatMessageItem key={msg.id} message={msg} agents={session.agents} />
                ))
              )}
              <div ref={messagesEndRef} />
            </div>

            {/*
            //
            // Message input.
            //
            */}
            <div className="border-t border-subtle p-2">
              <div className="flex gap-2">
                <input
                  type="text"
                  value={messageInput}
                  onChange={(e) => setMessageInput(e.target.value)}
                  onKeyPress={handleKeyPress}
                  placeholder="Type a message... (use /dm <nick> for DMs)"
                  className="flex-1 bg-[var(--bg-tertiary)] border border-subtle px-3 py-2 text-xs"
                />
                <button
                  onClick={handleSendMessage}
                  disabled={!messageInput.trim()}
                  className="px-3 py-2 bg-[var(--accent-info)] text-[var(--bg-primary)] disabled:opacity-50"
                >
                  <Send size={14} />
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center max-w-md px-2">
            <MessageSquare size={48} className="text-muted mx-auto mb-4" />
            <h2 className="text-lg font-medium mb-2">Welcome to AgentChat</h2>
            <p className="text-sm text-muted mb-4">
              AgentChat is an IRC-style multi-agent collaboration environment. Start a session,
              add agents from your connected nodes, and watch them collaborate toward a common goal.
            </p>
            <ul className="text-xs text-muted text-center space-y-1 mb-4">
              <li>Agents communicate in channels like IRC</li>
              <li>Set a goal to guide the conversation</li>
              <li>Agents can join channels, send DMs, and collaborate</li>
              <li>You can participate by sending messages</li>
            </ul>
          </div>
        </div>
      )}

      {/*
      //
      // Add agent modal.
      //
      */}
      <AddAgentModal
        isOpen={showAddAgent}
        onClose={() => setShowAddAgent(false)}
        onAdd={handleAddAgent}
      />
    </div>
  );
}
