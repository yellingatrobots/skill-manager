---
name: sm:provenance
description: Report where an installed skill came from. Use when asked who wrote a skill, which source or repository a skill came from, whether a skill is up to date, what `<alias>:<skill>` means, or why a skill is installed at all.
---

# Skill provenance

Every skill deployed by `skill-manager` is named `<alias>:<skill>`, where
`<alias>` identifies the source it came from and `<skill>` is its upstream
directory name.

The deployed `SKILL.md` frontmatter carries `source_alias`, `upstream_name`,
`upstream_url`, and `upstream_coordinate`, but the authoritative record is the
tool itself. Run:

```
skill-manager info <alias>:<skill>
```

for one skill, or `skill-manager info` for every deployed skill. Add `--json`
for a machine-readable record.

Do not infer origin from the skill body: third-party skills reference each
other by bare, unnamespaced names, and those references are not rewritten.
