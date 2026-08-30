# Manifest

The manifest is TOML at `~/etc/ai/skills.toml`.

```toml
version = 1

[sources.mp]
github = "mattpocock/skills"
ref = "main"
skills = ["handoff", "tdd"]

[sources.kepano]
git = "git@github.com:kepano/obsidian-skills.git"
dir = "skills"

[sources.likec4]
well_known = "https://likec4.dev/"

[sources.me]
path = "~/etc/ai/skills"
```

## Source tables

Each `[sources.<alias>]` table has exactly one transport key:

- `git` — a Git or SSH URL
- `github` — `owner/repository` shorthand
- `well_known` — an HTTPS endpoint whose
  `/.well-known/agent-skills/index.json` lists archive URLs and SHA digests
- `path` — a local source path

`sm` is reserved and cannot be used as an alias. Unknown keys are errors.

Optional keys:

- `ref` — Git/GitHub only; branch, tag, or commit intent
- `dir` — Git, GitHub, and local-path sources; narrows discovery to a relative
  directory
- `skills` — every source type; selects directory names, omitted means all

A leading `~` in a path is expanded. Environment-variable interpolation is
not supported.

## Validation

A malformed manifest aborts the command before deployment. A discovered skill
must be a directory containing `SKILL.md` with frontmatter `name` and
`description`. An upstream directory name containing `:` is invalid because
that character separates alias from skill. Invalid individual skills are
reported and skipped; valid skills continue and the command exits non-zero.
