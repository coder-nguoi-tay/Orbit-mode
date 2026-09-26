import type { Session } from '../stores/sessions';
import type { JournalEntry } from '../types';
import { invoke } from './invoke';

export interface CreateSessionOptions {
  projectPath: string;
  prompt: string;
  model?: string;
  permissionMode?: 'ignore' | 'approve';
  sessionName?: string;
  useWorktree?: boolean;
  provider?: string;
  providerAccountId?: string;
  /** Provider API key. Set before spawn to avoid race condition. */
  apiKey?: string;
  sshHost?: string;
  sshUser?: string;
  /** SSH private key file path. */
  sshKeyPath?: string;
}

/** Create a provider session with an explicit account binding when selected.
 * @param opts Project, provider, account and initial prompt settings.
 * @return Newly persisted session before its provider process starts.
 * @throws When the backend rejects the account or session settings.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export async function createSession(opts: CreateSessionOptions): Promise<Session> {
  return await invoke('create_session', {
    projectPath: opts.projectPath,
    prompt: opts.prompt,
    model: opts.model ?? null,
    permissionMode: opts.permissionMode ?? 'ignore',
    sessionName: opts.sessionName ?? null,
    useWorktree: opts.useWorktree ?? false,
    provider: opts.provider ?? 'claude-code',
    providerAccountId: opts.providerAccountId ?? null,
    apiKey: opts.apiKey ?? null,
    sshHost: opts.sshHost ?? null,
    sshUser: opts.sshUser ?? null,
    sshKeyPath: opts.sshKeyPath ?? null,
  });
}

export async function listSessions(): Promise<Session[]> {
  return await invoke('list_sessions');
}

export async function stopSession(sessionId: number): Promise<void> {
  await invoke('stop_session', { sessionId });
}

export async function sendSessionMessage(sessionId: number, message: string): Promise<void> {
  await invoke('send_session_message', { sessionId, message });
}

export async function updateSessionModel(sessionId: number, model: string): Promise<void> {
  await invoke('update_session_model', { sessionId, model });
}

export async function updateSessionEffort(sessionId: number, effort: string): Promise<void> {
  await invoke('update_session_effort', { sessionId, effort });
}

export async function setSessionApiKey(sessionId: number, apiKey: string): Promise<void> {
  await invoke('set_session_api_key', { sessionId, apiKey });
}

export async function renameSession(sessionId: number, name: string): Promise<void> {
  await invoke('rename_session', { sessionId, name });
}

export async function deleteSession(sessionId: number): Promise<void> {
  await invoke('delete_session', { sessionId });
}

export async function getSessionJournal(sessionId: number): Promise<JournalEntry[]> {
  return await invoke('get_session_journal', { sessionId });
}

/** Load a bounded conversation page so large sessions do not block project switching.
 * @param sessionId Session whose conversation history is requested.
 * @param cursor Optional sequence boundary for the requested page.
 * @param limit Maximum number of entries returned.
 * @param direction Whether entries before or after the cursor are requested.
 * @return The latest page without a cursor, otherwise the adjacent history page.
 * @throws When the desktop or web backend cannot read the journal.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-27
 */
export async function getSessionJournalPage(
  sessionId: number,
  cursor: number | null = null,
  limit = 100,
  direction: 'forward' | 'backward' = 'backward'
): Promise<JournalEntry[]> {
  return await invoke('get_session_journal_page', { sessionId, cursor, limit, direction });
}

export async function resetSessions(): Promise<void> {
  await invoke('reset_sessions');
}

export async function getSessionRawOutputs(sessionId: number): Promise<string[]> {
  return await invoke('get_session_raw_outputs', { sessionId });
}
