<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Activity,
    Cpu,
    Zap,
    Clock,
    RefreshCw,
    X,
    Layers,
    Search,
    ArrowUpDown,
    Database,
    ShieldAlert,
    ExternalLink,
    HelpCircle,
    ChevronDown,
    ChevronRight,
    TrendingUp,
  } from 'lucide-svelte';
  import Modal from './shared/Modal.svelte';
  import {
    usageOverview,
    providerQuotas,
    sessionUsages,
    isUsageLoading,
    refreshUsageOverview,
    usageCenterOpen,
  } from '../lib/stores/usage';
  import { sessions } from '../lib/stores/sessions';
  import { providerAccounts, refreshProviderAccounts } from '../lib/stores/providerAccounts';
  import { workspace, assignSession } from '../lib/stores/workspace';
  import {
    formatTokens,
    formatCost,
    formatTimeRemaining,
    getContextStatus,
    getQuotaStatus,
    formatPercent,
  } from '../lib/cost';
  import { modelShortName, sessionStatusDotColor } from '../lib/status';
  import type { SessionUsageSnapshot, ProviderQuota } from '../lib/types';
  import { setDefaultProviderAccount } from '../lib/tauri/accounts';

  export let onClose: () => void = () => usageCenterOpen.set(false);

  let searchQuery = '';
  type SortField = 'tokens' | 'context' | 'cost' | 'name';
  let sortField: SortField = 'tokens';
  let sortAsc = false;
  let activeTab: 'agents' | 'projects' | 'models' = 'agents';
  let quotasExpanded = false;

  type TimeRange = '1h' | '6h' | '24h' | '7d' | '30d';
  let selectedTimeRange: TimeRange = '24h';
  let hoveredPointIndex: number | null = null;
  let svgElement: SVGSVGElement | null = null;

  onMount(async () => {
    await refreshProviderAccounts();
    await refreshUsageOverview();
  });

  /** Set this account as the default so new sessions use its quota. */
  async function selectActiveAccount(accountId: string): Promise<void> {
    try {
      await setDefaultProviderAccount(accountId);
      await refreshProviderAccounts();
    } catch {
      /* backend rejected — UI stays unchanged */
    }
  }

  function setSort(field: SortField) {
    if (sortField === field) {
      sortAsc = !sortAsc;
    } else {
      sortField = field;
      sortAsc = false;
    }
  }

  // Merge persisted quotas with the latest rate-limit state from active sessions.
  let codexQuotas: ProviderQuota[] = [];
  let claudeQuota: ProviderQuota | null = null;
  $: {
    const quotasByAccount = new Map<string, ProviderQuota>();
    for (const quota of $providerQuotas) {
      quotasByAccount.set(
        `${quota.provider}:${quota.providerAccountId ?? quota.accountKey}`,
        quota
      );
    }
    for (const session of $sessions) {
      if (!session.rateLimit?.length) continue;
      const provider = session.provider ?? 'claude-code';
      if (provider !== 'codex' && provider !== 'claude-code' && provider !== 'claude') continue;
      const accountKey = session.providerAccountId ?? 'default';
      const fiveHourLimit = session.rateLimit.find((limit) => limit.rateLimitType === 'five_hour');
      const sevenDayLimit = session.rateLimit.find((limit) => limit.rateLimitType === 'seven_day');
      const liveQuota: ProviderQuota = {
        provider,
        accountKey,
        providerAccountId: session.providerAccountId,
        fiveHour: fiveHourLimit
          ? {
              utilization: fiveHourLimit.utilization,
              resetsAt: fiveHourLimit.resetsAt,
              status: fiveHourLimit.status,
            }
          : null,
        sevenDay: sevenDayLimit
          ? {
              utilization: sevenDayLimit.utilization,
              resetsAt: sevenDayLimit.resetsAt,
              status: sevenDayLimit.status,
            }
          : null,
        updatedAt: session.updatedAt,
        source: 'session_event',
      };
      const quotaKey = `${provider}:${accountKey}`;
      const persistedQuota = quotasByAccount.get(quotaKey);
      quotasByAccount.set(
        quotaKey,
        persistedQuota
          ? {
              ...persistedQuota,
              providerAccountId: persistedQuota.providerAccountId ?? liveQuota.providerAccountId,
              fiveHour: persistedQuota.fiveHour ?? liveQuota.fiveHour,
              sevenDay: persistedQuota.sevenDay ?? liveQuota.sevenDay,
            }
          : liveQuota
      );
    }
    codexQuotas = [...quotasByAccount.values()].filter((quota) => quota.provider === 'codex');
    claudeQuota =
      [...quotasByAccount.values()].find(
        (q) => q.provider === 'claude-code' || q.provider === 'claude'
      ) ??
      $providerQuotas.find(
        (q: ProviderQuota) => q.provider === 'claude-code' || q.provider === 'claude'
      ) ??
      null;
  }
  $: codexQuotaCards = (() => {
    const codexAccounts = Object.values($providerAccounts).filter(
      (account) => account.providerId === 'codex'
    );
    if (codexAccounts.length === 0) {
      if (codexQuotas.length > 0) {
        return codexQuotas;
      }
      return [
        {
          provider: 'codex',
          accountKey: 'default',
          providerAccountId: null,
          fiveHour: null,
          sevenDay: null,
          updatedAt: new Date().toISOString(),
          source: 'default',
        },
      ];
    }
    return codexAccounts.map((account) => {
      const exactMatch = codexQuotas.find(
        (quota) => quota.providerAccountId === account.id || quota.accountKey === account.id
      );
      const defaultMatch = account.isDefault
        ? codexQuotas.find((quota) => quota.accountKey === 'default' || !quota.providerAccountId)
        : undefined;
      const matched = exactMatch ?? defaultMatch;
      return (
        matched ?? {
          provider: 'codex',
          accountKey: account.id,
          providerAccountId: account.id,
          fiveHour: null,
          sevenDay: null,
          updatedAt: account.updatedAt,
          source: account.status,
        }
      );
    });
  })();
  // Combine live sessions and snapshots
  $: combinedAgents = (() => {
    const list: Array<{
      sessionId: number;
      name: string;
      projectName: string;
      provider: string;
      providerAccountId: string | null;
      model: string;
      status: string;
      tokens: number;
      contextPercent: number;
      contextTokens: number | null;
      contextLimit: number | null;
      cachedTokens: number;
      reasoningTokens: number;
      outputTokens: number;
      cost: number | null;
      updatedAt: string;
    }> = [];

    const snapshotMap = $sessionUsages;

    for (const s of $sessions) {
      const snap = snapshotMap.get(s.id);
      const totalTok = snap?.totalTokens ?? (s.tokens?.input ?? 0) + (s.tokens?.output ?? 0);
      const ctxPct = snap?.contextPercent ?? s.contextPercent ?? 0;
      const estimatedCost = snap?.estimatedCostUsd ?? s.costUsd ?? null;

      list.push({
        sessionId: s.id,
        name: s.name ?? `#${s.id}`,
        projectName: s.projectName ?? 'Default',
        provider: s.provider ?? 'unknown',
        providerAccountId: snap?.providerAccountId ?? s.providerAccountId ?? null,
        model: s.model ?? 'unknown',
        status: s.status,
        tokens: totalTok,
        contextPercent: ctxPct,
        contextTokens: snap?.contextTokens ?? null,
        contextLimit: snap?.contextLimit ?? s.contextWindow ?? null,
        cachedTokens: snap?.cachedInputTokens ?? s.tokens?.cacheRead ?? 0,
        reasoningTokens: snap?.reasoningTokens ?? 0,
        outputTokens: snap?.outputTokens ?? s.tokens?.output ?? 0,
        cost: estimatedCost,
        updatedAt: snap?.createdAt ?? new Date().toISOString(),
      });
    }

    return list;
  })();

  $: filteredAgents = combinedAgents
    .filter((a) => {
      if (!searchQuery) return true;
      const q = searchQuery.toLowerCase();
      return (
        a.name.toLowerCase().includes(q) ||
        a.projectName.toLowerCase().includes(q) ||
        a.provider.toLowerCase().includes(q) ||
        ($providerAccounts[a.providerAccountId ?? '']?.label ?? '').toLowerCase().includes(q) ||
        a.model.toLowerCase().includes(q)
      );
    })
    .sort((a, b) => {
      let cmp = 0;
      if (sortField === 'tokens') cmp = a.tokens - b.tokens;
      else if (sortField === 'context') cmp = a.contextPercent - b.contextPercent;
      else if (sortField === 'cost') cmp = (a.cost ?? 0) - (b.cost ?? 0);
      else if (sortField === 'name') cmp = a.name.localeCompare(b.name);
      return sortAsc ? cmp : -cmp;
    });

  function openAgentSession(sessionId: number): void {
    const ws = $workspace;
    if (ws.focusedPaneId) {
      assignSession(ws.focusedPaneId, sessionId);
    }
    onClose();
  }

  function getQuotaColorClass(utilization: number): string {
    const status = getQuotaStatus(utilization);
    return `quota-${status}`;
  }

  function getContextColorClass(pct: number): string {
    const status = getContextStatus(pct);
    return `ctx-${status}`;
  }

  const tokenChart = {
    width: 900,
    height: 220,
    padding: { top: 22, right: 28, bottom: 32, left: 52 },
  };

  interface ChartBucket {
    bucketStart: string;
    totalTokens: number;
    displayDate: string;
    displayTime: string;
    detailLabel: string;
  }

  $: rawTokenHistory = $usageOverview?.tokenUsageHistory ?? [];

  $: tokenUsageHistory = (() => {
    const now = new Date();
    const buckets: ChartBucket[] = [];

    if (selectedTimeRange === '30d') {
      // 30 discrete daily buckets (from 29 days ago to today)
      for (let i = 29; i >= 0; i--) {
        const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i, 0, 0, 0, 0);
        const nextD = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i + 1, 0, 0, 0, 0);
        const startTime = d.getTime();
        const endTime = nextD.getTime();

        const sum = rawTokenHistory
          .filter((pt) => {
            const ptTime = new Date(pt.bucketStart).getTime();
            return ptTime >= startTime && ptTime < endTime;
          })
          .reduce((acc, pt) => acc + pt.totalTokens, 0);

        const isToday = i === 0;
        const displayDate = d.toLocaleDateString([], { month: 'short', day: 'numeric' });
        const detailLabel = `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' })}${isToday ? ' (Today)' : ''}`;

        buckets.push({
          bucketStart: d.toISOString(),
          totalTokens: sum,
          displayDate,
          displayTime: isToday ? 'Today' : displayDate,
          detailLabel,
        });
      }
    } else if (selectedTimeRange === '7d') {
      // 7 discrete daily buckets (from 6 days ago to today)
      for (let i = 6; i >= 0; i--) {
        const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i, 0, 0, 0, 0);
        const nextD = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i + 1, 0, 0, 0, 0);
        const startTime = d.getTime();
        const endTime = nextD.getTime();

        const sum = rawTokenHistory
          .filter((pt) => {
            const ptTime = new Date(pt.bucketStart).getTime();
            return ptTime >= startTime && ptTime < endTime;
          })
          .reduce((acc, pt) => acc + pt.totalTokens, 0);

        const isToday = i === 0;
        const displayDate = d.toLocaleDateString([], { weekday: 'short', month: 'numeric', day: 'numeric' });
        const detailLabel = `${d.toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric', year: 'numeric' })}${isToday ? ' (Today)' : ''}`;

        buckets.push({
          bucketStart: d.toISOString(),
          totalTokens: sum,
          displayDate,
          displayTime: isToday ? 'Today' : displayDate,
          detailLabel,
        });
      }
    } else if (selectedTimeRange === '24h') {
      // 24 discrete hourly buckets
      const curHour = new Date(now.getFullYear(), now.getMonth(), now.getDate(), now.getHours(), 0, 0, 0);
      for (let i = 23; i >= 0; i--) {
        const startH = new Date(curHour.getTime() - i * 3600000);
        const endH = new Date(startH.getTime() + 3600000);
        const startTime = startH.getTime();
        const endTime = endH.getTime();

        const sum = rawTokenHistory
          .filter((pt) => {
            const ptTime = new Date(pt.bucketStart).getTime();
            return ptTime >= startTime && ptTime < endTime;
          })
          .reduce((acc, pt) => acc + pt.totalTokens, 0);

        const isCurrentHour = i === 0;
        const displayTime = startH.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        const displayDate = startH.toLocaleDateString([], { month: 'short', day: 'numeric' });
        const detailLabel = `${displayDate} · ${displayTime}${isCurrentHour ? ' (Now)' : ''}`;

        buckets.push({
          bucketStart: startH.toISOString(),
          totalTokens: sum,
          displayDate,
          displayTime,
          detailLabel,
        });
      }
    } else if (selectedTimeRange === '6h') {
      // 6 discrete hourly buckets
      const curHour = new Date(now.getFullYear(), now.getMonth(), now.getDate(), now.getHours(), 0, 0, 0);
      for (let i = 5; i >= 0; i--) {
        const startH = new Date(curHour.getTime() - i * 3600000);
        const endH = new Date(startH.getTime() + 3600000);
        const startTime = startH.getTime();
        const endTime = endH.getTime();

        const sum = rawTokenHistory
          .filter((pt) => {
            const ptTime = new Date(pt.bucketStart).getTime();
            return ptTime >= startTime && ptTime < endTime;
          })
          .reduce((acc, pt) => acc + pt.totalTokens, 0);

        const isCurrentHour = i === 0;
        const displayTime = startH.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        const displayDate = startH.toLocaleDateString([], { month: 'short', day: 'numeric' });
        const detailLabel = `${displayDate} · ${displayTime}${isCurrentHour ? ' (Now)' : ''}`;

        buckets.push({
          bucketStart: startH.toISOString(),
          totalTokens: sum,
          displayDate,
          displayTime,
          detailLabel,
        });
      }
    } else {
      // 1h: 12 discrete 5-minute buckets
      const cur5Min = new Date(Math.floor(now.getTime() / (5 * 60000)) * 5 * 60000);
      for (let i = 11; i >= 0; i--) {
        const startM = new Date(cur5Min.getTime() - i * 5 * 60000);
        const endM = new Date(startM.getTime() + 5 * 60000);
        const startTime = startM.getTime();
        const endTime = endM.getTime();

        const sum = rawTokenHistory
          .filter((pt) => {
            const ptTime = new Date(pt.bucketStart).getTime();
            return ptTime >= startTime && ptTime < endTime;
          })
          .reduce((acc, pt) => acc + pt.totalTokens, 0);

        const isCurrent = i === 0;
        const displayTime = startM.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        const detailLabel = `${startM.toLocaleDateString([], { month: 'short', day: 'numeric' })} · ${displayTime}${isCurrent ? ' (Now)' : ''}`;

        buckets.push({
          bucketStart: startM.toISOString(),
          totalTokens: sum,
          displayDate: displayTime,
          displayTime,
          detailLabel,
        });
      }
    }

    return buckets;
  })();

  $: rangeTotalTokens = tokenUsageHistory.reduce((sum, pt) => sum + pt.totalTokens, 0);
  $: tokenHistoryMax = Math.max(1, ...tokenUsageHistory.map((point) => point.totalTokens));

  $: axisTicks = (() => {
    if (tokenUsageHistory.length === 0) return [];
    const len = tokenUsageHistory.length;
    let indices: number[] = [];

    if (selectedTimeRange === '30d') {
      indices = [0, 6, 12, 18, 24, len - 1];
    } else if (selectedTimeRange === '7d') {
      indices = [0, 1, 2, 3, 4, 5, len - 1];
    } else if (selectedTimeRange === '24h') {
      indices = [0, 6, 12, 18, len - 1];
    } else if (selectedTimeRange === '6h') {
      indices = [0, 1, 2, 3, 4, len - 1];
    } else {
      indices = [0, 3, 6, 9, len - 1];
    }

    return indices.map((idx, i) => {
      const pt = tokenUsageHistory[idx];
      const isLast = i === indices.length - 1;
      let label = pt?.displayTime ?? '';
      if (isLast) {
        label = selectedTimeRange === '30d' || selectedTimeRange === '7d' ? 'TODAY' : 'NOW';
      }
      return { idx, label };
    });
  })();

  $: tokenChartPoints = (() => {
    const plotWidth = tokenChart.width - tokenChart.padding.left - tokenChart.padding.right;
    const plotHeight = tokenChart.height - tokenChart.padding.top - tokenChart.padding.bottom;

    if (tokenUsageHistory.length === 0) {
      return [];
    }

    const n = tokenUsageHistory.length;
    const points = tokenUsageHistory.map((point, index) => {
      const progress = n > 1 ? index / (n - 1) : 0.5;
      const x = tokenChart.padding.left + progress * plotWidth;
      const y =
        tokenChart.padding.top +
        (1 - point.totalTokens / tokenHistoryMax) * plotHeight;
      return { ...point, x, y, index };
    });

    return points;
  })();

  /** Catmull-Rom to Cubic Bezier smooth spline generator */
  function generateSmoothPath(points: Array<{ x: number; y: number }>): string {
    if (points.length === 0) return '';
    if (points.length === 1) return `M ${points[0].x.toFixed(2)},${points[0].y.toFixed(2)}`;
    if (points.length === 2) {
      return `M ${points[0].x.toFixed(2)},${points[0].y.toFixed(2)} L ${points[1].x.toFixed(2)},${points[1].y.toFixed(2)}`;
    }

    let path = `M ${points[0].x.toFixed(2)},${points[0].y.toFixed(2)}`;
    for (let i = 0; i < points.length - 1; i++) {
      const p0 = points[Math.max(0, i - 1)];
      const p1 = points[i];
      const p2 = points[i + 1];
      const p3 = points[Math.min(points.length - 1, i + 2)];

      const cp1x = p1.x + (p2.x - p0.x) / 6;
      const cp1y = p1.y + (p2.y - p0.y) / 6;
      const cp2x = p2.x - (p3.x - p1.x) / 6;
      const cp2y = p2.y - (p3.y - p1.y) / 6;

      path += ` C ${cp1x.toFixed(2)},${cp1y.toFixed(2)} ${cp2x.toFixed(2)},${cp2y.toFixed(2)} ${p2.x.toFixed(2)},${p2.y.toFixed(2)}`;
    }
    return path;
  }

  $: smoothCurvePath = generateSmoothPath(tokenChartPoints);
  $: smoothAreaPath = (() => {
    if (tokenChartPoints.length === 0) return '';
    const baselineY = tokenChart.height - tokenChart.padding.bottom;
    const last = tokenChartPoints[tokenChartPoints.length - 1];
    const first = tokenChartPoints[0];
    return `${smoothCurvePath} L ${last.x.toFixed(2)},${baselineY} L ${first.x.toFixed(2)},${baselineY} Z`;
  })();

  function handleChartMouseMove(event: MouseEvent) {
    if (!svgElement || tokenChartPoints.length === 0) return;
    const rect = svgElement.getBoundingClientRect();
    const mouseSvgX = ((event.clientX - rect.left) / rect.width) * tokenChart.width;

    let closestIdx = 0;
    let closestDist = Infinity;
    for (let i = 0; i < tokenChartPoints.length; i++) {
      const dist = Math.abs(tokenChartPoints[i].x - mouseSvgX);
      if (dist < closestDist) {
        closestDist = dist;
        closestIdx = i;
      }
    }
    hoveredPointIndex = closestIdx;
  }

  function handleChartMouseLeave() {
    hoveredPointIndex = null;
  }
