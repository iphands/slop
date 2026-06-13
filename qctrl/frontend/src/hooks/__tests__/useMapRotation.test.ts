import { describe, it, expect, vi, beforeEach } from 'vitest';
import { determineNextMap, fetchAvailableMaps } from '../useMapRotation';

vi.mock('../../lib/api', () => ({
  getMaps: vi.fn(),
}));

describe('determineNextMap', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('Sequential mode', () => {
    it('should return next map in queue', async () => {
      const result = await determineNextMap(
        'Sequential',
        ['q2dm1', 'kessel', 'dm6'],
        'q2dm1'
      );
      expect(result).toBe('kessel');
    });

    it('should loop to first map at end of queue', async () => {
      const result = await determineNextMap(
        'Sequential',
        ['q2dm1', 'kessel', 'dm6'],
        'dm6'
      );
      expect(result).toBe('q2dm1');
    });

    it('should return first map if current not in queue', async () => {
      const result = await determineNextMap(
        'Sequential',
        ['q2dm1', 'kessel'],
        'unknown'
      );
      expect(result).toBe('q2dm1');
    });

    it('should loop to first map with empty queue', async () => {
      const result = await determineNextMap('Sequential', [], 'q2dm1');
      expect(result).toBe('q2dm1');
    });

    it('should return default map if empty queue and no current', async () => {
      const result = await determineNextMap('Sequential', [], null);
      expect(result).toBe('q2dm1');
    });
  });

  describe('Random mode', () => {
    it('should return random map from queue', async () => {
      const maps = ['q2dm1', 'kessel', 'dm6'];
      const results = new Set();

      for (let i = 0; i < 10; i++) {
        const result = await determineNextMap('Random', maps, 'q2dm1');
        results.add(result);
      }

      expect(results.size).toBeGreaterThanOrEqual(2);
      results.forEach((result) => {
        expect(maps).toContain(result);
      });
    });

    it('should return the only map in queue', async () => {
      const result = await determineNextMap('Random', ['q2dm1'], 'q2dm1');
      expect(result).toBe('q2dm1');
    });

    it('should fetch all maps and pick random when queue is empty', async () => {
      const { getMaps } = await import('../../lib/api');
      vi.mocked(getMaps).mockResolvedValue({
        maps: [
          { name: 'q2dm1', filename: 'baseq2/maps/q2dm1.bsp', size: 1000, modified: 1234567890, source: { Pak: 'pak0.pak' } },
          { name: 'kessel', filename: 'baseq2/maps/kessel.bsp', size: 2000, modified: 1234567890, source: { Pak: 'pak0.pak' } },
          { name: 'dm6', filename: 'baseq2/maps/dm6.bsp', size: 1500, modified: 1234567890, source: { Pak: 'pak0.pak' } },
        ],
      });

      const result = await determineNextMap('Random', [], 'q2dm1');
      
      expect(getMaps).toHaveBeenCalled();
      expect(['q2dm1', 'kessel', 'dm6']).toContain(result);
    });

    it('should fallback to q2dm1 if fetch fails', async () => {
      const { getMaps } = await import('../../lib/api');
      vi.mocked(getMaps).mockRejectedValue(new Error('Network error'));

      const result = await determineNextMap('Random', [], 'q2dm1');
      
      expect(result).toBe('q2dm1');
    });
  });
});

describe('fetchAvailableMaps', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should return map names from API', async () => {
    const { getMaps } = await import('../../lib/api');
    vi.mocked(getMaps).mockResolvedValue({
      maps: [
        { name: 'q2dm1', filename: 'baseq2/maps/q2dm1.bsp', size: 1000, modified: 1234567890, source: { Pak: 'pak0.pak' } },
        { name: 'kessel', filename: 'baseq2/maps/kessel.bsp', size: 2000, modified: 1234567890, source: { Pak: 'pak0.pak' } },
      ],
    });

    const result = await fetchAvailableMaps();
    
    expect(result).toEqual(['q2dm1', 'kessel']);
  });

  it('should fallback to q2dm1 on error', async () => {
    const { getMaps } = await import('../../lib/api');
    vi.mocked(getMaps).mockRejectedValue(new Error('Network error'));

    const result = await fetchAvailableMaps();
    
    expect(result).toEqual(['q2dm1']);
  });
});
