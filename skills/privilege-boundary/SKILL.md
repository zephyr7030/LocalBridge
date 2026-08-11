
# Privilege Boundary
- 主进程永不整体提权。
- Elevated 只经 Broker。
- Broker：UAC、authenticated local IPC、generation/session、replay/stale defense。
- structured args/workdir；no shell default；timeout/cancel/output bound/redaction。
- control-plane 永久 deny。
