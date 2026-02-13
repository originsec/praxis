import { Outlet } from 'react-router-dom';
import { useState } from 'react';
import { Sidebar } from './Sidebar';
import { Header } from './Header';
import { ConfigWarningBanner } from './ConfigWarningBanner';
import { VersionUpdateBanner } from './VersionUpdateBanner';

export function MainLayout() {
  const [isMobileNavOpen, setIsMobileNavOpen] = useState(false);

  return (
    <div className="flex h-screen overflow-hidden">
      <div className="hidden md:block">
        <Sidebar />
      </div>

      {isMobileNavOpen && (
        <button
          className="md:hidden fixed inset-0 z-40 bg-black/50"
          onClick={() => setIsMobileNavOpen(false)}
          aria-label="Close navigation"
        />
      )}

      <div
        className={`md:hidden fixed left-0 top-0 z-50 h-full transition-transform duration-200 ${
          isMobileNavOpen ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        <Sidebar onNavigate={() => setIsMobileNavOpen(false)} />
      </div>

      <div className="flex-1 flex flex-col overflow-hidden">
        <Header onOpenMobileNav={() => setIsMobileNavOpen(true)} />
        <VersionUpdateBanner />
        <ConfigWarningBanner />

        <main className="flex-1 overflow-auto p-4 md:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
