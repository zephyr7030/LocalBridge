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
