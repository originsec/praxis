import { MessageSquare } from 'lucide-react';

export function AgentChatComingSoonPage() {
  return (
    <div className="space-y-6 h-full flex flex-col">
      {/*
      //
      // Header.
      //
      */}
      <div>
        <h1 className="text-2xl font-bold text-highlight">Agent Chat</h1>
        <p className="text-muted mt-1">Direct communication with deployed agents</p>
      </div>

      {/*
      //
      // Coming Soon Content.
      //
      */}
      <div className="flex-1 flex items-center justify-center">
        <div className="max-w-lg text-center space-y-6">
          <div className="flex justify-center text-muted">
            <MessageSquare size={48} className="text-[var(--accent-info)]/50" />
          </div>

          <div>
            <h2 className="text-sm font-bold tracking-wider text-title mb-2">COMING SOON</h2>
            <p className="text-xs text-muted leading-relaxed">
              Agent Chat will provide real-time interactive communication with your deployed agents.
              This module will allow you to send commands, receive responses, and manage agent
              sessions directly through a chat interface.
            </p>
          </div>

          <div className="pt-4 border-t border-subtle">
            <p className="text-xs text-muted/60 italic">Under active development</p>
          </div>
        </div>
      </div>
    </div>
  );
}
