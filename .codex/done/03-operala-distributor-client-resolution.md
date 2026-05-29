# PR 03 — Distributor client resolution for answers and SoRLa refs

## Goal

Use `greentic-distributor-client` to resolve non-file references for:

- `--answers`
- `answers.json.sorla.source`
- extension templates when later published through stores/repos/OCI

## Required supported schemes

```text
file:// or plain path
oci://
store://
repo://
```

## CLI examples

```bash
greentic-operala wizard --answers ./answers.json
greentic-operala wizard --answers oci://ghcr.io/greenticai/customer/answers/rent-recon:1.0.0
greentic-operala wizard --answers store://customer-store/demo-tenant/operala/answers/rent-recon
greentic-operala wizard --answers repo://customer-repo/operala/rent-recon.answers.json
```

## Implementation

Add crate:

```text
crates/operala-distribution
```

API:

```rust
pub enum ResolvedArtifact {
    LocalPath(PathBuf),
    Bytes(Vec<u8>),
    Json(serde_json::Value),
    Yaml(serde_yaml::Value),
}

pub trait ArtifactResolver {
    async fn resolve(&self, reference: &str, tenant: Option<&str>, team: Option<&str>) -> Result<ResolvedArtifact>;
}
```

Resolution returns bytes or structured documents only. SoRLa YAML is then parsed
by `greentic-sorla-lib`; OperaLa must not infer SoRLa semantics from raw YAML
with ad-hoc string matching.

## Security

- Verify OCI digest when present.
- Do not execute files resolved from remote refs.
- Cache resolved artifacts under `~/.greentic/cache/operala`.
- Record resolved digest in build metadata.
- Keep tenant/team context explicit in resolution metadata and never store
  plaintext provider credentials in build locks.

## Acceptance criteria

- Local file answers still work.
- OCI/store/repo answers resolve via distributor client abstraction.
- Resolved digest is stored in `operala.build.lock`.
- Resolved SoRLa source records its digest and parser/schema version in the
  handoff metadata.
