# Codex account profiles

Orbit can keep multiple local Codex profiles in separate credential homes. Open **Settings → Accounts**, add a profile, choose **Codex / ChatGPT** or **Codex / API key**, then authenticate it through the installed Codex CLI. Give each profile a label such as Personal or Work. Orbit stores profile metadata in SQLite; Codex owns the login credentials in each profile home. Orbit does not copy the current `~/.codex/auth.json` or store an API key in its account table.

**System Default** points to the existing Codex login. New profiles are created under Orbit's application-data directory with restricted directory permissions. Each Codex child process receives its selected `CODEX_HOME`; Orbit does not change the shell's global environment. The installed Codex CLI is shared across profiles.

When creating a local Codex agent, select the account in the new-session dialog. Orbit stores the account ID on the session and attributes subsequent quota samples and token usage to that profile. Usage Center and agent cards show the bound account. The selected account stays bound across Orbit restarts. A missing or unavailable bound account requires user action; Orbit does not silently choose another profile.

Codex ChatGPT allowance is shown per profile for the five-hour and seven-day windows when the CLI emits usage data. API profiles use API billing and rate limits, which are separate from ChatGPT allowance. Orbit never adds usage allowances from different profiles together.

When an authoritative Codex plan quota event reports exhaustion, Orbit stops only that provider process and preserves its session and worktree. In **Settings → Accounts**, enable **Automatic quota handoff** on the profiles that may be used as fallbacks. Orbit starts a new Codex conversation on the next enabled, authenticated profile without another prompt, using the same worktree and branch. Each enabled profile is used at most once per session; if no enabled profile is available, the session enters the normal handoff dialog. The handoff packet contains the original task, stored messages, Git state and recent summary; a remote conversation ID from the old account is not reused. Account transitions are recorded without credentials. A temporary rate-limit event does not trigger profile switching.

Removing a profile keeps projects, worktrees, sessions and usage history. Orbit asks separately whether to delete the profile's CLI-managed credential directory. An active session is never terminated by profile removal. Renaming changes only the display label.

Automatic handoff is an explicit user setting. Orbit does not combine quota percentages or rewrite historical usage, and it never buys usage extensions. Other providers and SSH-hosted credentials require their own provider-specific account integration.

## Current limits

- API key profiles use isolated Codex login, but the account dashboard does not yet fetch API RPM, TPM or spend from an API project.
- A quota event stops the process that reported it. Other already-running processes on the same profile are shown as affected, but each stops or hands off when its own provider process reports the limit; Orbit does not terminate sibling processes without their own event.
- A paused profile becomes ready only when a new authoritative quota event reports availability. Orbit does not poll a provider usage endpoint while every process on that profile is stopped.
- Model discovery uses the Codex CLI catalog and the new process must still start a model turn. A later provider rejection may still require another user action.
- Real two-profile authentication and handoff acceptance testing requires the user's interactive Codex login; the automated tests cover isolation settings, persistence, quota separation and startup validation.
