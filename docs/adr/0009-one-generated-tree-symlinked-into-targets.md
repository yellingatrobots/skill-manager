# One generated tree, symlinked into every target

`apply` generates each namespaced skill directory exactly once as part of a
complete immutable generation under
`~/.local/share/skill-manager/deployed/generations/<tree-digest>/`. The ID is
the digest of the complete generated tree. A stable `deployed/current` symlink
is atomically replaced only after the generation is complete; if it already
points to that digest, `apply` is a no-op. Each target entry is a directory symlink through `current` to that
skill, so content changes never require rewriting target entries.

Probes (2026-08-29) showed both harnesses discover symlinked skill
*directories* — it is the symlinked `SKILL.md` *file* that Codex misses, which
is what forces generation in ADR-0002 in the first place.

## Considered Options

A full physical copy per target doubles the write work on every rebuild and,
worse, forces target ownership to fall back on name-matching: "ours" becomes
"anything whose name matches a manifest entry", which deletes a name-colliding
stranger as a false positive.

Rewriting a live generated directory in place was rejected because a harness
could observe missing or partially written content. Building a complete
immutable generation and atomically switching `current` keeps every visible
generation internally complete.

## Consequences

Target ownership is stateless and exact: a target entry is ours **iff it is a
symlink into the build directory**. No placed-items ledger is needed; such
secondary state can go stale and strand orphans.

- Obsolete generations are pruned after `current` switches.
- Deleting from a target means removing the symlink only — the deletion code
  must never recurse through a symlink it did not create.
- `verify` checks generated content, the `current` generation, every expected
  target symlink, and the absence of stale or foreign deployed entries.

Targets are fixed in v1: `~/.claude/skills` and `~/.agents/skills`.
`~/.codex/skills` is redundant because Codex discovers through
`~/.agents/skills`.
