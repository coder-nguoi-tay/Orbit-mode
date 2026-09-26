import type { ProviderQuota, SessionUsageSnapshot, UsageOverview } from '../types';
import { invoke } from './invoke';

export async function getUsageOverview(): Promise<UsageOverview> {
  return invoke<UsageOverview>('get_usage_overview');
}

export async function getProviderQuotas(): Promise<ProviderQuota[]> {
  return invoke<ProviderQuota[]>('get_provider_quotas');
}

/** Read live Codex quotas from the CLI instead of waiting for a session event. */
export async function refreshCodexQuotas(): Promise<ProviderQuota[]> {
  return invoke<ProviderQuota[]>('refresh_codex_quotas');
}

/** Run a minimal Claude Code command to read current rate limits. */
export async function refreshClaudeQuotas(): Promise<ProviderQuota[]> {
  return invoke<ProviderQuota[]>('refresh_claude_quotas');
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
