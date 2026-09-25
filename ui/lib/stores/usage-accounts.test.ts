import { describe, expect, it, vi } from 'vitest';
import type { ProviderQuota } from '../types';

vi.mock('../tauri/usage', () => ({ getUsageOverview: vi.fn() }));
vi.mock('../tauri/events', () => ({
  onSessionUsageUpdated: vi.fn(),
  onProviderQuotaUpdated: vi.fn(),
}));

import { mergeAccountQuota } from './usage';

describe('account quota updates', () => {
  it('keeps separate windows for one account and never merges another account', () => {
    const accountA: ProviderQuota = {
      provider: 'codex',
      accountKey: 'account-a',
      providerAccountId: 'account-a',
      fiveHour: { utilization: 0.72 },
      sevenDay: { utilization: 0.45 },
      updatedAt: '2026-09-25T00:00:00Z',
      source: 'cli_event',
    };
    const accountB: ProviderQuota = {
      ...accountA,
      accountKey: 'account-b',
      providerAccountId: 'account-b',
      fiveHour: { utilization: 0.19 },
    };
    const nextA = mergeAccountQuota(accountA, {
      ...accountA,
      fiveHour: { utilization: 0.9 },
      sevenDay: null,
    });
    expect(nextA.fiveHour?.utilization).toBe(0.9);
    expect(nextA.sevenDay?.utilization).toBe(0.45);
    expect(accountB.fiveHour?.utilization).toBe(0.19);
  });
});
