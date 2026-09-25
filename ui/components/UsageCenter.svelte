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
  $: {
    const quotasByAccount = new Map<string, ProviderQuota>();
    for (const quota of $providerQuotas) {
      quotasByAccount.set(
        `${quota.provider}:${quota.providerAccountId ?? quota.accountKey}`,
        quota
      );
    }
    for (const session of $sessions) {
      if (session.provider !== 'codex' || !session.rateLimit?.length) continue;
      const accountKey = session.providerAccountId ?? 'default';
      const fiveHourLimit = session.rateLimit.find((limit) => limit.rateLimitType === 'five_hour');
      const sevenDayLimit = session.rateLimit.find((limit) => limit.rateLimitType === 'seven_day');
      const liveQuota: ProviderQuota = {
        provider: 'codex',
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
      const quotaKey = `codex:${accountKey}`;
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
  $: claudeQuota = $providerQuotas.find(
    (q: ProviderQuota) => q.provider === 'claude-code' || q.provider === 'claude'
  );

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

  function openAgentSession(sessionId: number) {
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
                        disabled={!['available', 'busy', 'near_limit'].includes(codexAccount.status)}
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
