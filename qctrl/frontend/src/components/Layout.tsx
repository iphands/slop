import type { ReactNode } from 'react';
import { MapCountdown } from './MapCountdown';

interface LayoutProps {
  children: ReactNode;
}

export function Layout({ children }: LayoutProps) {
  return (
    <div className="min-h-screen bg-gray-900 text-white">
      <header className="p-4 border-b border-gray-700 flex items-center justify-between">
        <h1 className="text-xl font-bold">qctrl</h1>
        <MapCountdown variant="chip" />
      </header>
      <main className="p-4">{children}</main>
      <footer className="p-4 border-t border-gray-700 text-sm text-gray-400">
        <p>qctrl v0.1.0</p>
      </footer>
    </div>
  );
}
