#![recursion_limit = "256"]

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("prosper-buzz-fleet-provisioner currently supports macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(target_os = "macos")]
mod macos {
    use anyhow::{bail, Context, Result};
    use chrono::{SecondsFormat, Utc};
    use nostr::{FromBech32, Keys, PublicKey, ToBech32};
    use serde::Deserialize;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, HashSet};
    use std::fs::{self, OpenOptions};
    use std::io::{Read, Write};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FleetManifest {
        version: u32,
        relay_url: String,
        expected_owner_npub: String,
        team: TeamManifest,
        agents: Vec<AgentManifest>,
    }

    #[derive(Debug, Deserialize)]
    struct TeamManifest {
        id: String,
        name: String,
        description: String,
        instructions: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AgentManifest {
        slug: String,
        name: String,
        runtime: String,
        command: String,
        prompt_file: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        provider: Option<String>,
    }

    struct LoadedAgent<'a> {
        manifest: &'a AgentManifest,
        prompt: String,
        command: PathBuf,
        source_version: String,
    }

    #[derive(serde::Serialize)]
    struct PersonaHashContent<'a> {
        display_name: &'a str,
        system_prompt: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        avatar_url: Option<&'a str>,
        runtime: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider: Option<&'a str>,
        name_pool: Vec<&'a str>,
        respond_to: Option<&'a str>,
        respond_to_allowlist: Vec<&'a str>,
        parallelism: Option<u32>,
    }

    pub fn run() -> Result<()> {
        let mode = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "doctor".to_string());
        let root = repo_root()?;
        let manifest = load_manifest(&root)?;
        validate_manifest(&manifest)?;
        let loaded = load_agents(&root, &manifest)?;

        match mode.as_str() {
            "plan" => plan(&root, &manifest, &loaded),
            "apply" => apply(&root, &manifest, &loaded),
            "doctor" => doctor(&root, &manifest, &loaded),
            other => bail!("unknown mode {other:?}; use plan, apply, or doctor"),
        }
    }

    fn repo_root() -> Result<PathBuf> {
        if let Some(root) = std::env::var_os("BUZZ_PROSPER_ROOT") {
            return fs::canonicalize(root).context("canonicalize BUZZ_PROSPER_ROOT");
        }
        let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for _ in 0..3 {
            root = root
                .parent()
                .context("resolve repository root")?
                .to_path_buf();
        }
        fs::canonicalize(root).context("canonicalize repository root")
    }

    fn load_manifest(root: &Path) -> Result<FleetManifest> {
        let path = root.join("prosper/fleet.json");
        let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
    }

    fn validate_manifest(manifest: &FleetManifest) -> Result<()> {
        if manifest.version != 1 {
            bail!("unsupported fleet manifest version {}", manifest.version);
        }
        if manifest.relay_url != "wss://prosper.communities.buzz.xyz" {
            bail!("refusing unexpected relay {}", manifest.relay_url);
        }
        if manifest.agents.len() != 6 {
            bail!("Prosper fleet must contain exactly six agents");
        }
        let mut slugs = HashSet::new();
        let mut names = HashSet::new();
        for agent in &manifest.agents {
            if !agent.slug.starts_with("prosper:") || !slugs.insert(agent.slug.clone()) {
                bail!("invalid or duplicate agent slug {}", agent.slug);
            }
            if !names.insert(agent.name.to_ascii_lowercase()) {
                bail!("duplicate agent name {}", agent.name);
            }
        }
        Ok(())
    }

    fn load_agents<'a>(root: &Path, manifest: &'a FleetManifest) -> Result<Vec<LoadedAgent<'a>>> {
        manifest
            .agents
            .iter()
            .map(|agent| {
                let prompt_path = root.join(&agent.prompt_file);
                let command = root.join(&agent.command);
                let prompt = fs::read_to_string(&prompt_path)
                    .with_context(|| format!("read persona {}", prompt_path.display()))?;
                if prompt.trim().is_empty() {
                    bail!("persona {} is empty", agent.name);
                }
                if !command.is_file() {
                    bail!("agent command missing: {}", command.display());
                }
                let mode = fs::metadata(&command)?.permissions().mode();
                if mode & 0o111 == 0 {
                    bail!("agent command is not executable: {}", command.display());
                }
                let content = PersonaHashContent {
                    display_name: &agent.name,
                    system_prompt: Some(prompt.trim()),
                    avatar_url: None,
                    runtime: Some(&agent.runtime),
                    model: agent.model.as_deref(),
                    provider: agent.provider.as_deref(),
                    name_pool: vec![&agent.name],
                    respond_to: Some("owner-only"),
                    respond_to_allowlist: vec![],
                    parallelism: Some(1),
                };
                let digest = Sha256::digest(serde_json::to_vec(&content)?);
                Ok(LoadedAgent {
                    manifest: agent,
                    prompt: prompt.trim().to_string(),
                    command,
                    source_version: hex::encode(digest),
                })
            })
            .collect()
    }

    fn app_data_dir() -> Result<PathBuf> {
        let home = std::env::var_os("HOME").context("HOME is unset")?;
        Ok(PathBuf::from(home).join("Library/Application Support/xyz.block.buzz.app"))
    }

    fn store_paths() -> Result<(PathBuf, PathBuf)> {
        let agents = app_data_dir()?.join("agents");
        Ok((
            agents.join("managed-agents.json"),
            agents.join("teams.json"),
        ))
    }

    fn read_values(path: &Path) -> Result<Vec<Value>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
    }

    fn expected_owner_public(expected_npub: &str) -> Result<PublicKey> {
        PublicKey::from_bech32(expected_npub).context("parse expected Buzz owner npub")
    }

    /// Read an explicitly supplied one-time owner recovery key from stdin.
    ///
    /// This is intentionally opt-in and stdin-only: the nsec never appears in
    /// argv, the environment, a repository file, or command output. Routine
    /// syncs preserve existing public agent identities and do not need it.
    fn owner_keys_from_stdin(expected_npub: &str) -> Result<Keys> {
        if std::env::var("BUZZ_OWNER_NSEC_STDIN").as_deref() != Ok("1") {
            bail!(
                "one or more agent identities must be created; pipe the verified owner nsec to stdin with BUZZ_OWNER_NSEC_STDIN=1"
            );
        }
        let mut identity = String::new();
        std::io::stdin()
            .take(256)
            .read_to_string(&mut identity)
            .context("read owner recovery nsec from stdin")?;
        let keys = Keys::parse(identity.trim()).context("parse owner recovery nsec")?;
        let npub = keys.public_key().to_bech32().context("encode owner npub")?;
        if npub != expected_npub {
            bail!("Buzz owner mismatch: expected {expected_npub}, found {npub}");
        }
        Ok(keys)
    }

    fn is_prosper_record(value: &Value) -> bool {
        value
            .get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| s.starts_with("prosper:"))
            || value
                .get("persona_id")
                .and_then(Value::as_str)
                .is_some_and(|s| s.starts_with("prosper:"))
    }

    fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
        value.get(key).and_then(Value::as_str)
    }

    fn existing_by_slug<'a>(
        records: &'a [Value],
        slug: &str,
        definition: bool,
    ) -> Option<&'a Value> {
        records.iter().find(|record| {
            let key = if definition { "slug" } else { "persona_id" };
            string_field(record, key) == Some(slug)
                && string_field(record, "pubkey")
                    .unwrap_or_default()
                    .is_empty()
                    == definition
        })
    }

    fn build_definition(agent: &LoadedAgent<'_>, now: &str, existing: Option<&Value>) -> Value {
        let created_at = existing
            .and_then(|v| string_field(v, "created_at"))
            .unwrap_or(now);
        let prior_updated = existing
            .and_then(|v| string_field(v, "updated_at"))
            .unwrap_or(now);
        let mut value = json!({
            "pubkey": "",
            "name": agent.manifest.name,
            "persona_id": null,
            "auth_tag": null,
            "relay_url": "",
            "avatar_url": null,
            "acp_command": "buzz-acp",
            "agent_command": "",
            "agent_command_override": null,
            "agent_args": [],
            "mcp_command": "",
            "turn_timeout_seconds": 320,
            "idle_timeout_seconds": null,
            "max_turn_duration_seconds": null,
            "parallelism": 1,
            "system_prompt": agent.prompt,
            "model": agent.manifest.model,
            "provider": agent.manifest.provider,
            "persona_source_version": null,
            "start_on_app_launch": false,
            "auto_restart_on_config_change": true,
            "runtime_pid": null,
            "backend": {"type": "local"},
            "backend_agent_id": null,
            "provider_binary_path": null,
            "created_at": created_at,
            "updated_at": prior_updated,
            "last_started_at": null,
            "last_stopped_at": null,
            "last_exit_code": null,
            "last_error": null,
            "last_error_code": null,
            "respond_to": "owner-only",
            "respond_to_allowlist": [],
            "display_name": agent.manifest.name,
            "slug": agent.manifest.slug,
            "runtime": agent.manifest.runtime,
            "name_pool": [agent.manifest.name],
            "is_builtin": false,
            "is_active": true,
            "definition_respond_to": "owner-only",
            "definition_respond_to_allowlist": [],
            "definition_parallelism": 1
        });
        if existing.is_some_and(|old| old != &value) {
            value["updated_at"] = Value::String(now.to_string());
        }
        value
    }

    fn valid_existing_identity(
        existing_definition: Option<&Value>,
        existing: Option<&Value>,
        owner: &PublicKey,
    ) -> Option<(PublicKey, String)> {
        // A definition is the durable signal that this is a provisioned
        // Prosper identity. The earlier rename-only setup had instances but no
        // definitions, so preserving those pubkeys would strand missing keys.
        existing_definition?;
        let pubkey = PublicKey::parse(existing.and_then(|v| string_field(v, "pubkey"))?).ok()?;
        let tag = existing.and_then(|v| string_field(v, "auth_tag"))?;
        let attested = buzz_sdk::nip_oa::verify_auth_tag(tag, &pubkey).ok()?;
        (attested == *owner).then(|| (pubkey, tag.to_string()))
    }

    fn build_instance(
        agent: &LoadedAgent<'_>,
        manifest: &FleetManifest,
        pubkey: &PublicKey,
        auth_tag: &str,
        inline_nsec: Option<&str>,
        existing: Option<&Value>,
        now: &str,
    ) -> Result<Value> {
        let created_at = existing
            .and_then(|v| string_field(v, "created_at"))
            .unwrap_or(now);
        let prior_updated = existing
            .and_then(|v| string_field(v, "updated_at"))
            .unwrap_or(now);
        let mut value = json!({
            "pubkey": pubkey.to_hex(),
            "name": agent.manifest.name,
            "persona_id": agent.manifest.slug,
            "team_id": manifest.team.id,
            "auth_tag": auth_tag,
            "relay_url": manifest.relay_url,
            "avatar_url": null,
            "acp_command": "buzz-acp",
            "agent_command": agent.command,
            "agent_command_override": agent.command,
            "agent_args": [],
            "mcp_command": "",
            "turn_timeout_seconds": 600,
            "idle_timeout_seconds": 300,
            "max_turn_duration_seconds": 900,
            "parallelism": 1,
            "system_prompt": agent.prompt,
            "model": agent.manifest.model,
            "provider": agent.manifest.provider,
            "persona_source_version": agent.source_version,
            "start_on_app_launch": true,
            "auto_restart_on_config_change": true,
            "runtime_pid": null,
            "backend": {"type": "local"},
            "backend_agent_id": null,
            "provider_binary_path": null,
            "created_at": created_at,
            "updated_at": prior_updated,
            "last_started_at": existing.and_then(|v| v.get("last_started_at")).cloned().unwrap_or(Value::Null),
            "last_stopped_at": existing.and_then(|v| v.get("last_stopped_at")).cloned().unwrap_or(Value::Null),
            "last_exit_code": existing.and_then(|v| v.get("last_exit_code")).cloned().unwrap_or(Value::Null),
            "last_error": null,
            "last_error_code": null,
            "respond_to": "owner-only",
            "respond_to_allowlist": [],
            "runtime": agent.manifest.runtime,
            "name_pool": [],
            "is_builtin": false,
            "is_active": true
        });
        if let Some(nsec) = inline_nsec {
            value["private_key_nsec"] = Value::String(nsec.to_string());
        }
        if existing.is_some_and(|old| old != &value) {
            value["updated_at"] = Value::String(now.to_string());
        }
        Ok(value)
    }

    fn build_team(manifest: &FleetManifest, now: &str, existing: Option<&Value>) -> Value {
        let created_at = existing
            .and_then(|v| string_field(v, "created_at"))
            .unwrap_or(now);
        let prior_updated = existing
            .and_then(|v| string_field(v, "updated_at"))
            .unwrap_or(now);
        let mut value = json!({
            "id": manifest.team.id,
            "name": manifest.team.name,
            "description": manifest.team.description,
            "instructions": manifest.team.instructions,
            "persona_ids": manifest.agents.iter().map(|a| a.slug.clone()).collect::<Vec<_>>(),
            "is_builtin": false,
            "is_symlink": false,
            "created_at": created_at,
            "updated_at": prior_updated
        });
        if existing.is_some_and(|old| old != &value) {
            value["updated_at"] = Value::String(now.to_string());
        }
        value
    }

    fn buzz_running() -> bool {
        Command::new("pgrep")
            .args(["-x", "buzz-desktop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn plan(root: &Path, manifest: &FleetManifest, agents: &[LoadedAgent<'_>]) -> Result<()> {
        let (store, _) = store_paths()?;
        let records = read_values(&store)?;
        let existing_instances = agents
            .iter()
            .filter(|a| existing_by_slug(&records, &a.manifest.slug, false).is_some())
            .count();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "mode": "plan",
                "repository": root,
                "relay": manifest.relay_url,
                "fleetAgents": agents.len(),
                "existingFleetInstances": existing_instances,
                "buzzRunning": buzz_running(),
                "willPreserveNonProsperRecords": true,
                "willReuseValidExistingKeys": true
            }))?
        );
        Ok(())
    }

    fn apply(_root: &Path, manifest: &FleetManifest, agents: &[LoadedAgent<'_>]) -> Result<()> {
        if buzz_running() {
            bail!("Buzz Desktop is still running; close it before applying the fleet store");
        }
        let (store_path, teams_path) = store_paths()?;
        fs::create_dir_all(store_path.parent().unwrap())?;
        let old_records = read_values(&store_path)?;
        let old_teams = read_values(&teams_path)?;
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);

        let owner_public = expected_owner_public(&manifest.expected_owner_npub)?;
        let needs_generation = agents.iter().any(|agent| {
            let definition = existing_by_slug(&old_records, &agent.manifest.slug, true);
            let instance = existing_by_slug(&old_records, &agent.manifest.slug, false);
            valid_existing_identity(definition, instance, &owner_public).is_none()
        });
        let owner = needs_generation
            .then(|| owner_keys_from_stdin(&manifest.expected_owner_npub))
            .transpose()?;
        let mut generated = 0usize;
        let mut preserved = 0usize;
        let mut definitions = Vec::new();
        let mut instances = Vec::new();

        for agent in agents {
            let existing_definition = existing_by_slug(&old_records, &agent.manifest.slug, true);
            let existing_instance = existing_by_slug(&old_records, &agent.manifest.slug, false);
            definitions.push(build_definition(agent, &now, existing_definition));
            let (pubkey, auth_tag, inline_nsec) = if let Some((pubkey, auth_tag)) =
                valid_existing_identity(existing_definition, existing_instance, &owner_public)
            {
                preserved += 1;
                (pubkey, auth_tag, None)
            } else {
                generated += 1;
                let owner = owner.as_ref().context("owner recovery key missing")?;
                let keys = Keys::generate();
                let auth_tag = buzz_sdk::nip_oa::compute_auth_tag(owner, &keys.public_key(), "")?;
                let nsec = keys.secret_key().to_bech32().context("encode agent nsec")?;
                (keys.public_key(), auth_tag, Some(nsec))
            };
            instances.push(build_instance(
                agent,
                manifest,
                &pubkey,
                &auth_tag,
                inline_nsec.as_deref(),
                existing_instance,
                &now,
            )?);
        }

        let mut next_records: Vec<Value> = old_records
            .iter()
            .filter(|v| !is_prosper_record(v))
            .cloned()
            .collect();
        next_records.extend(definitions);
        next_records.extend(instances);

        let existing_team = old_teams
            .iter()
            .find(|v| string_field(v, "id") == Some(&manifest.team.id));
        let mut next_teams: Vec<Value> = old_teams
            .iter()
            .filter(|v| string_field(v, "id") != Some(&manifest.team.id))
            .cloned()
            .collect();
        next_teams.push(build_team(manifest, &now, existing_team));

        let changed = next_records != old_records || next_teams != old_teams;
        if changed {
            backup_file(&store_path, "managed-agents", &now)?;
            backup_file(&teams_path, "teams", &now)?;
            atomic_write_json(&store_path, &next_records)
                .and_then(|_| atomic_write_json(&teams_path, &next_teams))
                .context("write fleet stores")?;
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "mode": "apply",
                "changed": changed,
                "ownerNpub": manifest.expected_owner_npub,
                "definitions": agents.len(),
                "instances": agents.len(),
                "preservedIdentities": preserved,
                "generatedKeys": generated,
                "pendingSignedKeychainMigrations": generated,
                "nextStep": if generated > 0 { "launch signed Buzz and wait for inline keys to be migrated and stripped" } else { "none" }
            }))?
        );
        Ok(())
    }

    fn backup_file(path: &Path, label: &str, now: &str) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let stamp: String = now
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(22)
            .collect();
        let dir = path.parent().unwrap().join("backups");
        fs::create_dir_all(&dir)?;
        let backup = dir.join(format!("{label}.before-prosper-provision-{stamp}.json"));
        fs::copy(path, &backup).with_context(|| format!("backup {}", path.display()))?;
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    fn atomic_write_json(path: &Path, values: &[Value]) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(values)?;
        bytes.push(b'\n');
        let temp = path.with_extension(format!("json.tmp.{}", std::process::id()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    fn doctor(root: &Path, manifest: &FleetManifest, agents: &[LoadedAgent<'_>]) -> Result<()> {
        let (store_path, teams_path) = store_paths()?;
        let raw = fs::read_to_string(&store_path).context("read managed agent store")?;
        if raw.contains("private_key_nsec") || raw.contains("nsec1") {
            bail!("managed agent store contains private-key material");
        }
        let records: Vec<Value> =
            serde_json::from_str(&raw).context("parse managed agent store")?;
        let teams = read_values(&teams_path)?;
        let owner = expected_owner_public(&manifest.expected_owner_npub)?;
        let team = teams
            .iter()
            .find(|v| string_field(v, "id") == Some(&manifest.team.id))
            .context("Prosper Fleet team missing")?;
        let team_personas: HashSet<String> = team
            .get("persona_ids")
            .and_then(Value::as_array)
            .context("team persona_ids missing")?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let mut public = BTreeMap::new();

        for agent in agents {
            let definition = existing_by_slug(&records, &agent.manifest.slug, true)
                .with_context(|| format!("definition {} missing", agent.manifest.slug))?;
            let instance = existing_by_slug(&records, &agent.manifest.slug, false)
                .with_context(|| format!("instance {} missing", agent.manifest.slug))?;
            if string_field(definition, "runtime") != Some(&agent.manifest.runtime) {
                bail!("{} definition runtime mismatch", agent.manifest.name);
            }
            if string_field(instance, "runtime") != Some(&agent.manifest.runtime)
                || string_field(instance, "relay_url") != Some(&manifest.relay_url)
                || string_field(instance, "respond_to") != Some("owner-only")
                || instance.get("parallelism").and_then(Value::as_u64) != Some(1)
                || instance.get("start_on_app_launch").and_then(Value::as_bool) != Some(true)
            {
                bail!("{} instance policy mismatch", agent.manifest.name);
            }
            if instance.get("last_error").is_some_and(|v| !v.is_null()) {
                bail!(
                    "{} still has a persisted error: {}",
                    agent.manifest.name,
                    instance["last_error"]
                );
            }
            let command = string_field(instance, "agent_command_override")
                .context("agent command missing")?;
            if command != agent.command.to_string_lossy() || !Path::new(command).is_file() {
                bail!("{} command mismatch or missing", agent.manifest.name);
            }
            let pubkey = string_field(instance, "pubkey").context("agent pubkey missing")?;
            let public_key = PublicKey::parse(pubkey)
                .with_context(|| format!("{} public key invalid", agent.manifest.name))?;
            let tag = string_field(instance, "auth_tag").context("auth tag missing")?;
            let attested = buzz_sdk::nip_oa::verify_auth_tag(tag, &public_key)?;
            if attested != owner {
                bail!("{} auth tag owner mismatch", agent.manifest.name);
            }
            if !team_personas.contains(&agent.manifest.slug) {
                bail!("{} is missing from Prosper Fleet team", agent.manifest.name);
            }
            public.insert(agent.manifest.name.clone(), pubkey[..12].to_string());
        }

        let prosper_definitions = records
            .iter()
            .filter(|v| string_field(v, "slug").is_some_and(|s| s.starts_with("prosper:")))
            .count();
        let prosper_instances = records
            .iter()
            .filter(|v| string_field(v, "persona_id").is_some_and(|s| s.starts_with("prosper:")))
            .count();
        if prosper_definitions != 6 || prosper_instances != 6 {
            bail!("unexpected Prosper record counts: {prosper_definitions} definitions, {prosper_instances} instances");
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "mode": "doctor",
                "repository": root,
                "ownerNpub": manifest.expected_owner_npub,
                "definitions": prosper_definitions,
                "instances": prosper_instances,
                "teamMembers": team_personas.len(),
                "keys": "no private material on disk; live runtime verification required for keychain proof",
                "authTags": "six valid NIP-OA owner attestations",
                "privateKeysOnDisk": false,
                "publicKeyPrefixes": public
            }))?
        );
        Ok(())
    }
}
