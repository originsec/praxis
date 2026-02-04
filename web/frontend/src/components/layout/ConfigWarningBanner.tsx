import { useEffect } from 'react';
import { Link } from 'react-router-dom';
import { AlertTriangle } from 'lucide-react';
import { useApp } from '../../context/AppContext';

//
// Features that require model assignments for full functionality.
// Note: llm_feature_orchestrator excluded as Orchestrator is currently hidden.
//
const REQUIRED_FEATURES = [
  'llm_feature_semantic_ops',
  'llm_feature_semantic_parser',
  'llm_feature_traffic_parser',
] as const;

export function ConfigWarningBanner() {
  const { state, getConfig } = useApp();

  //
  // Fetch feature assignments and model definitions once connected.
  //
  useEffect(() => {
    if (state.connected) {
      getConfig([...REQUIRED_FEATURES, 'llm_model_definitions']);
    }
  }, [state.connected, getConfig]);

  //
  // Check if user has configured any model definitions.
  //
  const hasModelDefinitions = (() => {
    const raw = state.config.llm_model_definitions;
    if (!raw) return false;
    try {
      const defs = JSON.parse(raw);
      return Array.isArray(defs) && defs.length > 0;
    } catch {
      return false;
    }
  })();

  //
  // Only show warning if user has model definitions but not all features
  // assigned.
  //
  if (!hasModelDefinitions) {
    return null;
  }

  //
  // Check if any features are missing model assignments. A feature is missing
  // if the value is undefined, null, or empty string.
  //
  const missingFeatures = REQUIRED_FEATURES.filter((key) => {
    const value = state.config[key];
    return !value || value.trim() === '';
  });

  if (missingFeatures.length === 0) {
    return null;
  }

  return (
    <div className="bg-[var(--accent-error)]/15 border-b border-[var(--accent-error)]/30 px-4 py-2">
      <div className="flex items-center gap-3">
        <AlertTriangle size={18} className="text-[var(--accent-error)] flex-shrink-0" />
        <p className="text-sm text-[var(--accent-error)]">
          <span className="font-medium">Configuration incomplete:</span>
          {' '}Not all features have LLM providers assigned. Some functionality will be limited.
          {' '}
          <Link
            to="/settings?tab=llm_providers&sub=feature_selection"
            className="underline hover:no-underline font-medium"
          >
            Go to Settings
          </Link>
          {' '}to configure.
        </p>
      </div>
    </div>
  );
}
