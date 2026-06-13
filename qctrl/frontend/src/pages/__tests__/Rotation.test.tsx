import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Rotation } from '../Rotation';

describe('Rotation Page', () => {
  it('renders without crashing', () => {
    render(<Rotation />);
    
    expect(screen.getByText('Map Rotation')).toBeInTheDocument();
  });

  it('displays the Map Rotation heading', () => {
    render(<Rotation />);
    
    const heading = screen.getByRole('heading', { level: 1 });
    expect(heading).toHaveTextContent('Map Rotation');
  });

  it('shows placeholder content', () => {
    render(<Rotation />);
    
    expect(screen.getByText('Map rotation management coming soon.')).toBeInTheDocument();
  });
});
