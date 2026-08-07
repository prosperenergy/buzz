# Prosper fleet integration for Buzz

This directory is the durable, secret-free source of truth for Prosper's Buzz setup.
It provisions six real Buzz managed-agent identities (Hanna, Sam, Burtha, Kayla,
Maddi, and Oma) against `wss://prosper.communities.buzz.xyz`.

The integration deliberately separates four layers:

1. `fleet.json` and `personas/` define the fleet.
2. `bin/` contains guarded ACP launchers that reuse Craig's existing first-party
   subscription logins without copying credentials into git.
3. `tools/fleet-provisioner` performs an atomic, keychain-backed install and validates
   NIP-OA owner attestations without printing private keys.
4. `scripts/` installs, provisions, tests, diagnoses, and keeps the local clone current.

Run the complete local install with:

```bash
prosper/scripts/provision-fleet.sh
prosper/scripts/test-harnesses.sh
BUZZ_OWNER_NSEC_STDIN=1 prosper/scripts/test-fleet.sh  # then supply the verified owner nsec on stdin
```

`provision-fleet.sh` gracefully closes and reopens Buzz because Buzz reconciles disk
definitions to relay events at boot. It always takes timestamped file backups before a
change. Existing agent keys are reused; missing fleet identities receive new keys in the
signed app's owner-mode store and Buzz immediately migrates them into macOS Keychain,
stripping the transient inline fields before the provisioner returns. Secrets are never
written to this repository.

Automation has two complementary paths:

- `.github/workflows/prosper-upstream-sync.yml` merges Block's upstream `main` into the
  Prosper fork every day and pushes the merge back to `prosperenergy/buzz`.
- `energy.prosper.buzz-fleet-sync.plist` fast-forwards this clean local clone every 30
  minutes. When the Prosper manifest or runtime changes, it safely reprovisions Buzz.

The signed Buzz application retains its own Tauri release updater. This repository sync
keeps the fork/source and Prosper integration current; the app updater keeps the installed
desktop binary current.

Safety boundaries:

- Every agent is `owner-only` and has parallelism `1`.
- The test harness creates a private, dedicated channel and requires an exact reply from
  each agent identity.
- Active Gemini, Codex/Goose, and Grok harnesses run with ambient API keys unset. Their
  existing first-party subscription logins remain in protected credential stores.
- Claude and Copilot launchers remain available as recovery rails, but are not assigned:
  live testing on 2026-08-07 found Claude org policy blocking subscription use and the
  Copilot monthly quota exhausted. Hanna and Burtha prompts disclose their tested fallbacks.
- No workflow force-pushes, rotates credentials, or deletes Craig's data.
