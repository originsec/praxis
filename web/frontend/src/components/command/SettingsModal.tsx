import { Monitor, LayoutGrid, Sun, Moon, ExternalLink } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { Modal } from '../common/Modal';
import { useTheme } from '../../context/ThemeContext';
import { getUiMode, setUiMode, type UiMode } from '../../utils/uiMode';

interface SettingsModalProps {
  onClose: () => void;
}

export function SettingsModal({ onClose }: SettingsModalProps) {
  const { theme, setTheme, isDark } = useTheme();
  const navigate = useNavigate();
  const currentMode = getUiMode();

  const handleModeChange = (mode: UiMode) => {
    setUiMode(mode);
    if (mode === 'legacy') {
      navigate('/dashboard');
    }
    onClose();
  };

  return (
    <Modal isOpen={true} onClose={onClose} title="Settings" size="md">
      <div className="space-y-6">

        {/*
        //
        // Interface mode selection.
        //
        */}

        <div>
          <h3 className="text-sm font-semibold text-highlight tracking-wider mb-1">INTERFACE MODE</h3>
          <p className="text-xs text-muted mb-3">Choose your preferred interface layout</p>

          <div className="space-y-2">
            <button
              onClick={() => handleModeChange('command_center')}
              className={`w-full flex items-center gap-3 p-3 border transition-colors text-left ${
                currentMode === 'command_center'
                  ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                  : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
              }`}
            >
              <LayoutGrid size={20} className={currentMode === 'command_center' ? 'text-[var(--accent-info)]' : 'text-muted'} />
              <div className="flex-1">
                <p className={`text-sm font-medium ${currentMode === 'command_center' ? 'text-highlight' : 'text-[var(--text-primary)]'}`}>
                  Command Center
                </p>
                <p className="text-xs text-muted">
                  Full-screen grid with node cards, orchestrator panel, and activity bar
                </p>
              </div>
              {currentMode === 'command_center' && (
                <span className="text-[10px] tracking-wider text-[var(--accent-info)]">ACTIVE</span>
              )}
            </button>

            <button
              onClick={() => handleModeChange('legacy')}
              className={`w-full flex items-center gap-3 p-3 border transition-colors text-left ${
                currentMode === 'legacy'
                  ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                  : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
              }`}
            >
              <Monitor size={20} className={currentMode === 'legacy' ? 'text-[var(--accent-info)]' : 'text-muted'} />
              <div className="flex-1">
                <p className={`text-sm font-medium ${currentMode === 'legacy' ? 'text-highlight' : 'text-[var(--text-primary)]'}`}>
                  Classic
                </p>
                <p className="text-xs text-muted">
                  Sidebar navigation with dedicated pages for each feature
                </p>
              </div>
              {currentMode === 'legacy' && (
                <span className="text-[10px] tracking-wider text-[var(--accent-info)]">ACTIVE</span>
              )}
            </button>
          </div>
        </div>

        {/*
        //
        // Theme selection.
        //
        */}

        <div className="pt-4 border-t border-subtle">
          <h3 className="text-sm font-semibold text-highlight tracking-wider mb-1">THEME</h3>
          <p className="text-xs text-muted mb-3">Choose your visual theme</p>

          <div className="flex gap-2">
            <button
              onClick={() => setTheme('origin_light')}
              className={`flex-1 flex items-center justify-center gap-2 p-3 border transition-colors ${
                theme === 'origin_light'
                  ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                  : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
              }`}
            >
              <Sun size={16} className={theme === 'origin_light' ? 'text-[var(--accent-info)]' : 'text-muted'} />
              <span className={`text-sm ${theme === 'origin_light' ? 'text-highlight' : 'text-muted'}`}>Light</span>
            </button>

            <button
              onClick={() => setTheme('praxis_dark')}
              className={`flex-1 flex items-center justify-center gap-2 p-3 border transition-colors ${
                theme === 'praxis_dark'
                  ? 'border-[var(--accent-info)]/50 bg-[var(--accent-info)]/5'
                  : 'border-subtle hover:border-[var(--border-hover)] hover:bg-[var(--bg-secondary)]'
              }`}
            >
              <Moon size={16} className={theme === 'praxis_dark' ? 'text-[var(--accent-info)]' : 'text-muted'} />
              <span className={`text-sm ${theme === 'praxis_dark' ? 'text-highlight' : 'text-muted'}`}>Dark</span>
            </button>
          </div>
        </div>

        {/*
        //
        // Link to full settings page.
        //
        */}

        <div className="pt-4 border-t border-subtle">
          <button
            onClick={() => { navigate('/settings'); onClose(); }}
            className="flex items-center gap-2 text-xs text-muted hover:text-[var(--accent-info)] transition-colors"
          >
            <ExternalLink size={12} />
            <span>Open full settings (LLM providers, agents, service config)</span>
          </button>
        </div>
      </div>
    </Modal>
  );
}
