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

**Alias**:
The short, unique handle for a source (`mp`, `me`). It is the source's table
key in the manifest, so uniqueness is structural.

**Namespaced name**:
A deployed skill's identity, `<alias>:<skill>` — `mp:handoff`. Written into
both the deployed directory name and the generated frontmatter `name:`.
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
The origin facts carried with a deployed skill: source alias, upstream
coordinate, and `upstream_name` — the original frontmatter `name:` that
namespacing overwrote.

**Hydrate**:
Populating the store from the lock without resolving anything new. The
fresh-machine step.
