import { useState, useRef, useEffect, useMemo } from 'react';
import { Send, Bot, Loader2, Download, Square } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { FloatingPanel } from './FloatingPanel';
import { useApp, type AgentSessionMessage } from '../../context/AppContext';
import { generateUUID } from '../../utils/uuid';
import { exportAgentSession, downloadTextFile } from '../../utils/export';
import type { NodeState } from '../../api/types';

interface AgentSessionModalProps {
  nodeId: string;
  agentShortName: string;
  node: NodeState;
  onClose: () => void;
}

export function AgentSessionModal({ nodeId, agentShortName, node, onClose }: AgentSessionModalProps) {
  const { state, sendCommand, addAgentSessionMessage } = useApp();
  const [input, setInput] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const selectedAgent = node.selected_agent?.short_name === agentShortName ? node.selected_agent : null;
  const sessionId = selectedAgent?.session_id;
  const hasSession = !!sessionId;
  const messages: AgentSessionMessage[] = useMemo(
    () => sessionId ? (state.agentSessionMessages[sessionId] || []) : [],
    [sessionId, state.agentSessionMessages],
  );

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    if (hasSession) {
      setTimeout(() => inputRef.current?.focus(), 100);
    }
  }, [hasSession]);

  const handleSend = async () => {
    if (!input.trim() || !sessionId || isLoading) return;
    const text = input.trim();
    setInput('');
    setIsLoading(true);

    addAgentSessionMessage(sessionId, {
      role: 'user',
      content: text,
      timestamp: new Date(),
    });

    try {
      const transactionId = generateUUID();
      const response = await sendCommand(nodeId, {
        Session: { Prompt: { text, transaction_id: transactionId } },
      });

      if (response?.result) {
        const result = response.result;
        if ('Session' in result) {
          const sessionResult = result.Session;
          if (typeof sessionResult === 'object' && 'PromptResponse' in sessionResult) {
            addAgentSessionMessage(sessionId, {
              role: 'assistant',
              content: sessionResult.PromptResponse.response,
              timestamp: new Date(),
            });
          }
        }
      }
    } finally {
      setIsLoading(false);
      inputRef.current?.focus();
    }
  };

  const handleCloseSession = async () => {
    if (!hasSession) return;
    await sendCommand(nodeId, { Session: 'Close' });
    onClose();
  };

  const handleExport = () => {
    if (messages.length === 0) return;
    const content = exportAgentSession(messages, agentShortName, nodeId);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    downloadTextFile(content, `agent-session-${agentShortName}-${timestamp}.md`);
  };

  return (
    <FloatingPanel
      title={`${agentShortName} · ${node.machine_name || nodeId.slice(0, 8)}`}
      onClose={onClose}
      defaultWidth={480}
      defaultHeight={400}
      headerActions={
        <>
          <button
            onClick={handleExport}
            disabled={messages.length === 0}
            className="p-1 text-muted hover:text-[var(--text-primary)] transition-colors disabled:opacity-30"
            title="Export session"
          >
            <Download size={11} />
          </button>
          {hasSession && (
            <button
              onClick={handleCloseSession}
              className="p-1 text-[var(--accent-error)] hover:text-[var(--accent-error)] transition-colors"
              title="Close session"
            >
              <Square size={9} />
            </button>
          )}
        </>
      }
    >
      {!hasSession ? (
        <div className="flex items-center justify-center flex-1">
          <div className="text-center p-4">
            <Bot size={28} className="mx-auto mb-2 text-muted opacity-50" />
            <p className="text-muted text-[11px]">No active session</p>
            <p className="text-[10px] text-muted mt-0.5">Start a session from the node card</p>
          </div>
        </div>
      ) : (
        <>
          {/*
          //
          // Session info.
          //
          */}
          {selectedAgent && (
            <div className="px-3 py-1 border-b border-subtle bg-[var(--bg-tertiary)] text-[9px] text-muted flex items-center gap-3 flex-shrink-0">
              {selectedAgent.process_name && <span>{selectedAgent.process_name}</span>}
              <span className="font-mono truncate">{sessionId}</span>
              {selectedAgent.working_dir && <span className="truncate">{selectedAgent.working_dir}</span>}
            </div>
          )}

          {/*
          //
          // Messages.
          //
          */}
          <div className="flex-1 overflow-auto p-2 space-y-1.5">
            {messages.map((msg, idx) => (
              <div key={idx} className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                <div className={`max-w-[90%] px-2 py-1.5 ${
                  msg.role === 'user'
                    ? 'bg-[var(--accent-purple)]/10 border-l-2 border-l-[var(--accent-purple)]'
                    : 'bg-[var(--bg-secondary)] border-l-2 border-l-[var(--accent-success)]'
                }`}>
                  {msg.role === 'assistant' ? (
                    <div className="prose prose-invert max-w-none break-words text-[10px] leading-relaxed text-[var(--text-secondary)] [&_p]:my-0.5 [&_pre]:text-[9px] [&_code]:text-[9px]">
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content}</ReactMarkdown>
                    </div>
                  ) : (
                    <div className="whitespace-pre-wrap break-words text-[10px]">{msg.content}</div>
                  )}
                  <p className="text-[8px] text-muted/40 mt-0.5">{msg.timestamp.toLocaleTimeString()}</p>
                </div>
              </div>
            ))}

            {isLoading && (
              <div className="flex justify-start">
                <div className="px-2 py-1.5 bg-[var(--bg-secondary)] flex items-center gap-1.5 text-muted text-[10px]">
                  <Loader2 size={10} className="animate-spin" />
                  <span>Thinking...</span>
                </div>
              </div>
            )}

            <div ref={messagesEndRef} />
          </div>

          {/*
          //
          // Input.
          //
          */}
          <div className="px-2 py-1.5 border-t border-subtle flex-shrink-0">
            <div className="flex gap-1">
              <input
                ref={inputRef}
                type="text"
                value={input}
                onChange={e => setInput(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && !e.shiftKey && handleSend()}
                placeholder="Send a message..."
                className="flex-1 bg-[var(--bg-primary)] border border-subtle px-2 py-1 text-[11px] text-[var(--text-primary)] placeholder-[var(--text-secondary)] focus:outline-none focus:border-[var(--border-active)]"
                disabled={isLoading}
              />
              <button
                onClick={handleSend}
                disabled={!input.trim() || isLoading}
                className="px-2 py-1 bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30 transition-colors disabled:opacity-30"
              >
                <Send size={12} />
              </button>
            </div>
          </div>
        </>
      )}
    </FloatingPanel>
  );
}
