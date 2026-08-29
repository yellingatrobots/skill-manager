# One generated tree, symlinked into every target

`apply` generates each namespaced skill directory exactly once, into a build
directory (`~/.local/share/skill-manager/deployed/`), and places a directory
symlink in each target pointing at it. Probes (2026-08-29) showed both
harnesses discover symlinked skill *directories* — it is the symlinked
`SKILL.md` *file* that Codex misses, which is what forces generation in
ADR-0002 in the first place.

## Considered Options

A full physical copy per target doubles the write work on every rebuild and,
worse, forces target ownership to fall back on name-matching: "ours" becomes
"anything whose name matches a manifest entry", which deletes a name-colliding
stranger as a false positive.

## Consequences

Target ownership is stateless and exact: a target entry is ours **iff it is a
symlink into the build directory**. No placed-items ledger is needed; such
secondary state can go stale and strand orphans.

- The build directory is itself converged: entries not derivable from the
  current manifest plus lock are pruned, so garbage cannot hide there.
- Deleting from a target means removing the symlink only — the deletion code
  must never recurse through a symlink it did not create.
- Content updates do not touch the targets at all: the symlink path is stable,
  so swapping the build-directory contents is the whole update.

Targets are `~/.claude/skills` and `~/.agents/skills`; `~/.codex/skills` is
redundant because Codex discovers through `~/.agents/skills`.
