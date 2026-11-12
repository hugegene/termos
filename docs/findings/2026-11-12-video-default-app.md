# Finding: is there a default app for videos, and how fast can tos ask? (2026-11-12)

## Question

You asked that `o` on a video (an mp4, say) open it in the default app, or else in mpv, or else ffplay. To fall back,
tos has to know whether a default app is set before it runs `xdg-open`, which otherwise picks something itself (a
browser, say) or fails after tos has let it go. The [grid copy and open in app plan](../plans/2026-11-11-grid-copy-and-open-in-app.md)
needs to know:

1. what `xdg-mime` answers for a video, with and without a default app
2. whether asking is fast enough to do while handling the key, on the server's thread

## Method

Dev machine: Ubuntu, Linux 6.8.0-149, xdg-utils 1.1.3 (`xdg-mime --version`).

```
f=crates/finder-core/tests/data/two-seconds.mp4
for t in video/mp4 video/x-matroska video/webm video/quicktime; do
  printf '%s -> %s\n' "$t" "$(xdg-mime query default $t)"
done
/usr/bin/time -f "filetype: %e s" xdg-mime query filetype $f
/usr/bin/time -f "default: %e s" xdg-mime query default video/mp4
which mpv ffplay vlc totem
grep -l "video/mp4" /usr/share/applications/*.desktop ~/.local/share/applications/*.desktop
```

## Results

```
video/mp4 -> org.gnome.Totem.desktop
video/x-matroska -> org.gnome.Totem.desktop
video/webm -> org.gnome.Totem.desktop
video/quicktime -> org.gnome.Totem.desktop
video/mp4
filetype: 0.01 s
org.gnome.Totem.desktop
default: 0.02 s
/usr/bin/mpv
/usr/bin/ffplay
/usr/bin/totem
/usr/share/applications/mpv.desktop
/usr/share/applications/org.gnome.Totem.desktop
```

(`which vlc` found nothing.)

## Conclusions

1. **On this machine,** `o` on an mp4 opens GNOME Videos (Totem), the default for every video type checked. mpv and
   ffplay are installed too.
2. **Asking takes 10–20 ms per query,** and two are needed: the file's type, then its default. That's fast enough to
   do on the server's thread when `o` is pressed.
3. **What "no default app" looks like** (from xdg-utils' behaviour, not tried here): `xdg-mime query default` prints
   nothing. Without xdg-utils, there's no `xdg-mime` at all. Either way, tos falls back to mpv, then ffplay.

**Still unknown:** macOS isn't covered. Its `open` always has an app for a video (QuickTime Player), so there tos
just runs `open`.
