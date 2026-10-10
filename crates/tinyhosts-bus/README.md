# tinyhosts-bus

The minimal vocabulary for callers of the compiled TinyHosts module. Dependencies
are restricted to serde, JSON serialization, and error derives; no feature links
a provider, TinyBus transport, HTTP client, filesystem collector, or runtime.

`Execute(String) -> String` accepts the existing JSON request and returns the
existing tagged result envelope. `Providers() -> String` returns provider slugs.
Request and Operation have generic input slots so the library retains its
validated Bundle and LaunchPlan APIs while hosts use `DeploymentInput` and
`LaunchInput` DTOs. File contents remain standard-base64 strings on the wire;
encoding, path validation, collection, and execution remain implementation work.
Requests and credential inputs are deserialize-only, with redacted Debug output.

Shared hosting records, errors, schemas, result envelopes, and recorded tool
declarations are re-exported by the implementation for compatibility. Host
approval, authorized workspace selection, credential custody, and lifecycle
policy stay with OpenHuman. The module validates inputs and talks to providers.
The package version follows the module release version; the release workflow
bumps both manifests together. Hosts pin published artifacts and verify digests.


Operation vocabulary `WIRE_CONTRACT_VERSION = (1, 1)` adds `prepare_bundle` to
Execute, without changing method arities or model tool declarations. The host
constructs `AuthorizedDirectory` only after authorizing a concrete canonical
workspace and relative input. The module returns a stateless `PreparedBundle`
containing actual files and bounded facts. Approve and deploy this snapshot's
exact bytes, rather than recollecting the directory. The DTO is a scope declaration,
not an authorization token, and must never be constructed from generic model
forwarding. See the implementation README for byte/frame limits, credential
exclusions and the stricter no-symlink source policy. Filesystem traversal and
base64 encoding are absent from this contract crate.
