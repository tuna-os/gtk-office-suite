# ADR 0013: One Matrix account for Mandelbrot and the collaboration sidecar

Status: accepted (decision only; the sign-in plumbing is the follow-up below).
Issue: #1973, part of #1962 (INTEGRATION-PLAN B7 and A4).

## Context

Two programs in the suite speak Matrix, and each has its own login and its own
store:

- **Mandelbrot** (`mandelbrot/`, a Fractal fork) is the chat client.
- **`collabd`** (`collab/`, RFC-0005) is the sidecar that carries document
  collaboration over Matrix (`dcp-matrix`) or Durable Streams.

Both resolve `matrix-sdk` **0.19.1** today (`collab/dcp-matrix/Cargo.toml` pins
`=0.19.1`; `mandelbrot/Cargo.lock` and `collab/Cargo.lock` agree), in separate
lockfiles, because the product lockfile must stay free of the network stack
(RFC-0005 option B, `tests/test_collab_defaults.py`). A person who uses both
would sign in twice, to two stores, for what is one identity.

## Decision

1. **The sidecar signs in on its own, as a second device of the same Matrix
   account.** Mandelbrot does not hand it a token, and the two processes share
   no session. The alternative, Mandelbrot owning the session and lending the
   sidecar a scoped access token over D-Bus, gives one place to sign out, but a
   compromised sidecar (the process that parses remote documents) would then
   hold a credential for the person's chat. As a separate device it holds only
   its own, and Matrix's own device list is the one place to see and revoke it.
2. **Key material is never shared between the processes.** Cross-signing keys,
   the key backup and room keys stay in the store of the device that made them.
   The sidecar gets its own device keys and is verified by the person from
   Mandelbrot like any other new device.
3. **Both stay on the same `matrix-sdk` minor.** The SQLite store schema is
   versioned with the crate, and a session store written by one program must
   not be opened by a different schema. `tests/test_matrix_sdk_pin.py` fails if
   the two lockfiles (or `dcp-matrix`'s pin) disagree, so a bump of one is a
   bump of both, in the same change.
4. **Storage and sign-in.** The sidecar keeps its credentials in the Secret
   portal (RFC-0005 §3: "Credentials are not in the command"). Signing in is
   the sidecar's own `Login` command (user id and password, or the SSO/OAuth
   flow).

## What "sign in once" then means

The person signs in to Mandelbrot once. When they first use document
collaboration, the sidecar is offered the **non-secret** facts Mandelbrot
already knows (homeserver URL and user id) and runs its own SSO/OAuth flow, so
the browser session they already have makes it one click and no second
password. This is a hint passed over the suite's D-Bus hand-off surface (see
#1974); no token, key or session crosses.

## Consequences

- A compromised sidecar cannot read Mandelbrot's keys or messages.
- Signing out of Mandelbrot does not sign out the sidecar. Sign-out of
  collaboration is removing the sidecar's device, which Mandelbrot can offer
  from its own device list.
- Two devices means two sets of device keys to verify; the first collaboration
  sign-in shows the usual verification prompt in Mandelbrot.
- A `matrix-sdk` upgrade is one coordinated change across `mandelbrot/` and
  `collab/`, guarded by the pin test.

## Not decided here

- Matrix OIDC versus a GNOME Online Accounts entry for Matrix (INTEGRATION-PLAN
  B7): GNOME Online Accounts has no Matrix provider today, so sign-in stays
  in-app.
- The D-Bus method that carries the homeserver and user id hint: it belongs
  with the `open-room`/`start-call` actions of #1974, in `mandelbrot/src/hand_off.rs`.
