// src/lib/cost.ts

export function formatTokens(n: number): string {
  if (n == null || isNaN(n)) return '0';
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}K`;
  return String(Math.round(n));
}

export function formatTokensLong(n: number): string {
  if (n == null || isNaN(n)) return '0';
  return n.toLocaleString();
}

export interface ModelPricing {
  inputPerM: number;
  cachedInputPerM: number;
  outputPerM: number;
  cacheWritePerM: number;
}

export function getModelPricing(model?: string | null): ModelPricing | null {
  if (!model) return null;
  const lower = model.toLowerCase();
  if (lower.includes('opus')) {
    return { inputPerM: 15.0, cachedInputPerM: 1.5, outputPerM: 75.0, cacheWritePerM: 18.75 };
  } else if (lower.includes('sonnet')) {
    return { inputPerM: 3.0, cachedInputPerM: 0.3, outputPerM: 15.0, cacheWritePerM: 3.75 };
  } else if (lower.includes('haiku')) {
    return { inputPerM: 0.8, cachedInputPerM: 0.08, outputPerM: 4.0, cacheWritePerM: 1.0 };
  } else if (lower.includes('mini')) {
    return { inputPerM: 0.4, cachedInputPerM: 0.2, outputPerM: 1.6, cacheWritePerM: 0.4 };
  } else if (lower.includes('gpt-5') || lower.includes('gpt-6') || lower.includes('codex')) {
    return { inputPerM: 2.5, cachedInputPerM: 1.25, outputPerM: 10.0, cacheWritePerM: 2.5 };
  }
  return null;
}

export function estimateCost(
  input: number,
  output: number,
  cachedInput: number = 0,
  cacheWrite: number = 0,
  model?: string | null
): number {
  const pricing = getModelPricing(model);
  if (!pricing) {
    // Default Claude Sonnet pricing baseline
    return (
      (Math.max(0, input - cachedInput) / 1_000_000) * 3 +
      (cachedInput / 1_000_000) * 0.3 +
      (output / 1_000_000) * 15
    );
  }
  const uncached = Math.max(0, input - cachedInput);
  return (
    (uncached / 1_000_000) * pricing.inputPerM +
    (cachedInput / 1_000_000) * pricing.cachedInputPerM +
    (output / 1_000_000) * pricing.outputPerM +
    (cacheWrite / 1_000_000) * pricing.cacheWritePerM
  );
}

export function formatCost(cost: number | null | undefined): string {
  if (cost == null || isNaN(cost)) return '$0.00';
  if (cost > 0 && cost < 0.01) return '< $0.01';
  return `$${cost.toFixed(2)}`;
}

export function formatPercent(val: number | null | undefined): string {
  if (val == null || isNaN(val)) return '0%';
  const num = val > 1.0 ? val : val * 100;
  return `${Math.round(num)}%`;
}

export function formatTimeRemaining(resetsAt?: number | null): string {
  if (resetsAt == null || resetsAt <= 0) return 'unavailable';
  const now = Math.floor(Date.now() / 1000);
  // Support timestamps in milliseconds or seconds
  const target = resetsAt > 10_000_000_000 ? Math.floor(resetsAt / 1000) : resetsAt;
  const diffSec = target - now;

  if (diffSec <= 0) return 'resets soon';
  const mins = Math.floor(diffSec / 60);
  const hours = Math.floor(mins / 60);
  const days = Math.floor(hours / 24);

  if (days > 0) {
    const d = new Date(target * 1000);
    const dayName = d.toLocaleDateString(undefined, { weekday: 'short' });
    const timeStr = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    return `${dayName} ${timeStr}`;
  }
  if (hours > 0) {
    const remMins = mins % 60;
    return `in ${hours}h ${remMins}m`;
  }
  return `in ${mins}m`;
}

export function getContextStatus(
  percent: number | null | undefined
): 'normal' | 'warning' | 'high' | 'critical' {
  if (percent == null) return 'normal';
  if (percent >= 95) return 'critical';
  if (percent >= 85) return 'high';
  if (percent >= 70) return 'warning';
  return 'normal';
}

export function getQuotaStatus(utilization: number): 'normal' | 'warning' | 'critical' {
  const p = utilization > 1.0 ? utilization : utilization * 100;
  if (p >= 95) return 'critical';
  if (p >= 80) return 'warning';
  return 'normal';
}
