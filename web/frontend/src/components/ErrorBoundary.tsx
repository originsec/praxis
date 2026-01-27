import { Component, type ErrorInfo, type ReactNode } from 'react';
import { AlertTriangle, RefreshCw } from 'lucide-react';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null, errorInfo: null };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('ErrorBoundary caught an error:', error, errorInfo);
    this.setState({ errorInfo });
  }

  handleReload = () => {
    window.location.href = '/';
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)] flex items-center justify-center p-8">
          <div className="max-w-lg w-full space-y-6">
            <div className="text-center">
              <AlertTriangle size={48} className="mx-auto mb-4 text-[var(--accent-error)]" />
              <h1 className="text-2xl font-bold text-title mb-2">Something went wrong</h1>
              <p className="text-muted">An error occurred while rendering this page.</p>
            </div>

            <div className="bg-[var(--bg-secondary)] border border-subtle p-4 rounded overflow-auto max-h-64">
              <p className="text-sm font-mono text-[var(--accent-error)]">
                {this.state.error?.message || 'Unknown error'}
              </p>
              {this.state.error?.stack && (
                <pre className="mt-2 text-xs text-muted whitespace-pre-wrap">
                  {this.state.error.stack}
                </pre>
              )}
            </div>

            <div className="flex justify-center gap-4">
              <button
                onClick={this.handleReload}
                className="inline-flex items-center gap-2 px-4 py-2 bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors"
              >
                <RefreshCw size={16} />
                Go to Dashboard
              </button>
              <button
                onClick={() => window.location.reload()}
                className="inline-flex items-center gap-2 px-4 py-2 border border-subtle hover:bg-[var(--bg-tertiary)] transition-colors"
              >
                <RefreshCw size={16} />
                Reload Page
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
