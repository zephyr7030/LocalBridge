# Release Artifacts

Generated release evidence is written here during LB-018/LB-019.

Expected examples:

```text
release-artifacts/<version>/
├─ sbom.*
├─ provenance.json
├─ size-report.json
├─ compatibility-verification.json
└─ acceptance-report.md
```

Large installer binaries do not need to be committed to Git merely because this directory is writable.
