# skill-manager

Installs, namespaces, and deploys AI agent skills across multiple agent
harnesses from a single declarative manifest.

Every deployed skill is named `<alias>:<skill>` — `mp:handoff`, `me:codemode` —
so skills from different sources never collide and always carry provenance.

## Install

```sh
nix run .# -- --help          # run it
nix profile install .#        # install it
nix develop                   # cargo, rustc, clippy, rustfmt, git
```

`git` must be on `PATH` for git and github sources; the packaged binary wraps
it in.

## Use

The first run writes a commented template to
`$XDG_CONFIG_HOME/skill-manager/skills.toml` — `~/.config/skill-manager/skills.toml`
by default — and says so instead of running the command. Declare intent there:

```toml
version = 1

[sources.mp]
github = "mattpocock/skills"
skills = ["tdd", "grilling"]

[sources.likec4]
well_known = "https://likec4.dev/"

[sources.me]
path = "~/etc/ai/skills"
```

```sh
skill-manager update            # resolve, fetch, rewrite the lock, deploy
skill-manager apply             # deploy from manifest + lock + store, offline
skill-manager hydrate           # fetch exactly what the lock pins (fresh machine)
skill-manager add mp:wizard     # select a skill, then deploy
skill-manager remove mp:wizard  # deselect a skill, then deploy
skill-manager info [mp:tdd]     # where a deployed skill came from (--json)
skill-manager verify            # read-only convergence check
```

Exit codes: `0` clean, `1` reported failures, `2` aborted.

## Where things live

| Path | What |
| --- | --- |
| `~/.config/skill-manager/skills.toml` | the manifest — the only file a human edits |
| `~/.config/skill-manager/skills.lock` | the lock, committed beside the manifest |
| `~/.local/share/skill-manager/store` | immutable content-addressed upstream content |
| `~/.local/share/skill-manager/deployed` | generated trees and the `current` generation |
| `~/.claude/skills`, `~/.agents/skills` | targets, symlinked through `current` |

## Design

`CONTEXT.md` defines the vocabulary. `docs/manifest.md`, `docs/lock.md`, and
`docs/commands.md` are the contracts; `docs/adr/` records why.
