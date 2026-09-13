# Manifest

The manifest is TOML at `$XDG_CONFIG_HOME/skill-manager/skills.toml`,
defaulting to `~/.config/skill-manager/skills.toml`.

Any command run without a manifest writes a template there — `version = 1`
with one commented example per transport — reports that it did so, and exits
without running. Nothing else is created or deployed on that run.

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

Each `[sources.<source-id>]` table has exactly one transport key:

- `git` — a Git or SSH URL
- `github` — `owner/repository` shorthand
- `well_known` — an HTTPS endpoint whose
  `/.well-known/agent-skills/index.json` lists archive URLs and SHA digests
- `path` — a local source path

`sm` is reserved and cannot be used as a source ID. A source ID may contain only
lowercase letters and digits, because it is the part before the first `-` in a
deployed `<source-id>-<skill>` name (ADR-0013). Unknown keys are errors.

Optional keys:

- `ref` — Git/GitHub only; branch, tag, or commit intent
- `dir` — Git, GitHub, and local-path sources; narrows discovery to a relative
  directory
- `skills` — every source type; selects directory names, omitted means all

A leading `~` in a path is expanded. Environment-variable interpolation is
not supported.

A `path` that does not exist is not an error: the source contributes no skills,
`apply` and `verify` report that they skipped it, and both still exit zero. One
manifest is shared across machines, and a local directory need not exist on all
of them. A `path` that exists but is not a directory is a manifest mistake and
aborts. `add` and `remove` name a source outright, so an absent `path` is fatal
there.

## Validation

A malformed manifest aborts the command before deployment. A discovered skill
must be a directory containing `SKILL.md` with frontmatter `name` and
`description`, in a directory whose name matches the agent-skills shape
`^[a-z0-9]+(-[a-z0-9]+)*$` — the deployed `<source-id>-<skill>` has to satisfy it
too. Invalid individual skills are reported and skipped; valid skills continue
and the command exits non-zero. The same applies to a skill whose `metadata:`
block is not a plain key-value map, already defines one of the `sm-` keys this
tool injects, or whose deployed `<source-id>-<skill>` would exceed the spec's
64-character limit (ADR-0013).
