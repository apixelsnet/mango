# Security

## Reporting a problem

Report it privately: on the repository's **Security** tab, choose **Report a
vulnerability**. Please do not open a public issue for something that lets a
visitor or an account do what it should not.

Say which version it is (Settings, About, or `mango version`), how Mango was
installed (image, archive or desktop shell), and the steps that show the problem.

Mango is kept in spare time. A report is read and answered; how fast a fix
follows depends on what it is.

## What gets fixes

The newest release. Older ones are not patched: the fix is to move up.

## What counts

- Getting in without the password, or setting the first one without the setup code.
- One account reading or changing another's library, sign-ins or settings.
- A reader doing what is for an admin.
- A page on another site making Mango change something.
- Mango fetching an address on its own machine or network because an account named it.
- Reading or writing files outside the data folder.
- An extension installed though it is not signed with its store's key.

## What does not

- What an extension does once it is installed. Extensions are other people's
  code and run on the server as the user Mango runs as; whoever brings one in
  trusts it. Problems in an extension go to whoever makes it.
- A Mango started with `MANGO_LOGIN=off`, `MANGO_STORES_LOCAL=on` or
  `MANGO_ANYONE_INSTALLS=on` doing what those switches open.
- Problems in the Suwayomi-Server runtime the extension host is built on. Those
  go to Suwayomi-Server; do say so here as well when a Mango release should move
  to a fixed version.

Why the defaults are what they are is written down in
[docs/features/02-extension-store.md](docs/features/02-extension-store.md) and
[docs/features/09-packaging-auth.md](docs/features/09-packaging-auth.md).
