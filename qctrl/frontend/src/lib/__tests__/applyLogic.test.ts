import { describe, it, expect } from 'vitest';
import { buildApplyCommands, type Change } from '../applyLogic';

const fraglimit: Change = {
  type: 'fraglimit',
  pendingValue: 10,
  description: 'fraglimit 10',
};

const mapChange = (value: string): Change => ({
  type: 'map',
  pendingValue: value,
  description: `map ${value}`,
});

describe('buildApplyCommands', () => {
  it('uses the queued map change', () => {
    const commands = buildApplyCommands([fraglimit, mapChange('q2dm3')], 'q2dm1');
    expect(commands).toEqual(['fraglimit 10', 'map q2dm3']);
  });

  it('restarts the current map to apply cvar changes when the map is known', () => {
    expect(buildApplyCommands([fraglimit], 'q2dm1')).toEqual(['fraglimit 10', 'map q2dm1']);
  });

  it('sends no map command when the current map is empty', () => {
    expect(buildApplyCommands([fraglimit], '')).toEqual(['fraglimit 10']);
  });

  it('sends no map command when the current map is unknown', () => {
    expect(buildApplyCommands([fraglimit], 'unknown')).toEqual(['fraglimit 10']);
  });

  it('falls back to the known current map when the queued map value is empty', () => {
    expect(buildApplyCommands([mapChange('')], 'q2dm1')).toEqual(['map q2dm1']);
  });

  it('sends no map command when both the queued and current maps are empty', () => {
    expect(buildApplyCommands([mapChange('')], '')).toEqual([]);
  });

  it('still restarts a known map when nothing is queued', () => {
    expect(buildApplyCommands([], 'q2dm2')).toEqual(['map q2dm2']);
  });
});
