# A skill is any directory with a `SKILL.md`, named for that directory

Skills are found by walking a source for directories containing a `SKILL.md`,
with an optional per-source `dir = "skills"` in the manifest to narrow the
walk. The `<skill>` half of `mp:<skill>` is the **directory name**, not the
frontmatter `name:`.

Source layouts genuinely differ — `skills/<name>/SKILL.md` in one upstream, a
tarball root in another — so a hardcoded search order would need maintenance
every time a convention shifts. The directory name is what every source's
README advertises and what a human would type into the manifest's
`skills = [...]` filter.

## Consequences

The two candidate names can disagree, which the harness probes confirmed
happens. The frontmatter `name:` is recorded as `upstream_name` in the
generated frontmatter and in the lock, then discarded — it is the only thing
the namespacing rewrite destroys.
