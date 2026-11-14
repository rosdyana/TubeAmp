# Installing TubeAmp on macOS

TubeAmp is **not signed with an Apple Developer ID and not notarized** yet. It is ad-hoc signed, which
is enough to run, but macOS Gatekeeper blocks it the first time. This guide covers that one-time
step. Every app update may also ask for the audio permission again (see "Updates" below).

**Requirements:** macOS 14.2 (Sonoma) or later. Apple silicon and Intel Macs are both supported by the
universal build.

## 1. Download and open the DMG

1. Download `TubeAmp-<version>-macos.dmg` (or the `.zip`) from the release page.
2. Double-click the DMG to mount it. A `TubeAmp` window opens with `TubeAmp.app` and an
   `Applications` shortcut.
3. Drag `TubeAmp.app` onto `Applications`, then eject the disk image.

The `.zip` contains the same `TubeAmp.app`. Unzip it and drag it to `Applications` yourself.

## 2. First launch (Gatekeeper)

Because the app is not notarized, macOS refuses the first double-click. Use one of these:

**Option A: right-click (macOS 14 Sonoma)**

1. In `Applications`, right-click (or Control-click) `TubeAmp` and choose **Open**.
2. Click **Open** in the dialog.

**Option B: System Settings (macOS 15 Sequoia and later)**

Sequoia removed the right-click bypass for apps that are not notarized.

1. Double-click `TubeAmp` once. macOS shows a warning and refuses to open it. Click **Done**.
2. Open **System Settings ▸ Privacy & Security**, scroll to the security section, and click
   **Open Anyway** next to the TubeAmp message. Confirm with your password or Touch ID.

**Option C: Terminal**

```sh
xattr -dr com.apple.quarantine /Applications/TubeAmp.app
```

This removes the download quarantine flag from the app. Only do this for a copy you downloaded from
the official release page.

You only need to do this once per downloaded copy.

## 3. Allow System Audio Recording

TubeAmp sends YouTube Music's audio through its equalizer and visualizer. macOS needs your permission to
capture system audio for that. The audio never leaves your Mac.

1. The first time playback starts, macOS asks whether TubeAmp may record system audio. Click **Allow**.
2. If you missed it or clicked **Don't Allow**, open **System Settings ▸ Privacy & Security ▸ Screen &
   System Audio Recording**, turn on **TubeAmp**, then quit and reopen TubeAmp.

TubeAmp takes over YouTube Music's output while it captures the audio. If macOS refuses the permission,
the capture is silent, so TubeAmp watches for that: when YouTube Music is playing but TubeAmp hears
nothing for about 4 seconds, it **switches the EQ off and hands the output back to YouTube Music**, so you
still hear the music. The marquee then reads **EQ OFF: ALLOW SYSTEM AUDIO RECORDING FOR TUBEAMP** for about
8 seconds.

To get the EQ and visualizer back after allowing the permission, use either:

- the **O** menu's **Retry audio capture** item, or
- the EQ window's **ON** switch (turning it on retries the capture).

TubeAmp does not retry by itself after such a switch-off, so it will not keep interrupting playback. A
restart also retries.

## 4. Sign in to YouTube Music

TubeAmp plays YouTube Music in an embedded page, so your library, likes and Premium work after you sign
in there once.

1. Press **L** (or **Alt+V**) to open the video window. It hosts the YouTube Music page.
2. Use the browser-view button on the video window's bottom bar to show the whole page, then sign in
   with your Google account.
3. Your session is kept between launches. You only need to sign in again if you sign out or Google
   expires the session.

## Updates

With ad-hoc signing, the app's code identity changes on every build. macOS may therefore ask for the
**System Audio Recording** permission again after you install an update, and Gatekeeper may ask you to
allow the new copy as in step 2. Your settings and YouTube Music sign-in are kept, because they live in
the config folder below, not in the app.

## Uninstall

1. Quit TubeAmp.
2. Delete `/Applications/TubeAmp.app`.
3. Optional, to remove your settings, playlist, skins and YouTube Music sign-in:

   ```sh
   rm -rf ~/Library/Application\ Support/app.TubeAmp.TubeAmp
   ```

4. Optional: in **System Settings ▸ Privacy & Security ▸ Screen & System Audio Recording**, remove
   TubeAmp from the list.

## Troubleshooting

- **"TubeAmp is damaged and can't be opened"**: this is Gatekeeper on a quarantined, ad-hoc signed
  copy. Use step 2, Option C.
- **EQ OFF message in the marquee**: the permission is missing or was refused. Allow it (step 3), then
  use **Retry audio capture** from the O menu or turn the EQ ON. The music keeps playing meanwhile.
- **Equalizer and visualizer do nothing, no message**: check the System Audio Recording permission
  (step 3), then restart TubeAmp.
- **Two copies of TubeAmp cannot run together**: they would share one config folder. The second copy
  quits with a message. This is intended.
