# skill-manager

Installs, namespaces, and deploys AI agent skills across multiple agent
harnesses from a single declarative manifest.

## Language

**Skill**:
A directory containing a `SKILL.md` — frontmatter `name` and `description`,
plus a body — and optional bundled files. The unit this tool moves.

**Source**:
An upstream that supplies skills: a git repo, GitHub shorthand, a well-known
HTTPS endpoint, or a local path.
_Avoid_: package, registry, repo

**Source ID**:
The short, unique handle for a source (`mp`, `me`) — lowercase letters and
digits only, so a deployed name splits unambiguously at its first `-`. It is
the source's table key in the manifest, so uniqueness is structural, and it is
recorded in a deployed skill as `sm-source-id`.
_Avoid_: alias, prefix, namespace

**Namespaced name**:
A deployed skill's identity, `<source-id>-<skill>` — `mp-handoff`. Written into
both the deployed directory name and the generated frontmatter `name:`, and
shaped to what the agent-skills spec allows for a `name:` (ADR-0013).
_Avoid_: prefixed name, qualified name

**Manifest**:
The declaration of intent, at `$XDG_CONFIG_HOME/skill-manager/skills.toml`
(`~/.config/skill-manager/skills.toml` by default). The only file a human
edits, and the tool writes a commented template there on first run.
_Avoid_: config, skills list

**Lock**:
The record of resolution: for each deployed skill, the upstream coordinate it
came from and the content digest of what landed.
_Avoid_: lockfile registry

**Store**:
Immutable, content-addressed storage of pristine upstream skill content.
Written only by `update` and `hydrate`; never edited.
_Avoid_: cache, originals repository, vendor directory

**Deployed tree**:
The generated, namespaced skill directories that harnesses read. A build
output — deletable and reproducible from manifest plus lock plus store.
_Avoid_: install

**Harness**:
An agent product that discovers and runs skills. Claude Code and Codex are
the two that exist here.
_Avoid_: agent, client

**Target**:
A filesystem path a harness discovers skills in. One harness may have more
than one.

**Upstream coordinate**:
What identifies content at its origin — a resolved commit SHA, a tarball
digest. Distinct from the content digest of what was extracted.

**Provenance**:
The origin facts carried in a deployed skill's frontmatter `metadata:` block,
each key prefixed `sm-` because the spec's `metadata` is one flat map
(ADR-0013): `sm-source-id`, `sm-source-kind`, and `sm-source-url` describe the
source; `sm-upstream-path`, `sm-upstream-coordinate`, `sm-digest`, and
`sm-upstream-name` — the original frontmatter `name:` that namespacing
overwrote — describe this skill at its origin.

**Hydrate**:
Populating the store from the lock without resolving anything new. The
fresh-machine step.