</script>

<Modal
  width="min(1360px, 94vw)"
  zIndex={700}
  overlayBg="rgba(0, 0, 0, 0.75)"
  modalStyle="max-height: 90vh; height: 86vh; background: var(--bg1, #0f1412); border: 1px solid var(--bd, #27322b); overflow: hidden; box-shadow: 0 20px 60px rgba(0,0,0,0.85); gap: 0; padding: 0; border-radius: 8px; display: flex; flex-direction: column;"
  on:close={onClose}
>
  <!-- Header -->
  <div class="control-header">
    <div class="header-left">
      <div class="header-icon">
        <Activity size={18} />
      </div>
      <div>
        <div class="header-title-row">
          <span class="header-title">AGENT USAGE CONTROL CENTER</span>
          <span class="header-badge">REALTIME</span>
        </div>
        <div class="header-sub">
          Multi-agent monitoring, provider quota tracking & cost intelligence
        </div>
      </div>
    </div>
    <div class="header-right">
      <button
        class="action-btn"
        class:spinning={$isUsageLoading}
        on:click={refreshUsageOverview}
        title="Refresh usage data"
        aria-label="Refresh usage data"
      >
        <RefreshCw size={14} />
        <span>Refresh</span>
      </button>
      <button class="close-btn" on:click={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>
  </div>

  <!-- Quick KPI Banner -->
  <div class="kpi-banner">
    <div class="kpi-card">
      <div class="kpi-icon"><Zap size={15} /></div>
      <div class="kpi-content">
        <div class="kpi-label">TODAY TOKENS</div>
        <div class="kpi-value">{formatTokens($usageOverview?.totalTokensToday ?? 0)}</div>
      </div>
    </div>
    <div class="kpi-card">
      <div class="kpi-icon"><Database size={15} /></div>
      <div class="kpi-content">
        <div class="kpi-label">ESTIMATED COST</div>
        <div class="kpi-value">
          {formatCost($usageOverview?.totalCostToday ?? null)}
        </div>
      </div>
    </div>
    <div class="kpi-card">
      <div class="kpi-icon"><Cpu size={15} /></div>
      <div class="kpi-content">
        <div class="kpi-label">ACTIVE AGENTS</div>
        <div class="kpi-value">
          {combinedAgents.filter((a) => a.status === 'working' || a.status === 'input').length} / {combinedAgents.length}
        </div>
      </div>
    </div>
    <div class="kpi-card">
      <div class="kpi-icon"><ShieldAlert size={15} /></div>
      <div class="kpi-content">
        <div class="kpi-label">NEAR LIMIT AGENTS</div>
        <div class="kpi-value warn-value">
          {combinedAgents.filter((a) => a.contextPercent >= 80).length}
        </div>
      </div>
    </div>
  </div>

  <div class="content-scroll">
    <!-- Hourly / Time-ranged token history with smooth curves & interactive tooltips -->
    <section class="token-history-card" aria-label="Token usage over time">
      <div class="token-history-header">
        <div class="token-history-title-block">
          <div class="section-title">
            <TrendingUp size={15} />
            <span>TOKEN USAGE OVER TIME</span>
          </div>
          <div class="token-history-subtitle">
            {#if selectedTimeRange === '1h'}
              Last 1 hour · minute resolution
            {:else if selectedTimeRange === '6h'}
              Last 6 hours · token volume per hour
            {:else if selectedTimeRange === '24h'}
              Last 24 hours · new tokens per hour
            {:else if selectedTimeRange === '7d'}
              Last 7 days · daily token aggregates
            {:else}
              Last 30 days · monthly token aggregates
            {/if}
          </div>
        </div>

        <div class="token-history-controls">
          <!-- Time range selector -->
          <div class="time-range-group" role="radiogroup" aria-label="Select time range">
            <button
              class="time-range-btn"
              class:active={selectedTimeRange === '1h'}
              on:click={() => (selectedTimeRange = '1h')}
            >
              1H
            </button>
            <button
              class="time-range-btn"
              class:active={selectedTimeRange === '6h'}
              on:click={() => (selectedTimeRange = '6h')}
            >
              6H
            </button>
            <button
              class="time-range-btn"
              class:active={selectedTimeRange === '24h'}
              on:click={() => (selectedTimeRange = '24h')}
            >
              24H
            </button>
            <button
              class="time-range-btn"
              class:active={selectedTimeRange === '7d'}
              on:click={() => (selectedTimeRange = '7d')}
            >
              7D
            </button>
            <button
              class="time-range-btn"
              class:active={selectedTimeRange === '30d'}
              on:click={() => (selectedTimeRange = '30d')}
            >
              30D
            </button>
          </div>

          <!-- Total & Peak chip badges -->
          <div class="token-stat-chips">
            <div class="token-stat-chip total">
              <span class="chip-label">Total</span>
              <span class="chip-val">{formatTokens(rangeTotalTokens)}</span>
            </div>
            <div class="token-stat-chip peak">
              <span class="chip-label">Peak</span>
              <span class="chip-val">{formatTokens(tokenHistoryMax)}</span>
            </div>
          </div>
        </div>
      </div>

      {#if tokenChartPoints.length > 0}
        {@const midY = tokenChart.padding.top + (tokenChart.height - tokenChart.padding.top - tokenChart.padding.bottom) / 2}
        {@const bottomY = tokenChart.height - tokenChart.padding.bottom}
        {@const topY = tokenChart.padding.top}
        {@const hoveredPoint = hoveredPointIndex !== null && tokenChartPoints[hoveredPointIndex] ? tokenChartPoints[hoveredPointIndex] : null}

        <div class="token-chart-wrap">
          <svg
            bind:this={svgElement}
            class="token-chart"
            viewBox={`0 0 ${tokenChart.width} ${tokenChart.height}`}
            role="img"
            aria-label="Token usage chart"
            on:mousemove={handleChartMouseMove}
            on:mouseleave={handleChartMouseLeave}
          >
            <defs>
              <linearGradient id="tokenSmoothGradient" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stop-color="#00d47e" stop-opacity="0.32" />
                <stop offset="45%" stop-color="#00d47e" stop-opacity="0.10" />
                <stop offset="100%" stop-color="#00d47e" stop-opacity="0.0" />
              </linearGradient>

              <filter id="neonCurveGlow" x="-20%" y="-20%" width="140%" height="140%">
                <feGaussianBlur stdDeviation="2.5" result="blur" />
                <feMerge>
                  <feMergeNode in="blur" />
                  <feMergeNode in="SourceGraphic" />
                </feMerge>
              </filter>
            </defs>

            <!-- Horizontal Guide Lines -->
            <line
              class="token-chart-grid"
              x1={tokenChart.padding.left}
              y1={topY}
              x2={tokenChart.width - tokenChart.padding.right}
              y2={topY}
            />
            <line
              class="token-chart-grid mid"
              x1={tokenChart.padding.left}
              y1={midY}
              x2={tokenChart.width - tokenChart.padding.right}
              y2={midY}
            />
            <line
              class="token-chart-grid"
              x1={tokenChart.padding.left}
              y1={bottomY}
              x2={tokenChart.width - tokenChart.padding.right}
              y2={bottomY}
            />

            <!-- Y Axis text labels -->
            <text class="token-chart-y-label" x="4" y={topY + 4}>
              {formatTokens(tokenHistoryMax)}
            </text>
            <text class="token-chart-y-label mid" x="4" y={midY + 4}>
              {formatTokens(Math.round(tokenHistoryMax / 2))}
            </text>
            <text class="token-chart-y-label" x="30" y={bottomY + 4}>0</text>

            <!-- Gradient Area Fill below curve -->
            <path class="token-chart-smooth-area" d={smoothAreaPath} />

            <!-- Smooth Bezier Spline Curve -->
            <path class="token-chart-smooth-line" d={smoothCurvePath} />

            <!-- Discrete Checkpoint Data Points -->
            {#each tokenChartPoints as point (point.bucketStart)}
              <circle
                class="token-chart-point-subtle"
                class:has-tokens={point.totalTokens > 0}
                cx={point.x}
                cy={point.y}
                r={point.totalTokens > 0 ? 3 : 1.5}
              />
            {/each}

            <!-- Interactive Hover Effects -->
            {#if hoveredPoint}
              <!-- Vertical Crosshair line -->
              <line
                class="hover-crosshair"
                x1={hoveredPoint.x}
                y1={topY}
                x2={hoveredPoint.x}
                y2={bottomY}
              />

              <!-- Outer Glowing Halo Ring -->
              <circle
                class="hover-halo"
                cx={hoveredPoint.x}
                cy={hoveredPoint.y}
                r="7"
              />

              <!-- Inner Active Dot -->
              <circle
                class="hover-point-active"
                cx={hoveredPoint.x}
                cy={hoveredPoint.y}
                r="3.5"
              />

              <!-- Interactive Floating Tooltip Bubble inside SVG -->
              {@const tooltipWidth = 175}
              {@const tooltipHeight = 46}
              {@const tooltipX = Math.min(
                tokenChart.width - tokenChart.padding.right - tooltipWidth,
                Math.max(tokenChart.padding.left, hoveredPoint.x - tooltipWidth / 2)
              )}
              {@const tooltipY = Math.max(
                4,
                hoveredPoint.y - tooltipHeight - 12 < topY
                  ? hoveredPoint.y + 14
                  : hoveredPoint.y - tooltipHeight - 12
              )}

              <g class="chart-tooltip-group" transform={`translate(${tooltipX}, ${tooltipY})`}>
                <rect
                  class="chart-tooltip-bg"
                  width={tooltipWidth}
                  height={tooltipHeight}
                  rx="6"
                />
                <text class="chart-tooltip-time" x="10" y="17">
                  {hoveredPoint.detailLabel}
                </text>
                <text class="chart-tooltip-tokens" x="10" y="34">
                  {formatTokens(hoveredPoint.totalTokens)} tokens
                </text>
              </g>
            {/if}
          </svg>
        </div>

        <div class="token-chart-axis">
          {#each axisTicks as tick}
            <span>{tick.label}</span>
          {/each}
        </div>
      {:else}
        <div class="token-chart-empty">
          No token usage recorded in the selected time frame ({selectedTimeRange.toUpperCase()}).
        </div>
      {/if}
    </section>

    <!-- Provider Quota Overview (Collapsible to save vertical space) -->
    <div class="section-container">
      <button
        class="section-header-btn"
        on:click={() => (quotasExpanded = !quotasExpanded)}
        aria-expanded={quotasExpanded}
        type="button"
      >
        <div class="section-title">
          <Layers size={14} />
          <span>PROVIDER & ACCOUNT QUOTAS</span>
          <span class="quota-count-badge">{codexQuotaCards.length + 1}</span>
        </div>
        <div class="section-header-right">
          <span class="section-hint">
            {quotasExpanded
              ? 'Click to collapse quotas'
              : 'Click to view active quotas across accounts'}
          </span>
          <span class="collapse-icon">
            {#if quotasExpanded}
              <ChevronDown size={14} />
            {:else}
              <ChevronRight size={14} />
            {/if}
          </span>
        </div>
      </button>

      {#if quotasExpanded}
        <div class="quota-grid">
          <!-- Each Codex profile has its own quota; never combine shared account limits. -->
          {#each codexQuotaCards as codexQuota (codexQuota.providerAccountId ?? codexQuota.accountKey)}
            {@const codexAccount = $providerAccounts[codexQuota.providerAccountId ?? '']}
            <div class="quota-card">
              <div class="quota-card-header">
                <div class="provider-badge codex">
                  <span class="dot">●</span>
                  <span
                    >CODEX · {$providerAccounts[codexQuota.providerAccountId ?? '']?.label ??
                      codexQuota.accountKey}</span
                  >
                </div>
                <div class="quota-card-actions">
                  <span class="source-tag">{codexQuota?.source ?? 'local'}</span>
                  {#if codexAccount && codexAccount.id !== 'codex-system-default'}
                    <label
                      class="auto-handoff-toggle"
                      title="Use this account's quota for new sessions"
                    >
                      <input
                        type="checkbox"
                        checked={codexAccount.isDefault}
                        disabled={!['available', 'busy', 'near_limit'].includes(
                          codexAccount.status
                        )}
                        on:change={() => selectActiveAccount(codexAccount.id)}
                      />
                      active
                    </label>
                  {/if}
                </div>
              </div>

              {#if $providerAccounts[codexQuota.providerAccountId ?? '']?.authType === 'api_key'}
                <p class="reset-meta">
                  API billing and rate limits are separate from ChatGPT allowance.
                </p>
              {:else}
                <!-- 5-hour window -->
                <div class="quota-row">
                  <div class="quota-meta">
                    <span class="quota-label">5-Hour Usage</span>
                    <span
                      class="quota-pct {codexQuota?.fiveHour
                        ? getQuotaColorClass(codexQuota.fiveHour.utilization)
                        : ''}"
                    >
                      {codexQuota?.fiveHour
                        ? formatPercent(codexQuota.fiveHour.utilization)
                        : 'unavailable'}
                    </span>
                  </div>
                  <div class="progress-track">
                    <div
                      class="progress-fill {codexQuota?.fiveHour
                        ? getQuotaColorClass(codexQuota.fiveHour.utilization)
                        : ''}"
                      style="width: {codexQuota?.fiveHour
                        ? Math.min(100, Math.max(0, codexQuota.fiveHour.utilization * 100))
                        : 0}%"
                    ></div>
                  </div>
                  <div class="reset-meta">
                    <Clock size={11} />
                    <span
                      >{codexQuota?.fiveHour?.resetsAt
                        ? `Resets ${formatTimeRemaining(codexQuota.fiveHour.resetsAt)}`
                        : 'Reset time pending'}</span
                    >
                  </div>
                </div>

                <!-- 7-day window -->
                <div class="quota-row">
                  <div class="quota-meta">
                    <span class="quota-label">7-Day Usage</span>
                    <span
                      class="quota-pct {codexQuota?.sevenDay
                        ? getQuotaColorClass(codexQuota.sevenDay.utilization)
                        : ''}"
                    >
                      {codexQuota?.sevenDay
                        ? formatPercent(codexQuota.sevenDay.utilization)
                        : 'unavailable'}
                    </span>
                  </div>
                  <div class="progress-track">
                    <div
                      class="progress-fill {codexQuota?.sevenDay
                        ? getQuotaColorClass(codexQuota.sevenDay.utilization)
                        : ''}"
                      style="width: {codexQuota?.sevenDay
                        ? Math.min(100, Math.max(0, codexQuota.sevenDay.utilization * 100))
                        : 0}%"
                    ></div>
                  </div>
                  <div class="reset-meta">
                    <Clock size={11} />
                    <span
                      >{codexQuota?.sevenDay?.resetsAt
                        ? `Resets ${formatTimeRemaining(codexQuota.sevenDay.resetsAt)}`
                        : 'Reset time pending'}</span
                    >
                  </div>
                </div>
              {/if}
            </div>
          {/each}

          <!-- Claude Quota Card -->
          <div class="quota-card">
            <div class="quota-card-header">
              <div class="provider-badge claude">
                <span class="dot">●</span>
                <span>CLAUDE CODE (Anthropic)</span>
              </div>
              <span class="source-tag">{claudeQuota?.source ?? 'session'}</span>
            </div>

            <!-- 5-hour window -->
            <div class="quota-row">
              <div class="quota-meta">
                <span class="quota-label">5-Hour Usage</span>
                <span
                  class="quota-pct {claudeQuota?.fiveHour
                    ? getQuotaColorClass(claudeQuota.fiveHour.utilization)
                    : ''}"
                >
                  {claudeQuota?.fiveHour
                    ? formatPercent(claudeQuota.fiveHour.utilization)
                    : 'Reported by CLI'}
                </span>
              </div>
              <div class="progress-track">
                <div
                  class="progress-fill {claudeQuota?.fiveHour
                    ? getQuotaColorClass(claudeQuota.fiveHour.utilization)
                    : ''}"
                  style="width: {claudeQuota?.fiveHour
                    ? Math.min(100, Math.max(0, claudeQuota.fiveHour.utilization * 100))
                    : 0}%"
                ></div>
              </div>
              <div class="reset-meta">
                <Clock size={11} />
                <span
                  >{claudeQuota?.fiveHour?.resetsAt
                    ? `Resets ${formatTimeRemaining(claudeQuota.fiveHour.resetsAt)}`
                    : 'Monitored per session'}</span
                >
              </div>
            </div>

            <!-- 7-day window -->
            <div class="quota-row">
              <div class="quota-meta">
                <span class="quota-label">7-Day Usage</span>
                <span
                  class="quota-pct {claudeQuota?.sevenDay
                    ? getQuotaColorClass(claudeQuota.sevenDay.utilization)
                    : ''}"
                >
                  {claudeQuota?.sevenDay
                    ? formatPercent(claudeQuota.sevenDay.utilization)
                    : 'Standard tier'}
                </span>
              </div>
              <div class="progress-track">
                <div
                  class="progress-fill {claudeQuota?.sevenDay
                    ? getQuotaColorClass(claudeQuota.sevenDay.utilization)
                    : ''}"
                  style="width: {claudeQuota?.sevenDay
                    ? Math.min(100, Math.max(0, claudeQuota.sevenDay.utilization * 100))
                    : 0}%"
                ></div>
              </div>
              <div class="reset-meta">
                <Clock size={11} />
                <span
                  >{claudeQuota?.sevenDay?.resetsAt
                    ? `Resets ${formatTimeRemaining(claudeQuota.sevenDay.resetsAt)}`
                    : 'Monitored per session'}</span
                >
              </div>
            </div>
          </div>
        </div>
      {/if}
    </div>

    <!-- Navigation Tabs -->
    <div class="tabs-row">
      <div class="tabs-group">
        <button
          class="tab-btn"
          class:active={activeTab === 'agents'}
          on:click={() => (activeTab = 'agents')}
        >
          Active Agents ({filteredAgents.length})
        </button>
        <button
          class="tab-btn"
          class:active={activeTab === 'projects'}
          on:click={() => (activeTab = 'projects')}
        >
          By Project ({$usageOverview?.projectSummaries?.length ?? 0})
        </button>
        <button
          class="tab-btn"
          class:active={activeTab === 'models'}
          on:click={() => (activeTab = 'models')}
        >
          By Model ({$usageOverview?.modelSummaries?.length ?? 0})
        </button>
      </div>

      {#if activeTab === 'agents'}
        <div class="search-box">
          <Search size={13} />
          <input
            type="text"
            bind:value={searchQuery}
            placeholder="Filter agents, projects, models…"
            aria-label="Filter agents"
          />
          {#if searchQuery}
            <button class="clear-search" on:click={() => (searchQuery = '')}>✕</button>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Tab 1: Agents Ranking Table -->
    {#if activeTab === 'agents'}
      <div class="table-container">
        <table class="usage-table">
          <thead>
            <tr>
              <th on:click={() => setSort('name')} class="sortable col-name">
                <div class="th-content">
                  <span>Project / Agent</span>
                  <ArrowUpDown size={11} />
                </div>
              </th>
              <th class="col-provider">Provider</th>
              <th class="col-account">Account</th>
              <th class="col-model">Model</th>
              <th on:click={() => setSort('context')} class="sortable col-context">
                <div class="th-content">
                  <span>Context Window</span>
                  <ArrowUpDown size={11} />
                </div>
              </th>
              <th on:click={() => setSort('tokens')} class="sortable col-tokens text-right">
                <div class="th-content right">
                  <span>Tokens</span>
                  <ArrowUpDown size={11} />
                </div>
              </th>
              <th on:click={() => setSort('cost')} class="sortable col-cost text-right">
                <div class="th-content right">
                  <span>Cost (Est)</span>
                  <ArrowUpDown size={11} />
                </div>
              </th>
              <th class="col-status text-center">Status</th>
              <th class="col-action text-center"></th>
            </tr>
          </thead>
          <tbody>
            {#if filteredAgents.length === 0}
              <tr>
                <td colspan="9" class="empty-state">No agent sessions found matching filter.</td>
              </tr>
            {:else}
              {#each filteredAgents as agent (agent.sessionId)}
                {@const dotColor = sessionStatusDotColor(agent.status, null)}
                <tr class="agent-row" on:click={() => openAgentSession(agent.sessionId)}>
                  <td class="col-name">
                    <div class="project-agent">
                      <span class="p-name">{agent.projectName}</span>
                      <span class="sep">/</span>
                      <span class="a-name">{agent.name}</span>
                    </div>
                  </td>
                  <td class="col-provider">
                    <span class="provider-pill {agent.provider}">{agent.provider}</span>
                  </td>
                  <td class="col-account">
                    <span class="account-text"
                      >{$providerAccounts[agent.providerAccountId ?? '']?.label ??
                        'Unassigned'}</span
                    >
                  </td>
                  <td class="col-model">
                    <span class="model-text">{modelShortName(agent.model)}</span>
                  </td>
                  <td class="col-context">
                    <div
                      class="ctx-cell"
                      title={agent.contextTokens
                        ? `${agent.contextTokens.toLocaleString()} / ${agent.contextLimit?.toLocaleString()} tokens (${agent.contextPercent.toFixed(1)}%)`
                        : `${agent.contextPercent.toFixed(1)}%`}
                    >
                      <div class="ctx-track">
                        <div
                          class="ctx-fill {getContextColorClass(agent.contextPercent)}"
                          style="width: {Math.min(100, Math.max(0, agent.contextPercent))}%"
                        ></div>
                      </div>
                      <span class="ctx-val {getContextColorClass(agent.contextPercent)}">
                        {agent.contextPercent.toFixed(0)}%
                      </span>
                    </div>
                  </td>
                  <td class="col-tokens text-right">
                    <div class="tok-wrapper">
                      <span class="tok-main">{formatTokens(agent.tokens)}</span>
                      {#if agent.cachedTokens > 0}
                        <span class="tok-sub">cache {formatTokens(agent.cachedTokens)}</span>
                      {/if}
                    </div>
                  </td>
                  <td class="col-cost text-right">
                    <span class="cost-text">{formatCost(agent.cost)}</span>
                  </td>
                  <td class="col-status text-center">
                    <span class="status-badge" style="--dot-c: {dotColor}">
                      <span class="status-dot" style="background: {dotColor}"></span>
                      <span class="status-text">{agent.status.toUpperCase()}</span>
                    </span>
                  </td>
                  <td class="col-action text-center">
                    <button class="jump-btn" title="Open Agent" aria-label="Open Agent">
                      <ExternalLink size={13} />
                    </button>
                  </td>
                </tr>
              {/each}
            {/if}
          </tbody>
        </table>
      </div>
    {:else if activeTab === 'projects'}
      <!-- Tab 2: Project Breakdown -->
      <div class="grid-breakdown">
        {#each $usageOverview?.projectSummaries ?? [] as p}
          <div class="summary-card">
            <div class="card-head">
              <span class="card-title">{p.projectName}</span>
              <span class="card-badge">{p.agentCount} agents</span>
            </div>
            <div class="card-metric">
              <span class="m-label">Tokens</span>
              <span class="m-val">{formatTokens(p.totalTokens)}</span>
            </div>
            <div class="card-metric">
              <span class="m-label">Estimated Cost</span>
              <span class="m-val">{formatCost(p.estimatedCostUsd)}</span>
            </div>
          </div>
        {/each}
      </div>
    {:else if activeTab === 'models'}
      <!-- Tab 3: Model Breakdown -->
      <div class="grid-breakdown">
        {#each $usageOverview?.modelSummaries ?? [] as m}
          <div class="summary-card">
            <div class="card-head">
              <span class="card-title">{modelShortName(m.model)}</span>
              <span class="card-badge">{m.sessionCount} sessions</span>
            </div>
            <div class="card-metric">
              <span class="m-label">Tokens</span>
              <span class="m-val">{formatTokens(m.totalTokens)}</span>
            </div>
            <div class="card-metric">
              <span class="m-label">Estimated Cost</span>
              <span class="m-val">{formatCost(m.estimatedCostUsd)}</span>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Footer Note -->
  <div class="control-footer">
    <div class="footer-note">
      <HelpCircle size={13} />
      <span
        >Cost estimation is based on public model API token pricing. Quotas reflect server
        subscription limits.</span
      >
    </div>
  </div>
</Modal>

<style>
  .control-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-bottom: 1px solid var(--bd, #27322b);
    background: var(--bg2, #161c18);
  }
  .header-left {
    display: flex;
    align-items: center;
    gap: var(--sp-4, 10px);
  }
  .header-icon {
    width: 34px;
    height: 34px;
    border-radius: 6px;
    background: rgba(0, 212, 126, 0.12);
    color: var(--ac, #00d47e);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .header-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .header-title {
    font-size: var(--md, 13px);
    font-weight: 600;
    color: var(--t0, #ffffff);
    letter-spacing: 0.08em;
  }
  .header-badge {
    font-size: 9px;
    background: rgba(0, 212, 126, 0.16);
    color: var(--ac, #00d47e);
    border: 1px solid rgba(0, 212, 126, 0.3);
    padding: 1px 5px;
    border-radius: 4px;
    font-family: var(--mono);
    font-weight: 600;
  }
  .header-sub {
    font-size: 11px;
    color: var(--t2, #8b9991);
    margin-top: 2px;
  }
  .header-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .action-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--bg3, #1d2520);
    border: 1px solid var(--bd, #27322b);
    color: var(--t1, #c5d1cb);
    padding: 6px 10px;
    border-radius: 4px;
    font-size: 11px;
    cursor: pointer;
    transition: all 0.15s;
  }
  .action-btn:hover {
    color: var(--t0, #ffffff);
    border-color: var(--ac, #00d47e);
  }
  .spinning :global(svg) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    100% {
      transform: rotate(360deg);
    }
  }
  .close-btn {
    background: none;
    border: none;
    color: var(--t2, #8b9991);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 4px;
  }
  .close-btn:hover {
    color: var(--t0, #ffffff);
    background: var(--bg3, #1d2520);
  }

  /* KPI Banner */
  .kpi-banner {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 14px;
    padding: 14px 24px;
    background: var(--bg1, #0f1412);
    border-bottom: 1px solid var(--bd, #27322b);
  }
  .kpi-card {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--bg2, #161c18);
    border: 1px solid var(--bd, #27322b);
    border-radius: 6px;
  }
  .kpi-icon {
    color: var(--ac, #00d47e);
    display: flex;
    align-items: center;
  }
  .kpi-label {
    font-size: 9.5px;
    color: var(--t3, #5a6660);
    letter-spacing: 0.08em;
    font-family: var(--mono);
  }
  .kpi-value {
    font-size: 17px;
    font-weight: 600;
    color: var(--t0, #ffffff);
    font-family: var(--mono);
    margin-top: 2px;
  }
  .warn-value {
    color: #e5a544;
  }

  /* Token history chart */
  .token-history-card {
    background: var(--bg2, #161c18);
    border: 1px solid var(--bd, #27322b);
    border-radius: 6px;
    padding: 16px 20px 14px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .token-history-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }
  .token-history-title-block {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .token-history-subtitle {
    color: var(--t3, #5a6660);
    font-size: 11px;
  }
  .token-history-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .time-range-group {
    display: inline-flex;
    align-items: center;
    background: var(--bg1, #0f1412);
    border: 1px solid var(--bd, #27322b);
    border-radius: 6px;
    padding: 2px;
    gap: 2px;
  }
  .time-range-btn {
    background: transparent;
    border: none;
    color: var(--t2, #8b9991);
    font-family: var(--mono);
    font-size: 10px;
    font-weight: 500;
    padding: 3px 8px;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .time-range-btn:hover {
    color: var(--t0, #ffffff);
    background: var(--bg3, #1d2520);
  }
  .time-range-btn.active {
    color: #0f1412;
    background: var(--ac, #00d47e);
    font-weight: 600;
  }
  .token-stat-chips {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .token-stat-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    background: var(--bg1, #0f1412);
    border: 1px solid var(--bd, #27322b);
    border-radius: 4px;
    font-size: 11px;
    font-family: var(--mono);
  }
  .token-stat-chip .chip-label {
    color: var(--t3, #5a6660);
    font-size: 10px;
    text-transform: uppercase;
  }
  .token-stat-chip.total .chip-val {
    color: var(--ac, #00d47e);
    font-weight: 600;
  }
  .token-stat-chip.peak .chip-val {
    color: var(--t1, #c5d1cb);
    font-weight: 500;
  }
  .token-chart-wrap {
    width: 100%;
    position: relative;
    cursor: crosshair;
  }
  .token-chart {
    display: block;
    width: 100%;
    height: 220px;
    overflow: visible;
  }
  .token-chart-grid {
    stroke: var(--bd, #27322b);
    stroke-width: 1;
    stroke-dasharray: 4 4;
  }
  .token-chart-grid.mid {
    stroke: rgba(39, 50, 43, 0.4);
  }
  .token-chart-y-label {
    fill: var(--t3, #5a6660);
    font-family: var(--mono);
    font-size: 10px;
  }
  .token-chart-smooth-area {
    fill: url(#tokenSmoothGradient);
    transition: d 0.25s ease;
  }
  .token-chart-smooth-line {
    fill: none;
    stroke: var(--ac, #00d47e);
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 2.5;
    filter: url(#neonCurveGlow);
    transition: d 0.25s ease;
  }
  .token-chart-point-subtle {
    fill: var(--bg1, #0f1412);
    stroke: var(--ac, #00d47e);
    stroke-width: 1.5;
    opacity: 0.4;
    transition: all 0.2s ease;
  }
  .token-chart-point-subtle.has-tokens {
    opacity: 1;
    fill: #00ff94;
    stroke: #00ff94;
  }
  .hover-crosshair {
    stroke: rgba(0, 212, 126, 0.4);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }
  .hover-halo {
    fill: rgba(0, 212, 126, 0.25);
    stroke: rgba(0, 212, 126, 0.6);
    stroke-width: 1;
  }
  .hover-point-active {
    fill: #ffffff;
    stroke: var(--ac, #00d47e);
    stroke-width: 2;
  }
  .chart-tooltip-bg {
    fill: rgba(15, 20, 18, 0.95);
    stroke: var(--bd, #27322b);
    stroke-width: 1;
    filter: drop-shadow(0 4px 12px rgba(0, 0, 0, 0.6));
  }
  .chart-tooltip-time {
    fill: var(--t2, #8b9991);
    font-family: var(--sans);
    font-size: 9.5px;
  }
  .chart-tooltip-tokens {
    fill: var(--ac, #00d47e);
    font-family: var(--mono);
    font-size: 11px;
    font-weight: 600;
  }
  .token-chart-axis {
    display: flex;
    justify-content: space-between;
    padding: 4px 28px 0 52px;
    color: var(--t3, #5a6660);
    font-family: var(--mono);
    font-size: 10px;
  }
  .token-chart-empty {
    display: grid;
    min-height: 140px;
    place-items: center;
    color: var(--t3, #5a6660);
    font-size: 11.5px;
    background: var(--bg1, #0f1412);
    border: 1px dashed var(--bd, #27322b);
    border-radius: 6px;
  }

  /* Scroll Area */
  .content-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  /* Section Header */
  .section-container {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .section-header-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    background: var(--bg2, #161c18);
    border: 1px solid var(--bd, #27322b);
    border-radius: 6px;
    padding: 10px 14px;
    cursor: pointer;
    text-align: left;
    transition: all 0.15s ease;
    user-select: none;
  }
  .section-header-btn:hover {
    background: var(--bg3, #1d2520);
    border-color: rgba(0, 212, 126, 0.35);
  }
  .section-title {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 11px;
    font-weight: 600;
    color: var(--t1, #c5d1cb);
    letter-spacing: 0.06em;
  }
  .quota-count-badge {
    font-size: 10px;
    font-family: var(--mono);
    background: var(--bg3, #1d2520);
    border: 1px solid var(--bd, #27322b);
    color: var(--ac, #00d47e);
    padding: 1px 6px;
    border-radius: 999px;
    font-weight: 600;
  }
  .section-header-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .section-hint {
    font-size: 10px;
    color: var(--t3, #5a6660);
  }
  .collapse-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--t2, #8b9991);
    transition: color 0.15s ease;
  }
  .section-header-btn:hover .collapse-icon {
    color: var(--t0, #ffffff);
  }

  /* Quota Cards */
  .quota-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .quota-card {
    background: var(--bg2, #161c18);
    border: 1px solid var(--bd, #27322b);
    border-radius: 6px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .quota-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .quota-card-actions {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .auto-handoff-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--t3, #5a6660);
    font-size: 9px;
    text-transform: uppercase;
  }
  .auto-handoff-toggle input {
    accent-color: #00d47e;
    margin: 0;
  }
  .provider-badge {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
  }
  .provider-badge.codex {
    color: #00d47e;
  }
  .provider-badge.claude {
    color: #d97757;
  }
  .dot {
    font-size: 8px;
  }
  .source-tag {
    font-size: 9px;
    padding: 2px 6px;
    background: var(--bg3, #1d2520);
    border: 1px solid var(--bd, #27322b);
    border-radius: 4px;
    color: var(--t3, #5a6660);
    font-family: var(--mono);
    text-transform: uppercase;
  }
  .quota-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .quota-meta {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
  }
  .quota-label {
    color: var(--t2, #8b9991);
  }
  .quota-pct {
    font-family: var(--mono);
    font-weight: 600;
  }
  .progress-track {
    height: 6px;
    background: var(--bg3, #1d2520);
    border-radius: 999px;
    overflow: hidden;
  }
  .progress-fill {
    height: 100%;
    border-radius: 999px;
    transition: width 0.3s ease;
  }
  .reset-meta {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    color: var(--t3, #5a6660);
    font-family: var(--mono);
  }

  /* Status Colors */
  .quota-normal,
  .ctx-normal {
    color: #00d47e;
  }
  .quota-warning,
  .ctx-warning {
    color: #e5a544;
  }
  .quota-high,
  .ctx-high {
    color: #f07b3f;
  }
  .quota-critical,
  .ctx-critical {
    color: #f04848;
  }
  .progress-fill.quota-normal,
  .ctx-fill.ctx-normal {
    background-color: #00d47e;
  }
  .progress-fill.quota-warning,
  .ctx-fill.ctx-warning {
    background-color: #e5a544;
  }
  .progress-fill.quota-high,
  .ctx-fill.ctx-high {
    background-color: #f07b3f;
  }
  .progress-fill.quota-critical,
  .ctx-fill.ctx-critical {
    background-color: #f04848;
  }

  /* Navigation Tabs */
  .tabs-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--bd, #27322b);
    padding-bottom: 8px;
    margin-top: 4px;
  }
  .tabs-group {
    display: flex;
    gap: 8px;
  }
  .tab-btn {
    background: none;
    border: none;
    font-size: 12px;
    font-weight: 500;
    color: var(--t2, #8b9991);
    padding: 6px 12px;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s;
  }
  .tab-btn:hover {
    color: var(--t0, #ffffff);
    background: var(--bg2, #161c18);
  }
  .tab-btn.active {
    color: var(--ac, #00d47e);
    background: rgba(0, 212, 126, 0.1);
  }
  .search-box {
    display: flex;
    align-items: center;
    gap: 7px;
    background: var(--bg2, #161c18);
    border: 1px solid var(--bd, #27322b);
    border-radius: 5px;
    padding: 6px 10px;
    width: 280px;
    color: var(--t2, #8b9991);
  }
  .search-box input {
    background: transparent;
    border: none;
    outline: none;
    color: var(--t0, #ffffff);
    font-size: 11.5px;
    width: 100%;
  }
  .clear-search {
    background: none;
    border: none;
    color: var(--t3, #5a6660);
    cursor: pointer;
    font-size: 10px;
    padding: 0;
  }

  /* Table */
  .table-container {
    background: var(--bg2, #161c18);
    border: 1px solid var(--bd, #27322b);
    border-radius: 6px;
    overflow-x: auto;
  }
  .usage-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
    table-layout: fixed;
  }
  .usage-table th {
    background: var(--bg3, #1d2520);
    color: var(--t2, #8b9991);
    font-weight: 500;
    text-align: left;
    padding: 12px 14px;
    border-bottom: 1px solid var(--bd, #27322b);
    user-select: none;
    vertical-align: middle;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11.5px;
  }
  .usage-table th.sortable {
    cursor: pointer;
  }
  .usage-table th.sortable:hover {
    color: var(--t0, #ffffff);
  }
  .th-content {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    vertical-align: middle;
  }
  .th-content.right {
    justify-content: flex-end;
    width: 100%;
  }
  .usage-table td {
    padding: 14px 14px;
    border-bottom: 1px solid rgba(39, 50, 43, 0.4);
    color: var(--t1, #c5d1cb);
    vertical-align: middle;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
  }
  .col-name {
    width: auto;
    min-width: 240px;
  }
  .col-provider {
    width: 90px;
  }
  .col-account {
    width: 110px;
  }
  .col-model {
    width: 125px;
  }
  .col-context {
    width: 140px;
  }
  .col-tokens {
    width: 95px;
  }
  .col-cost {
    width: 90px;
  }
  .col-status {
    width: 105px;
  }
  .col-action {
    width: 40px;
    text-align: center;
  }
  .agent-row {
    cursor: pointer;
    transition: background 0.12s;
  }
  .agent-row:hover {
    background: rgba(0, 212, 126, 0.04);
  }
  .project-agent {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    vertical-align: middle;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .p-name {
    color: var(--t2, #8b9991);
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sep {
    color: var(--t3, #5a6660);
    flex-shrink: 0;
  }
  .a-name {
    color: var(--t0, #ffffff);
    font-weight: 500;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .account-text {
    color: var(--t2, #8b9991);
    font-size: 11.5px;
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .provider-pill {
    display: inline-flex;
    align-items: center;
    font-size: 9.5px;
    padding: 2px 7px;
    border-radius: 4px;
    font-family: var(--mono);
    text-transform: uppercase;
    background: var(--bg3, #1d2520);
    border: 1px solid var(--bd, #27322b);
    line-height: 1.3;
    vertical-align: middle;
  }
  .provider-pill.codex {
    border-color: rgba(0, 212, 126, 0.3);
    color: #00d47e;
  }
  .provider-pill.claude-code,
  .provider-pill.claude {
    border-color: rgba(217, 119, 87, 0.3);
    color: #d97757;
  }
  .model-text {
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--t2, #8b9991);
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ctx-cell {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    vertical-align: middle;
  }
  .ctx-track {
    width: 64px;
    height: 5px;
    background: var(--bg3, #1d2520);
    border-radius: 999px;
    overflow: hidden;
    flex-shrink: 0;
  }
  .ctx-fill {
    height: 100%;
    border-radius: 999px;
  }
  .ctx-val {
    font-family: var(--mono);
    font-size: 10.5px;
    font-weight: 500;
    min-width: 24px;
  }
  .tok-wrapper {
    display: inline-flex;
    flex-direction: column;
    align-items: flex-end;
    justify-content: center;
    vertical-align: middle;
    line-height: 1.25;
  }
  .tok-main {
    font-family: var(--mono);
    font-weight: 500;
    color: var(--t0, #ffffff);
    font-size: 12px;
  }
  .tok-sub {
    font-size: 9.5px;
    color: var(--t3, #5a6660);
    font-family: var(--mono);
  }
  .cost-text {
    font-family: var(--mono);
    color: var(--t1, #c5d1cb);
    vertical-align: middle;
    font-size: 12px;
  }
  .status-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    font-size: 10px;
    font-family: var(--mono);
    font-weight: 600;
    color: var(--dot-c);
    vertical-align: middle;
  }
  .status-dot {
    width: 6.5px;
    height: 6.5px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .jump-btn {
    background: none;
    border: none;
    color: var(--t3, #5a6660);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    vertical-align: middle;
  }
  .jump-btn:hover {
    color: var(--ac, #00d47e);
    background: var(--bg3, #1d2520);
  }
  .text-right {
    text-align: right;
  }
  .text-center {
    text-align: center;
  }
  .empty-state {
    text-align: center;
    padding: 36px;
    color: var(--t3, #5a6660);
  }

  /* Grid Breakdown */
  .grid-breakdown {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
  }
  .summary-card {
    background: var(--bg2, #161c18);
    border: 1px solid var(--bd, #27322b);
    border-radius: 6px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--bd, #27322b);
    padding-bottom: 6px;
  }
  .card-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--t0, #ffffff);
  }
  .card-badge {
    font-size: 10px;
    color: var(--t2, #8b9991);
    font-family: var(--mono);
  }
  .card-metric {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11px;
  }
  .m-label {
    color: var(--t2, #8b9991);
  }
  .m-val {
    font-family: var(--mono);
    font-weight: 600;
    color: var(--t0, #ffffff);
  }

  /* Footer */
  .control-footer {
    padding: 10px 18px;
    background: var(--bg2, #161c18);
    border-top: 1px solid var(--bd, #27322b);
  }
  .footer-note {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    color: var(--t3, #5a6660);
  }

  .text-right {
    text-align: right;
  }
  .text-center {
    text-align: center;
  }
</style>
