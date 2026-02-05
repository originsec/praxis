import { Link } from 'react-router-dom';
import { Home, AlertCircle } from 'lucide-react';

export function NotFoundPage() {
  return (
    <div className="flex items-center justify-center h-full">
      <div className="text-center">
        <AlertCircle size={48} className="mx-auto mb-4 text-muted opacity-50" />
        <h1 className="text-2xl font-bold text-title mb-2">Page Not Found</h1>
        <p className="text-muted mb-6">The page you're looking for doesn't exist.</p>
        <Link
          to="/"
          className="inline-flex items-center gap-2 px-4 py-2 bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:bg-[var(--accent-info)]/30 transition-colors"
        >
          <Home size={16} />
          Go to Dashboard
        </Link>
      </div>
    </div>
  );
}
