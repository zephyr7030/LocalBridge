# Spikes

`spikes/` contains disposable compatibility/proof-of-concept work that is explicitly authorized by a PR contract.

Production code must never depend on spike code.

For LB-000:

- Job Object PoC;
- portable Python PoC;
- upstream MCP probes;
- tunnel-client live probe helpers;
- path/reparse adversarial probes.

After the ADR/baseline is produced, spike code remains evidence only and must not become runtime implementation.

LB-000 canonical evidence is kept as small reproducible probes/results under `spikes/lb-000/` plus frozen snapshots under `compatibility/`. Downloaded upstream source trees, binaries and embedded-runtime staging directories are temporary probe inputs and are not product runtime payloads.
