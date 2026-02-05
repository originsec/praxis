import { Skull, Crosshair, Shield, KeyRound } from 'lucide-react';

export function ToolkitPage() {
  return (
    <div className="space-y-6 h-full flex flex-col">
      {/*
      //
      // Header.
      //
      */}
      <div>
        <h1 className="text-2xl font-bold text-highlight">Toolkit</h1>
        <p className="text-muted mt-1">Offensive tools and utilities</p>
      </div>

      {/*
      //
      // Coming Soon Content.
      //
      */}
      <div className="flex-1 flex items-center justify-center">
        <div className="max-w-lg text-center space-y-6">
          <div className="flex justify-center gap-4 text-muted">
            <Skull size={32} className="text-[var(--accent-error)]/50" />
            <Crosshair size={32} className="text-[var(--accent-purple)]/50" />
            <Shield size={32} className="text-[var(--accent-warning)]/50" />
            <KeyRound size={32} className="text-[var(--accent-success)]/50" />
          </div>

          <div>
            <h2 className="text-sm font-bold tracking-wider text-title mb-2">COMING SOON</h2>
            <p className="text-xs text-muted leading-relaxed">
              The Toolkit will be your swiss army knife for offensive operations. This module will
              include capabilities for session poisoning, credential harvesting, payload generation,
              persistence mechanisms, and other red team utilities.
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
