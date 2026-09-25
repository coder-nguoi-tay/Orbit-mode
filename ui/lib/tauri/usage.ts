import type { ProviderQuota, SessionUsageSnapshot, UsageOverview } from '../types';
import { invoke } from './invoke';

export async function getUsageOverview(): Promise<UsageOverview> {
  return invoke<UsageOverview>('get_usage_overview');
}

export async function getProviderQuotas(): Promise<ProviderQuota[]> {
  return invoke<ProviderQuota[]>('get_provider_quotas');
}

export async function getSessionUsages(
  sessionId?: number,
  limit?: number
): Promise<SessionUsageSnapshot[]> {
  return invoke<SessionUsageSnapshot[]>('get_session_usages', {
    sessionId: sessionId ?? null,
    limit: limit ?? null,
  });
}
