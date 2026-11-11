# Finding: what the finder's captions show, and what `l preview` does, on an HTML file (2026-11-11)

## Question

You asked whether, with an HTML file selected, a caption mentioning "preview" does nothing. That decides whether the
[grid copy and open in app plan](../plans/2026-11-11-grid-copy-and-open-in-app.md) has a bug to fix in the key
handling, or only a misleading caption.

## Method

Dev machine: Ubuntu, Linux 6.8.0-149, tmux 3.4, `tos` built from the working tree at `ce60027` plus uncommitted
changes (`cargo build --offline`, debug). A throwaway session with its own socket and state folders, driven by tmux
and read back with `capture-pane`:

```
mkdir -p /tmp/tq/s /tmp/tq/st /tmp/tq/demo
cp crates/finder-core/tests/data/office.html /tmp/tq/demo/
printf '# Notes\n\nhello\n' > /tmp/tq/demo/notes.md
export TOS_SOCKET_DIR=/tmp/tq/s TOS_STATE_DIR=/tmp/tq/st
tmux -L tq new-session -d -x 130 -y 24 -e TOS_SOCKET_DIR=/tmp/tq/s -e TOS_STATE_DIR=/tmp/tq/st \
     "cd /tmp/tq/demo && target/debug/tos"
target/debug/tos finder /tmp/tq/demo
tmux -L tq send-keys -t 0 j                  # select office.html
tmux -L tq capture-pane -p | tail -1         # the status bar, at 130 columns
tmux -L tq resize-window -t 0 -x 200 -y 24
tmux -L tq send-keys -t 0 h; tmux -L tq capture-pane -p | tail -1     # list focused, 200 columns
tmux -L tq send-keys -t 0 l; tmux -L tq capture-pane -p | tail -1     # after l
tmux -L tq capture-pane -p -e | sed -n 2p | cat -v                   # frame colours, preview focused
tmux -L tq send-keys -t 0 h; tmux -L tq capture-pane -p -e | sed -n 2p | cat -v   # list focused
tmux -L tq send-keys -t 0 l Enter; tmux -L tq capture-pane -p | sed -n 2,8p      # Enter in the preview
tos kill main; tmux -L tq kill-server; rm -rf /tmp/tq
```

## Results

**The status bar at 130 columns, list focused** (the left side is tos's own keys; the finder's caption is on the
right):

```
 Ctrl +  <g> LOCK  <p> PANE  <t> TAB  <n> RESIZE  <h> MOVE  <s> SCROLL  <o> SESSION  <q> QUIT           j/k move · Enter open/edit
```

**At 200 columns, list focused:**

```
 … <q> QUIT                               j/k move · Enter open/edit · l preview · h up · / filter · . hidden · m menu
```

**After `l`:**

```
 … <q> QUIT                                                  preview · j/k scroll · Space/b page · Enter edit · h back
```

**Frame colours** (start of row 2: the list's frame). With the preview focused, the list's frame is plain
(`^[[39m`). With the list focused, it's green and its title bold (`^[[38;5;2m … ^[[1m /tmp/tq/demo`). The preview's
content is the same either way.

**Enter in the preview:** the pane's title becomes `✎ office.html`, and it shows the HTML source in Neovim:

```
┌ ✎ office.htm…
│  1 <!doctype html>
│  2 <html>
│  3 <head><meta charset="utf-8"><title>Office fixture</title></head>
```

The caption becomes `editing · Alt+s save · Alt+q close`.

## Conclusions

1. **Nothing is broken for HTML.**
   - **`l` moves keyboard focus** to the preview pane: its frame turns green, the caption changes, and `↓↑` then
     scroll the preview instead of moving through files.
   - **Enter in the preview** edits the HTML in Neovim.
2. **But `l preview` reads as "show the preview",** which is already on screen. Pressing it changes only a frame
   colour and the caption, so it looks like nothing happened. The preview caption also starts with the word
   `preview`, a label that reads like a key.
3. **Captions are cut from the right.** At 130 columns only `j/k move · Enter open/edit` shows. So the most useful
   keys need to come first.
