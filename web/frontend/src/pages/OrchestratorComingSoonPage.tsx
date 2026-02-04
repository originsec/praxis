import { Bot } from 'lucide-react';

export function OrchestratorComingSoonPage() {
  return (
    <div className="space-y-6 h-full flex flex-col">
      {/*
      //
      // Header.
      //
      */}
      <div>
        <h1 className="text-2xl font-bold text-highlight">Orchestrator</h1>
        <p className="text-muted mt-1">Interactive AI assistant for red teaming orchestration</p>
      </div>

      {/*
      //
      // Coming Soon Content.
      //
      */}
      <div className="flex-1 flex items-center justify-center">
        <div className="max-w-lg text-center space-y-6">
          <div className="flex justify-center text-muted">
            <Bot size={48} className="text-[var(--accent-info)]/50" />
          </div>

          <div>
            <h2 className="text-sm font-bold tracking-wider text-title mb-2">COMING SOON</h2>
            <p className="text-xs text-muted leading-relaxed">
              Orchestrator will be your intelligent command and control assistant. This module will
              provide natural language interaction for orchestrating operations, managing nodes,
              analyzing data, and automating complex red team workflows across your infrastructure.
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
