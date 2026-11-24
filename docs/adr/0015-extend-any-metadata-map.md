# Extend any `metadata:` map, keeping values we do not model

ADR-0013 rejected a skill whose `metadata:` was not a plain block map of
string to string. Upstreams write it otherwise: `elevenlabs/skills` uses an
inline flow map with nested values,

```yaml
metadata: {"openclaw": {"requires": {"env": ["ELEVENLABS_API_KEY"]}, "primaryEnv": "ELEVENLABS_API_KEY"}}
```

which is valid YAML, though not the spec's `dict[str, str]`. Rejecting it
blocks the skill outright, and judging upstream conformance to the spec is not
this tool's job; injecting provenance is.

`metadata:` is now extensible when it is any map — a block map or a one-line
flow map `{...}` — whatever its values. Top-level scalar values are read; any
other value (nested lines, a flow collection, a block scalar, an anchor, alias,
or tag) is kept as written and never interpreted. The provenance keys stay
flat `sm-*` scalars beside them.

## Consequences

- An untouched `SKILL.md` still renders byte for byte. A regenerated
  `metadata:` is always emitted as a block map: a flow map's values become
  block entries, collection values staying in flow style. The YAML value is
  unchanged.
- A `metadata:` that is not a map — a sequence, a scalar, an unbalanced flow
  map — is still reported and skipped.
- An injected key is detected whatever the shape of its value, so an upstream
  `sm-digest: {...}` is still rejected rather than overwritten.
- A flow map spanning several lines is not read and fails as before.
