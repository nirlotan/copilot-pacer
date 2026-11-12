# Privacy policy

Copilot Pacer has no analytics, no telemetry and no servers of its own.

This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it.

## What it sends, and where

Copilot Pacer talks only to **GitHub**, the service you sign in to:

- **Sign-in:** when you choose to sign in, it uses GitHub's device sign-in
  (github.com/login), or reuses the GitHub CLI's sign-in if you have one (`gh auth token`).
- **Your Copilot usage:** while you're signed in, it reads your own Copilot credit usage
  from GitHub's API (api.github.com) every few minutes, as often as you set in Settings.
- **Update checks:** once a day it asks GitHub Releases whether a newer Copilot Pacer exists,
  and downloads the installer only if you choose to update. Turn this off in Settings
  (**Check for updates**).

These requests carry your GitHub sign-in, so GitHub knows they come from your account. See
[GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).

To find your network's proxy, Copilot Pacer asks your operating system, which may read
your network's proxy configuration the way your browser does. Nothing about you or your
usage is sent in the process.

## What stays on your computer

- Your GitHub sign-in token, in the macOS Keychain or the Windows Credential Manager.
  Signing out deletes it.
- Your settings and a small usage history, in the app's data folder.

Public-holiday calendars come with the app ([date-holidays](https://github.com/commenthol/date-holidays))
and need no connection.
