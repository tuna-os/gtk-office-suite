# Test-only TLS material

`dev_tls_m.localhost.key`, `dev_tls_m.localhost.crt` and `dev_tls_local-ca.crt`
are Element Call's published development certificate for `*.m.localhost`,
issued by its "Element Call Dev CA", copied verbatim from element-call's
`backend/` directory. The private key is public by design.

They exist only so the local MatrixRTC stack in `../compose.yml` (LiveKit and
its JWT service behind nginx) can terminate TLS during the e2e tests. They
protect nothing: the hostnames resolve to localhost and the CA is trusted only
by the test harness.

**Never** reuse this key or CA for anything real, and never add the CA to a
system trust store. A secret scanner that reports the key is reporting a known
public test fixture, not a leak.

Generating a fresh key and CA at the start of each run would remove the file
from the tree, but changes how the stack's clients trust the CA; that needs the
e2e stack to verify, so it is left until someone can run it.
