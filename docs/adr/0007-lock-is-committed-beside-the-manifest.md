# The lock is committed beside the manifest

The lock lives next to `skills.toml` in `$XDG_CONFIG_HOME/skill-manager` and
is committed; the store is machine-local in
`~/.local/share/skill-manager/store/`. The lock is precisely what a fresh
machine needs in order to `hydrate`, so it has to be in the repo you clone — this is what the current setup gets wrong, with
declared intent in the config repo and resolved reality in
`~/.agents/.skill-lock.json` outside it.

Viable only because `apply` does not write the lock (see ADR-0005); if it
did, a committed lock would mean a permanently dirty config repo.

The file is `skills.lock` beside the manifest, encoded as TOML with top-level
`version = 1`. Sources and skills are serialized in deterministic key order,
and no timestamps appear in the file.
