# Namespace with `-`, and prefix provenance keys under `metadata:`

`<alias>:<skill>` is outside the agent-skills spec, which constrains a skill's
`name:` to 1–64 characters of `^[a-z0-9]+(-[a-z0-9]+)*$`, and `:` in a
directory name is awkward for shells, paths, and harness tooling. Deployed
skills are named `<source-id>-<skill>` instead — `mp-handoff`, `me-codemode` —
and the source ID is the part before the first `-`.

The same spec defines `metadata` as **a map from string keys to string
values**, and recommends "making your key names reasonably unique to avoid
accidental conflicts". Nesting our keys under a `skill-manager:` sub-map would
be a map value where the spec says string, and the reference validator declares
`metadata: dict[str, str]`. Provenance is therefore prefixed, not nested:

```yaml
metadata:
  sm-source-id: mp
  sm-source-kind: github
  sm-source-url: https://github.com/mattpocock/skills.git
  sm-upstream-name: handoff
  sm-upstream-path: skills/handoff
  sm-upstream-coordinate: git:<commit-sha>
  sm-digest: sha256:<content-digest>
```

`sm-source-*` describes the source; `sm-upstream-*` describes this skill where
it came from. The generated tree stays a pure function of manifest plus lock
plus store: neither a timestamp nor the tool version enters `metadata:`.

This supersedes the `<alias>:<skill>` naming of ADR-0001 and the flat
`source_alias`/`upstream_*` keys of ADR-0002. It also renames the concept those
ADRs call an **alias**: it is a **source ID** throughout the code, the CLI, and
the current-state docs. Earlier ADRs keep their original wording as records.

## Consequences

A source ID may contain only lowercase letters and digits. A `-`, `_`, `.`, or
capital would either make the split ambiguous or push the deployed name outside
the spec shape, so those manifests are now rejected. No existing `skills.toml`
changes shape: the source ID is the `[sources.<source-id>]` table key.

Four ways a skill is now reported and skipped, while its siblings deploy:

- an upstream directory name outside `^[a-z0-9]+(-[a-z0-9]+)*$`, replacing the
  old "no `:`" rule
- a `metadata:` block that is not a plain key-value map, since provenance
  cannot be injected into a shape we do not model
- a `metadata:` block that already defines one of the seven injected keys —
  overwriting it would discard something whose purpose we do not know
- a deployed `<source-id>-<skill>` longer than 64 characters

The first three are checked during discovery, so `update` will not lock a skill
that `apply` would refuse; the length check needs the source ID and so runs
when the deployment is planned.

Generated output changed shape as well as content. Every frontmatter key is now
re-emitted byte for byte — comments, blank lines, block scalars, and shapes the
parser does not model all survive — and only `name:` and `metadata:` are
regenerated. Upstream keys under `metadata:` that we do not inject, `sm-`
prefixed or not, are preserved.

Every deployed skill changes name on upgrade. The next `apply` removes the old
`alias:skill` entries from every target and links the new ones; nothing else
migrates.
