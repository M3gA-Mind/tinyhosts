# Minimal hosting contract

Hosts execute hosting operations in the compiled TinyHosts artifact via the
existing `Execute(String) -> String` and `Providers() -> String` members. No
member arity, request tag, result tag, credential input, tool name, or schema
changes. Shared vocabulary lives in `tinyhosts-bus`, with compatibility
re-exports in the library. Provider transport, deployment order, bundle
collection, base64 encoding, and credential validation stay in the implementation.

The pure RPC Operation enum has generic plan and deployment payload slots.
The library aliases bind these to validated LaunchPlan/DeployRequest; bus hosts
use the data-only LaunchInput/DeploymentInput. Both serialize identically.
Module deserialization still rejects unsafe bundle paths before provider I/O.
Credential fields remain deserialize-only, and their Debug representation is
redacted. Hosts pass credential envelopes through confidential bus calls.

Recorded tool declarations include external-effect metadata for host approvals.
The host validates authorized workspace input before transferring files; the
module owns deployment sequencing and provider requests. Unavailable modules
fail the affected operation without a linked implementation fallback.

The contract package version is synchronized with the module release. Consume
this owner change only after upstream availability and pin released artifacts
with verified digests; never represent a local build as a release.
