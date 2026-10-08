#!/usr/bin/env python3
"""Talk to klipper-ls over stdio, the way an editor would.

    probe.py FILE NEEDLE [NEEDLE ...]

For each NEEDLE, finds its first occurrence in FILE and asks for hover and
go-to-definition one character into it. Handy for checking the server
without restarting an editor.

PROBE_OPTIONS='{"klipperConfig": "..."}' passes initialization options, as
the editor's klipper-ls initialization options would.
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

SERVER = os.environ.get(
    "KLIPPER_LS",
    str(Path(__file__).resolve().parents[1] / "target/debug/klipper-ls"),
)


class Client:
    def __init__(self):
        self.proc = subprocess.Popen(
            [SERVER], stdin=subprocess.PIPE, stdout=subprocess.PIPE
        )
        self.next_id = 0

    def send(self, payload):
        body = json.dumps(payload).encode()
        self.proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.proc.stdin.flush()

    def receive(self):
        length = 0
        while (line := self.proc.stdout.readline().strip()):
            name, _, value = line.partition(b":")
            if name.lower() == b"content-length":
                length = int(value)
        return json.loads(self.proc.stdout.read(length))

    def request(self, method, params):
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        while True:
            message = self.receive()
            if message.get("id") == self.next_id:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message.get("result")

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})


def position_of(text, needle):
    offset = text.index(needle) + 1
    line = text.count("\n", 0, offset)
    character = offset - (text.rfind("\n", 0, offset) + 1)
    return {"line": line, "character": character}


def main():
    path = Path(sys.argv[1]).resolve()
    needles = sys.argv[2:]
    text = path.read_text()
    uri = path.as_uri()

    client = Client()
    client.request(
        "initialize",
        {
            "processId": os.getpid(),
            "rootUri": path.parent.as_uri(),
            "capabilities": {},
            # Same shape as the editor's klipper-ls initialization options.
            "initializationOptions": json.loads(os.environ.get("PROBE_OPTIONS", "{}")),
        },
    )
    client.notify("initialized", {})
    language = "gcode" if path.suffix.lower() in (".gcode", ".gco", ".g") else "klipper"
    client.notify(
        "textDocument/didOpen",
        {"textDocument": {"uri": uri, "languageId": language, "version": 1, "text": text}},
    )
    # Marlin's docs load on a background thread; give it a moment.
    time.sleep(float(os.environ.get("PROBE_WAIT", "0.5")))

    for needle in needles:
        where = {"textDocument": {"uri": uri}, "position": position_of(text, needle)}
        hover = client.request("textDocument/hover", where)
        definition = client.request("textDocument/definition", where)
        print(f"=== {needle!r}")
        print(hover["contents"]["value"] if hover else "(no hover)")
        for location in definition or []:
            target = location["uri"].removeprefix("file://")
            print(f"--> {target}:{location['range']['start']['line'] + 1}")
        print()

    client.request("shutdown", None)
    client.notify("exit", None)


if __name__ == "__main__":
    main()
