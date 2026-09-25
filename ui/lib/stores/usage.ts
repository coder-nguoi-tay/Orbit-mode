import { writable } from 'svelte/store';
import type { ProviderQuota, SessionUsageSnapshot, UsageOverview } from '../types';
import { getUsageOverview } from '../tauri/usage';
import { onSessionUsageUpdated, onProviderQuotaUpdated } from '../tauri/events';

export const usageOverview = writable<UsageOverview | null>(null);
export const providerQuotas = writable<ProviderQuota[]>([]);
export const sessionUsages = writable<Map<number, SessionUsageSnapshot>>(new Map());
export const usageCenterOpen = writable<boolean>(false);
export const isUsageLoading = writable<boolean>(false);

let initialized = false;

function isSameQuota(a: ProviderQuota, b: ProviderQuota): boolean {
  if (a.provider !== b.provider) return false;
  if (a.accountKey === b.accountKey) return true;
  if (a.providerAccountId && b.providerAccountId && a.providerAccountId === b.providerAccountId) {
    return true;
  }
  if (
    (a.accountKey === 'default' || !a.providerAccountId) &&
    (b.accountKey === 'default' || !b.providerAccountId)
  ) {
    return true;
  }
  return false;
}

/** Preserve the other account window when a live sample updates only one window.
 * @param current Last known quota for this provider account.
 * @param incoming Newly received account quota sample.
 * @return Combined latest windows for that same account.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function mergeAccountQuota(current: ProviderQuota, incoming: ProviderQuota): ProviderQuota {
  return {
    ...incoming,
    providerAccountId: incoming.providerAccountId ?? current.providerAccountId,
    fiveHour: incoming.fiveHour ?? current.fiveHour,
    sevenDay: incoming.sevenDay ?? current.sevenDay,
  };
}

export async function refreshUsageOverview() {
  isUsageLoading.set(true);
  try {
    const overview = await getUsageOverview();
    usageOverview.set(overview);
    if (overview.quotas) {
      providerQuotas.set(overview.quotas);
    }
  } catch (err) {
    console.error('Failed to fetch usage overview:', err);
  } finally {
    isUsageLoading.set(false);
  }
}

/** Start one shared subscription for account quota and session usage events.
 * @return No value; stores receive live updates.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function initUsageListeners() {
  if (initialized) return;
  initialized = true;

  // Initial fetch
  refreshUsageOverview();

  // Listen to live usage events
  onSessionUsageUpdated((snapshot: SessionUsageSnapshot) => {
    sessionUsages.update((map) => {
      const next = new Map(map);
      next.set(snapshot.sessionId, snapshot);
      return next;
    });
  });

  onProviderQuotaUpdated((quota: ProviderQuota) => {
    providerQuotas.update((list) => {
      const idx = list.findIndex((existingQuota) => isSameQuota(existingQuota, quota));
      if (idx >= 0) {
        const next = [...list];
        next[idx] = mergeAccountQuota(next[idx], quota);
        return next;
      }
      return [...list, quota];
    });

    usageOverview.update((curr) => {
      if (!curr) return curr;
      const existingQuota = curr.quotas.find((sample) => isSameQuota(sample, quota));
      const quotas = curr.quotas.filter((sample) => !isSameQuota(sample, quota));
      return {
        ...curr,
        quotas: [...quotas, existingQuota ? mergeAccountQuota(existingQuota, quota) : quota],
      };
    });
  });
}
