import { writable } from 'svelte/store';
import type { ProviderAccount } from '../types';
import { getProviderAccounts } from '../tauri/accounts';

export const providerAccounts = writable<Record<string, ProviderAccount>>({});

/** Refresh the shared account map after account actions or app startup.
 * @return Configured accounts keyed by stable ID.
 * @throws When the backend cannot list accounts.
 * @author ductv <ductv@getflycrm.com>
 * @since 2026-09-25
 */
export async function refreshProviderAccounts(): Promise<Record<string, ProviderAccount>> {
  const accounts = await getProviderAccounts();
  const byId = Object.fromEntries(accounts.map((account) => [account.id, account]));
  providerAccounts.set(byId);
  return byId;
}
