## Volt Launcher 1.2.1

Two changes on top of 1.2: the APK is signed with the modern signature scheme as
well, and these notes say what to do when Android still asks whether it is safe.

### Fixed

- **The APK is now signed with APK Signature Scheme v3 as well as v1 and v2.**
  Earlier releases carried v1 and v2 only; v3 is the scheme Android has moved to
  since 9, and a scanner that only finds the older schemes has less to check.
  The new signature comes from the same certificate as every previous release,
  so an installed 1.1.x or 1.2 still updates in place.
- **What "may be harmful" means, and what to do.** Play Protect's verdict on an
  APK is made on Google's servers, and an APK Google has never seen uploaded is
  not in their database — a self-built release signed with its own certificate
  lands in that group even when nothing is wrong with it. The build is unchanged
  in what it asks for, the release page carries `SHA256SUMS.txt` so the file can
  be checked, and the installer path is: tap
  **More details → Install anyway** on the dialog. If the dialog offers no such
  link, allow the app you are installing from under **Settings → Apps → Special
  access → Install unknown apps**, then open the file again. Updates downloaded by
  Volt itself go through the system package installer and skip the prompt.

### Notes

- Nothing else changes: this release is 1.2 plus the signing scheme and notes.
- Installing 1.2.1 over 1.2, 1.1.1 or any earlier build updates in place — same
  application id, same signing certificate, higher version code.
- Both channels are rebuilt from this commit: the rolling `debug` release
  (a separate app, "Volt Debug") and this one.
