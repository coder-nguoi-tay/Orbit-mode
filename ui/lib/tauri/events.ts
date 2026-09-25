import type { Session, TokenUsage, MiniLogEntry } from '../stores/sessions';
import type {
  JournalEntry,
  SubagentInfo,
  ProviderQuota,
  SessionUsageSnapshot,
  RateLimitInfo,
} from '../types';
import { listen } from './invoke';

export interface SessionOutputPayload {
  sessionId: number;
  entry: JournalEntry;
}

export interface SessionStatePayload {
  sessionId: number;
  status: string;
  tokens: TokenUsage;
  contextPercent: number;
  pendingApproval: string | null;
  miniLog: MiniLogEntry[];
  gitBranch: string | null;
  subagents: SubagentInfo[];
  model: string | null;
  contextWindow: number | null;
  attention: { requiresAttention: boolean; reason: string | null; since: string | null } | null;
  rateLimit: RateLimitInfo[];
  costUsd: number | null;
}

export function onSessionCreated(cb: (session: Session) => void) {
  return listen<Session>('session:created', (e) => cb(e.payload));
}

export function onSessionOutput(cb: (payload: SessionOutputPayload) => void) {
  return listen<SessionOutputPayload>('session:output', (e) => cb(e.payload));
}

export function onSessionState(cb: (payload: SessionStatePayload) => void) {
  return listen<SessionStatePayload>('session:state', (e) => cb(e.payload));
}

export function onSessionStopped(cb: (sessionId: number) => void) {
  return listen<{ sessionId: number }>('session:stopped', (e) => cb(e.payload.sessionId));
}

export function onSessionRunning(cb: (sessionId: number, pid: number) => void) {
  return listen<{ sessionId: number; pid: number }>('session:running', (e) =>
    cb(e.payload.sessionId, e.payload.pid)
  );
}

export function onSessionError(cb: (sessionId: number, error: string) => void) {
  return listen<{ sessionId: number; error: string }>('session:error', (e) =>
    cb(e.payload.sessionId, e.payload.error)
  );
}

export function onSessionRateLimit(cb: (sessionId: number) => void) {
  return listen<{ sessionId: number }>('session:rate-limit', (e) => cb(e.payload.sessionId));
}

export function onSessionTaskUpdate(cb: (sessionId: number) => void) {
  return listen<{ sessionId: number }>('session:task-update', (e) => cb(e.payload.sessionId));
}

export function onSessionReset(cb: () => void) {
  return listen<Record<string, never>>('session:reset', () => cb());
}

export function onSessionDeleted(cb: (sessionId: number) => void) {
  return listen<{ sessionId: number }>('session:deleted', (e) => cb(e.payload.sessionId));
}

export type RawOutputPayload = { sessionId: number; line: string };
export function onSessionRawOutput(cb: (payload: RawOutputPayload) => void) {
  return listen<RawOutputPayload>('session:raw-output', (e) => cb(e.payload));
}

export interface StderrPayload {
  sessionId: number;
  line: string;
}
export function onSessionStderr(cb: (payload: StderrPayload) => void) {
  return listen<StderrPayload>('session:stderr', (e) => cb(e.payload));
}

export interface GitUpdatePayload {
  sessionId: number;
  snapshot: import('../types').GitSnapshot;
}
export function onSessionGitUpdate(cb: (payload: GitUpdatePayload) => void) {
  return listen<GitUpdatePayload>('session:git-update', (e) => cb(e.payload));
}

export interface SubagentCreatedPayload {
  parentSessionId: number;
  description: string;
  tool: string;
}

export function onSessionSubagentCreated(cb: (payload: SubagentCreatedPayload) => void) {
  return listen<SubagentCreatedPayload>('session:subagent-created', (e) => cb(e.payload));
}

export function onSessionUsageUpdated(cb: (payload: SessionUsageSnapshot) => void) {
  return listen<SessionUsageSnapshot>('session:usage-updated', (e) => cb(e.payload));
}

export function onProviderQuotaUpdated(cb: (payload: ProviderQuota) => void) {
  return listen<ProviderQuota>('provider:quota-updated', (e) => cb(e.payload));
}

/** Observe sessions paused after an authoritative account quota event.
 * @param callback Receives the affected Orbit session ID.
 * @return Event listener cleanup handle.
 * @throws When the event listener cannot be installed.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function onSessionAccountActionRequired(
  callback: (sessionId: number) => void
): Promise<() => void> {
  return listen<{ sessionId: number }>('session:account-action-required', (event) =>
    callback(event.payload.sessionId)
  );
}

/** Observe a committed account transition after a new process starts.
 * @param callback Receives the session and new account IDs.
 * @return Event listener cleanup handle.
 * @throws When the event listener cannot be installed.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function onSessionAccountChanged(
  callback: (payload: { sessionId: number; providerAccountId: string }) => void
): Promise<() => void> {
  return listen<{ sessionId: number; providerAccountId: string }>(
    'session:account-changed',
    (event) => callback(event.payload)
  );
}

/** Observe a failed handoff while the original account binding stays intact.
 * @param callback Receives the failed session and error.
 * @return Event listener cleanup handle.
 * @throws When the event listener cannot be installed.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export function onSessionHandoffFailed(
  callback: (payload: { sessionId: number; error: string }) => void
): Promise<() => void> {
  return listen<{ sessionId: number; error: string }>('session:handoff-failed', (event) =>
    callback(event.payload)
  );
}
