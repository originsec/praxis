import { useEffect, useState } from 'react';
import { ArrowUpCircle, X } from 'lucide-react';
import { useApp } from '../../context/AppContext';

//
// Version check endpoint.
//
const VERSION_CHECK_URL = 'https://praxis.originhq.com/version';

//
// Compare two semver strings. Returns:
//   1 if a > b
//   0 if a == b
//  -1 if a < b
//
function compareSemver(a: string, b: string): number {
  const parseVersion = (v: string) => {
    const clean = v.replace(/^v/, '');
    const parts = clean.split('.').map((p) => parseInt(p, 10) || 0);
    return { major: parts[0] || 0, minor: parts[1] || 0, patch: parts[2] || 0 };
  };

  const va = parseVersion(a);
  const vb = parseVersion(b);

  if (va.major !== vb.major) return va.major > vb.major ? 1 : -1;
  if (va.minor !== vb.minor) return va.minor > vb.minor ? 1 : -1;
  if (va.patch !== vb.patch) return va.patch > vb.patch ? 1 : -1;
  return 0;
}

interface VersionResponse {
  version: string;
  release_notes_url?: string;
}

export function VersionUpdateBanner() {
  const { state } = useApp();
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [releaseNotesUrl, setReleaseNotesUrl] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [error, setError] = useState<string | null>(null);

  //
  // Fetch latest version once connected and we have the current version.
  //
  useEffect(() => {
    if (!state.connected || !state.version) {
      return;
    }

    //
    // Check if already dismissed this session.
    //
    const dismissedVersion = sessionStorage.getItem('praxis_dismissed_version');
    if (dismissedVersion) {
      setDismissed(true);
    }

    const checkVersion = async () => {
      try {
        const response = await fetch(VERSION_CHECK_URL, {
          method: 'GET',
          headers: {
            Accept: 'application/json',
          },
        });

        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`);
        }

        const data: VersionResponse = await response.json();
        setLatestVersion(data.version);
        if (data.release_notes_url) {
          setReleaseNotesUrl(data.release_notes_url);
        }
      } catch (err) {
        //
        // Silently fail - version check is non-critical.
        //
        console.warn('Version check failed:', err);
        setError(err instanceof Error ? err.message : 'Unknown error');
      }
    };

    checkVersion();
  }, [state.connected, state.version]);

  //
  // Handle dismiss.
  //
  const handleDismiss = () => {
    setDismissed(true);
    if (latestVersion) {
      sessionStorage.setItem('praxis_dismissed_version', latestVersion);
    }
  };

  //
  // Don't show if:
  // - Not connected
  // - No current version
  // - No latest version fetched
  // - User dismissed
  // - Fetch error
  // - Current version is same or newer.
  //
  if (
    !state.connected ||
    !state.version ||
    !latestVersion ||
    dismissed ||
    error ||
    compareSemver(latestVersion, state.version) <= 0
  ) {
    return null;
  }

  return (
    <div className="bg-[var(--accent-info)]/15 border-b border-[var(--accent-info)]/30 px-4 py-2">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <ArrowUpCircle size={18} className="text-[var(--accent-info)] flex-shrink-0" />
          <p className="text-sm text-[var(--accent-info)]">
            <span className="font-medium">New version available:</span>
            {' '}Praxis {latestVersion} is available (you have {state.version}).
            {releaseNotesUrl && (
              <>
                {' '}
                <a
                  href={releaseNotesUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="underline hover:no-underline font-medium"
                >
                  View release notes
                </a>
              </>
            )}
          </p>
        </div>
        <button
          onClick={handleDismiss}
          className="p-1 hover:bg-[var(--accent-info)]/20 rounded transition-colors"
          aria-label="Dismiss"
        >
          <X size={16} className="text-[var(--accent-info)]" />
        </button>
      </div>
    </div>
  );
}
