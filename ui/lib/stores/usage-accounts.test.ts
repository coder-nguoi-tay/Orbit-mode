import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { ProviderQuota } from '../types';

const { mockGetUsageOverview, mockRefreshCodexQuotas, mockRefreshClaudeQuotas } = vi.hoisted(
  () => ({
    mockGetUsageOverview: vi.fn(),
    mockRefreshCodexQuotas: vi.fn(),
    mockRefreshClaudeQuotas: vi.fn(),
  })
);

vi.mock('../tauri/usage', () => ({
  getUsageOverview: mockGetUsageOverview,
  refreshCodexQuotas: mockRefreshCodexQuotas,
  refreshClaudeQuotas: mockRefreshClaudeQuotas,
}));
vi.mock('../tauri/events', () => ({
  onSessionUsageUpdated: vi.fn(),
  onProviderQuotaUpdated: vi.fn(),
}));

import { mergeAccountQuota, refreshUsageOverview, providerQuotas } from './usage';

function quota(overrides: Partial<ProviderQuota> = {}): ProviderQuota {
  return {
    provider: 'codex',
    accountKey: 'default',
    providerAccountId: null,
    fiveHour: { utilization: 0.33 },
    sevenDay: { utilization: 0.05 },
    updatedAt: '2026-09-26T00:00:00Z',
    source: 'app_server',
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  providerQuotas.set([]);
  mockGetUsageOverview.mockResolvedValue({ quotas: [] });
  mockRefreshCodexQuotas.mockResolvedValue([]);
  mockRefreshClaudeQuotas.mockResolvedValue([]);
});

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

/** The live CLI read is deliberately not awaited, so let its promise chain settle. */
const settleLiveRead = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('refreshUsageOverview live Codex read', () => {
  it('does not launch a Claude CLI probe during usage refresh', async () => {
    await refreshUsageOverview();
    await settleLiveRead();
    expect(mockRefreshClaudeQuotas).not.toHaveBeenCalled();
  });

  it('never waits for the CLI read, so a hung codex cannot stall the UI', async () => {
    mockGetUsageOverview.mockResolvedValue({ quotas: [quota()] });
    // A read that never settles — refreshUsageOverview must still resolve.
    mockRefreshCodexQuotas.mockReturnValue(new Promise(() => {}));
    await expect(refreshUsageOverview()).resolves.toBeUndefined();
    expect(get(providerQuotas)).toHaveLength(1);
  });

  it('populates the 5-hour window when the DB overview has no quotas', async () => {
    mockRefreshCodexQuotas.mockResolvedValue([quota()]);
    await refreshUsageOverview();
    await settleLiveRead();
    const list = get(providerQuotas);
    expect(list).toHaveLength(1);
    expect(list[0].fiveHour?.utilization).toBe(0.33);
    expect(list[0].source).toBe('app_server');
  });

  it('overwrites a stale persisted window with the live value', async () => {
    mockGetUsageOverview.mockResolvedValue({
      quotas: [quota({ fiveHour: { utilization: 0.9 }, source: 'session_event' })],
    });
    mockRefreshCodexQuotas.mockResolvedValue([quota({ fiveHour: { utilization: 0.33 } })]);
    await refreshUsageOverview();
    await settleLiveRead();
    const list = get(providerQuotas);
    expect(list).toHaveLength(1);
    expect(list[0].fiveHour?.utilization).toBe(0.33);
  });

  it('keeps a persisted window the live read did not report', async () => {
    mockGetUsageOverview.mockResolvedValue({
      quotas: [quota({ sevenDay: { utilization: 0.61 } })],
    });
    mockRefreshCodexQuotas.mockResolvedValue([quota({ sevenDay: null })]);
    await refreshUsageOverview();
    await settleLiveRead();
    expect(get(providerQuotas)[0].sevenDay?.utilization).toBe(0.61);
  });

  it('keeps per-account quotas separate', async () => {
    mockRefreshCodexQuotas.mockResolvedValue([
      quota({ accountKey: 'acc-a', providerAccountId: 'acc-a', fiveHour: { utilization: 0.1 } }),
      quota({ accountKey: 'acc-b', providerAccountId: 'acc-b', fiveHour: { utilization: 0.8 } }),
    ]);
    await refreshUsageOverview();
    await settleLiveRead();
    const list = get(providerQuotas);
    expect(list).toHaveLength(2);
    expect(list.find((q) => q.providerAccountId === 'acc-a')?.fiveHour?.utilization).toBe(0.1);
    expect(list.find((q) => q.providerAccountId === 'acc-b')?.fiveHour?.utilization).toBe(0.8);
  });

  it('leaves existing quotas untouched when the live read fails', async () => {
    mockGetUsageOverview.mockResolvedValue({ quotas: [quota()] });
    mockRefreshCodexQuotas.mockRejectedValue(new Error('codex not installed'));
    await refreshUsageOverview();
    await settleLiveRead();
    expect(get(providerQuotas)[0].fiveHour?.utilization).toBe(0.33);
  });

  it('still reads live quotas when the overview request fails', async () => {
    mockGetUsageOverview.mockRejectedValue(new Error('db unavailable'));
    mockRefreshCodexQuotas.mockResolvedValue([quota()]);
    await refreshUsageOverview();
    await settleLiveRead();
    expect(get(providerQuotas)[0].fiveHour?.utilization).toBe(0.33);
  });
});
