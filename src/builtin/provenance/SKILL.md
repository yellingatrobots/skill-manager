---
name: sm-provenance
description: Report where an installed skill came from. Use when asked who wrote a skill, which source or repository a skill came from, whether a skill is up to date, what `<source-id>-<skill>` means, or why a skill is installed at all.
---

# Skill provenance

Every skill deployed by `skill-manager` is named `<source-id>-<skill>`, where
`<source-id>` identifies the source it came from and `<skill>` is its upstream
directory name. The source ID is the part before the first `-`.

The deployed `SKILL.md` carries provenance under its frontmatter `metadata:`
key, prefixed `sm-`: `sm-source-id`, `sm-source-kind`, `sm-source-url`,
`sm-upstream-name`, `sm-upstream-path`, `sm-upstream-coordinate`, and
`sm-digest`. The authoritative record is the tool itself. Run:

```
skill-manager info <source-id>-<skill>
```

for one skill, or `skill-manager info` for every deployed skill. Add `--full`
for every provenance field plus where the manifest, lock, store, deployed tree,
and targets live, or `--json` for a machine-readable
`{ "locations": {…}, "skills": […] }` object.

Do not infer origin from the skill body: third-party skills reference each
other by bare, unnamespaced names, and those references are not rewritten.
