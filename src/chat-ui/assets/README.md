# Application icon

`bonsai.svg` uses the same two-leaf paths as the native sidebar mark in
`../src/window/branding.rs`, with green ink on a transparent background.
Keep their geometry together when changing the logo.

Run the following from the repository root on Windows to regenerate the committed
assets. The script uses Windows System.Drawing; normal builds need no image tools.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/Update-ApplicationIcon.ps1
```

`bonsai.ico` contains 16, 24, 32, 48, 64, 128 and 256 pixel PNG frames for the
Windows executable resources. `bonsai.png` supplies the same 256 pixel frame to
the native window, including its taskbar and Alt+Tab icon.
