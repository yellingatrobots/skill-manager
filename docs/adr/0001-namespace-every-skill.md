# Namespace every skill with its source alias

Skills from different sources collide by name and carry no provenance:
nothing tells you that `handoff` came from mattpocock rather than from your
own repo. Every deployed skill is therefore named `<alias>:<skill>` —
`mp:handoff`, `me:codemode` — with no unprefixed class, because "no
namespace" as a special case would infect every code path and bare names are
exactly where collisions land.

## Considered Options

Claude Code's native plugin mechanism gives free `plugin:skill` namespacing,
but it is Claude-only and the prefix string is not ours to choose. Codex
would still see colliding bare names.

## Consequences

Third-party skills reference each other by bare name, and those references
break under prefixing. Accepted: the agent is expected to cope. The tool does
not rewrite skill bodies and does not scan for or warn about broken
references.
