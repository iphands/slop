import type { ReactNode } from 'react';

interface SectionProps {
  title?: string;
  children: ReactNode;
  className?: string;
  showDivider?: boolean;
}

export function Section({ title, children, className = '', showDivider = false }: SectionProps) {
  return (
    <section className={`p-4 bg-gray-800 rounded-lg ${className}`}>
      {title && (
        <>
          <h2 className="text-lg font-semibold mb-3">{title}</h2>
          {showDivider && <hr className="border-gray-700 mb-4" />}
        </>
      )}
      {children}
    </section>
  );
}
