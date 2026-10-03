# Security policy

Letters, Tables and Decks open documents from anywhere: email attachments,
downloads, shared drives. A file crafted to crash an app, exhaust its memory,
read or write files it shouldn't, or run code is a security problem, and so
is anything else that lets a document or another program do more than the
user asked for.

## Reporting a vulnerability

**Please don't report security problems in a public issue, discussion or
pull request.**

Report them privately with GitHub's **Report a vulnerability** button on the
repository's [Security tab](https://github.com/tuna-os/gtk-office-suite/security).
That opens a private advisory that only you and the maintainers can see.

If the button isn't there, open a public issue titled "Security contact
request" with no details of the problem, and a maintainer will arrange a
private channel.

Include, as far as you can:

- the app and version (the About dialog, or the commit you built);
- what an attacker can do, and what they need (a file the user opens, a
  particular setting);
- steps or a proof-of-concept file to reproduce it.

## What happens next

- We aim to acknowledge a report within **7 days**, and to tell you within
  **30 days** whether we accept it and what the plan is.
- We fix accepted issues privately, then publish the fix with an advisory
  that credits you, unless you'd rather not be named.
- Please give us **90 days** from your report before disclosing it
  publicly, or less if the fix ships sooner. If a vulnerability is already
  being exploited, tell us and we'll move faster.

## Supported versions

Fixes go into `main` and the next release. The project has not yet tagged
a stable release, so there are no older branches to patch.

## Dependencies

Known advisories in our dependencies are tracked in `deny.toml` and checked
by `cargo deny` in CI, with each accepted exception explained there. If you
find a dependency vulnerability that affects the apps and isn't listed,
report it as above.
