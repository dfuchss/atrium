# Changelog

All notable changes to Atrium are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[Semantic Versioning](https://semver.org/) (`X.Y.Z`); a release is tagged `vX.Y.Z`.

## [Unreleased]

First release.

### Added

- All your messengers in one window with a compact sidebar, unread badges, a dock badge and a menu bar icon
- Every account in its own isolated web view, so several accounts of the same service work side by side
- App menu with keyboard shortcuts, launch at login, start hidden and keep running in the background
- Built-in services for Element / Matrix, Slack, Discord, Rainbow, LinkedIn and Mastodon
- Custom services created in the settings
- Element starts reliably after a restart instead of getting stuck on a stale session lock
- Built-in services for WhatsApp, Microsoft Teams and Bluesky
- Accurate Element unread counts, read from the page with the window title as fallback
- Per-account zoom, user agent, custom CSS, site icon, camera and microphone permission, and enable/disable
- Reorder accounts by drag and drop, choose the icon size, pick a Simple Icons glyph, and let inactive services sleep to free memory
- Open and reload the services folder from the settings; sleeping services show a grayscale icon
- Native notifications that open the right account, with Do Not Disturb and per-account mute
- Links open in your browser of choice or in an app window that keeps the account's login
- A per-account list of hosts that always open in the app
- Camera and microphone are only available to the service's own sites
