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
