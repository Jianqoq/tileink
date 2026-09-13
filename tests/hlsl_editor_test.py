"""Run with TILEINK_HLSL_LANGUAGE_SERVER set to ShaderTools.LanguageServer.exe.

DXC accepts loop names that Shader Tools diagnoses under its older scope rules.
Use the actual editor server, and require a diagnostic round trip for each file;
a silent or unresponsive server must never count as a passing syntax check.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time
import unittest


class ShaderEditorTest(unittest.TestCase):
    def test_prefix_scans_have_no_editor_diagnostics(self):
        server = os.environ.get("TILEINK_HLSL_LANGUAGE_SERVER")
        if not server:
            self.skipTest("requires TILEINK_HLSL_LANGUAGE_SERVER")
        root = Path(__file__).resolve().parents[1] / "src/shaders/hlsl"
        messages = queue.Queue()
        process = subprocess.Popen(
            [server], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )

        def read():
            try:
                while True:
                    headers = {}
                    while True:
                        line = process.stdout.readline()
                        if not line:
                            raise EOFError("Shader Tools closed stdout")
                        if line == b"\r\n":
                            break
                        key, value = line.decode().split(":", 1)
                        headers[key.lower()] = value.strip()
                    messages.put(json.loads(process.stdout.read(int(headers["content-length"]))))
            except Exception as error:
                messages.put(error)

        def send(message):
            data = json.dumps({"jsonrpc": "2.0", **message}).encode()
            process.stdin.write(f"Content-Length: {len(data)}\r\n\r\n".encode() + data)
            process.stdin.flush()

        def wait(predicate):
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                message = messages.get(timeout=max(0.01, deadline - time.monotonic()))
                if isinstance(message, Exception):
                    raise message
                if "id" in message and "method" in message:
                    send({"id": message["id"], "result": [] if message["method"] == "workspace/configuration" else None})
                if predicate(message):
                    return message
            self.fail("Shader Tools did not respond")

        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        try:
            send({"id": 1, "method": "initialize", "params": {
                "processId": os.getpid(), "rootUri": root.as_uri(), "capabilities": {}}})
            initialized = wait(lambda message: message.get("id") == 1)
            self.assertNotIn("error", initialized)
            send({"method": "initialized", "params": {}})
            for relative in ["cumsum.hlsl", "coarse/prefix_scan.hlsli"]:
                with self.subTest(shader=relative):
                    path = root / relative
                    uri = path.as_uri()
                    if os.name == "nt":
                        uri = uri.replace(path.drive, path.drive.lower(), 1)
                    source = path.read_text(encoding="utf-8")
                    # Force a nonempty first publication; the corrected document must
                    # subsequently publish diagnostics, including an explicit empty list.
                    send({"method": "textDocument/didOpen", "params": {"textDocument": {
                        "uri": uri, "languageId": "hlsl", "version": 1, "text": source + "\n@\n"}}})
                    def for_document(message):
                        return message.get("method") == "textDocument/publishDiagnostics" and message["params"]["uri"].casefold() == uri.casefold()
                    first = wait(for_document)
                    self.assertTrue(first["params"]["diagnostics"])
                    send({"method": "textDocument/didChange", "params": {
                        "textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": source.count("\n") + 2, "character": 0}}, "text": source}]}})
                    final = wait(for_document)
                    self.assertEqual(final["params"]["diagnostics"], [])
                    send({"method": "textDocument/didClose", "params": {"textDocument": {"uri": uri}}})
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            reader.join(timeout=5)
            process.stdin.close()
            process.stdout.close()


if __name__ == "__main__":
    unittest.main()
