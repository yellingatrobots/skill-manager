# Generate the deployed tree; never mutate upstream content

Namespacing requires the prefix to appear in both the directory name (Claude
Code's identity key) and the frontmatter `name:` (Codex's identity key), so a
deployed skill cannot be the upstream bytes unchanged. `apply` therefore
generates a real directory `mp:handoff/` whose `SKILL.md` is produced from
the pristine original with `name:` rewritten and provenance injected, while
the store copy is never edited.

## Considered Options

Both alternatives were probed empirically against Claude Code 2.1.245 and
Codex 0.149.0, not read from documentation.

Pure symlink deployment is dead: Codex does not discover a symlinked
`SKILL.md` file (a symlinked skill *directory* is discovered by both, which
is why the existing nix-store setup works).

A shim `SKILL.md` deferring to a pristine `SKILL_ORIGINAL.md` was probed
working in both harnesses but is rejected — it must duplicate the
`description` verbatim anyway, since that is the trigger text, so it buys
nothing and costs an extra file read plus a model-compliance gamble on every
invocation.

## Consequences

Generated frontmatter carries `source_alias`, `upstream_name`, `upstream_url`,
and `upstream_coordinate`. It carries no deployment timestamp: generated
output must remain a pure function of manifest plus lock plus store.

For a local-path source, `upstream_url` preserves the configured manifest path
and `upstream_coordinate` is the current semantic content digest. `info` may
also display the resolved absolute path at runtime; the local source remains
absent from the lock.

A built-in `sm:provenance` skill, embedded in the binary and deployed on every
`apply`, tells harnesses to use `skill-manager info` for authoritative origin
information. `sm` is a reserved source alias and cannot appear in the
manifest.
