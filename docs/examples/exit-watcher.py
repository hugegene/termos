#!/usr/bin/env python3
"""An example tos extension: warns in the status bar when a pane's command fails.

It reads `tos subscribe` events, and when a pane closes with a non-zero exit status it shows
"✗ <title> exited <status>" in the status bar for 30 seconds. Clicking the warning clears it and goes back
to the tab where it happened.

Run it inside a tos session (where TOS_SESSION is set), or name the session:

    python3 exit-watcher.py            # the session it runs in
    python3 exit-watcher.py work       # session "work"

Set TOS to the tos program if it isn't on your PATH.
"""

import json
import os
import subprocess
import sys

TOS = os.environ.get("TOS", "tos")
SESSION = ["-s", sys.argv[1]] if len(sys.argv) > 1 else []


def action(*args):
    subprocess.run([TOS, *SESSION, "action", *args], check=False, capture_output=True)


def main():
    titles = {}  # pane id -> its title, to name a pane after it has closed
    failed_in = None  # the tab the last failure happened in
    sub = subprocess.Popen([TOS, *SESSION, "subscribe", "--format", "json"], stdout=subprocess.PIPE, text=True)
    for line in sub.stdout:
        event = json.loads(line)
        kind = event["event"]
        if kind in ("pane_opened", "pane_retitled"):
            titles[event["pane_id"]] = event["title"]
        elif kind == "pane_closed" and event.get("exit_status") not in (None, 0):
            name = titles.pop(event["pane_id"], f"pane {event['pane_id']}")
            failed_in = event["tab_id"]
            action("set-status", "--id", "exit-watcher", "--ttl", "30", f"✗ {name} exited {event['exit_status']}")
        elif kind == "status_clicked" and event["id"] == "exit-watcher":
            action("clear-status", "--id", "exit-watcher")
            if failed_in is not None:
                action("go-to-tab-by-id", str(failed_in))


if __name__ == "__main__":
    main()
