# Security policy

## Supported versions

Security fixes go into the latest release. CineVault checks for updates on its own and verifies every update against a signature built into the app, so please update to the newest version before reporting.

## Reporting a vulnerability

Please **do not open a public issue** for a security problem. Report it privately through GitHub's security advisories instead:

<https://github.com/mustafayevt/CineVault/security/advisories/new>

Include what you found, how to reproduce it, the CineVault version and your operating system. You will get an answer as soon as possible, and you will be kept informed while a fix is prepared. Once a fix is released, the advisory is published, with credit to you if you want it.

Things that are especially worth reporting:

- a way for CineVault to delete, overwrite or move files outside what the user asked for;
- API keys or other secrets ending up in logs, NFO files, exports or anywhere outside the OS credential store;
- a way to make the app accept an update that was not signed with the release key;
- a way for another user or a remote client to reach the local channel the `cinevault` command line uses to talk to a running app.
