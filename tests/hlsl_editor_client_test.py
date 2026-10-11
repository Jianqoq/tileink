"""Exercise the real editor test's message loop without a shader language server."""
import queue
import unittest

from hlsl_editor_test import wait_for


class EditorClientProtocolTest(unittest.TestCase):
    def exchange(self, *messages):
        incoming = queue.Queue()
        for message in messages:
            incoming.put(message)
        sent = []
        reply = wait_for(incoming, sent.append, lambda m: m.get("id") == 1, timeout=1)
        return reply, sent

    def test_unknown_requests_report_method_not_found_and_preserve_ids(self):
        reply = {"id": 1, "result": {"capabilities": {}}}
        for request_id in [0, 17, "17", None]:
            with self.subTest(request_id=request_id):
                actual, sent = self.exchange(
                    {"id": request_id, "method": "unsupported/method"}, reply
                )
                self.assertEqual(actual, reply)
                self.assertEqual(sent, [{
                    "id": request_id,
                    "error": {"code": -32601, "message": "Method not found"},
                }])

    def test_unadvertised_configuration_support_is_not_fabricated(self):
        _, sent = self.exchange(
            {"id": "config", "method": "workspace/configuration", "params": {
                "items": [{"section": "hlsl"}, {"section": "editor"}]}},
            {"id": 1, "result": {"capabilities": {}}},
        )
        self.assertNotIn("result", sent[0])
        self.assertEqual(sent[0]["error"]["code"], -32601)

    def test_server_request_id_cannot_complete_a_client_request(self):
        reply = {"id": 1, "result": {"capabilities": {}}}
        actual, sent = self.exchange({"id": 1, "method": "unsupported/method"}, reply)
        self.assertIs(actual, reply)
        self.assertEqual(len(sent), 1)
        self.assertEqual(sent[0]["error"]["code"], -32601)

    def test_notifications_are_not_answered(self):
        reply = {"id": 1, "result": {}}
        actual, sent = self.exchange(
            {"method": "window/logMessage", "params": {"message": "ready", "type": 3}},
            reply,
        )
        self.assertIs(actual, reply)
        self.assertEqual(sent, [])

    def test_protocol_error_responses_are_preserved(self):
        error = {"id": 1, "error": {"code": -32603, "message": "server failure"}}
        actual, sent = self.exchange(error)
        self.assertIs(actual, error)
        self.assertNotIn("result", actual)
        self.assertEqual(sent, [])

    def test_expected_diagnostic_notifications_complete_the_wait(self):
        incoming = queue.Queue()
        incoming.put({"id": 1, "method": "unsupported/method"})
        diagnostic = {"method": "textDocument/publishDiagnostics", "params": {
            "uri": "file:///shader.hlsl", "diagnostics": []}}
        incoming.put(diagnostic)
        sent = []
        actual = wait_for(incoming, sent.append, lambda m:
                          m.get("method") == "textDocument/publishDiagnostics")
        self.assertIs(actual, diagnostic)
        self.assertEqual(sent[0]["error"]["code"], -32601)

    def test_reply_write_failures_propagate(self):
        incoming = queue.Queue()
        incoming.put({"id": 7, "method": "unsupported/method"})
        def failed_send(_):
            raise BrokenPipeError("language server closed stdin")
        with self.assertRaises(BrokenPipeError):
            wait_for(incoming, failed_send, self.fail)

    def test_reader_failures_propagate_without_consuming_a_later_success(self):
        incoming = queue.Queue()
        error = EOFError("language server closed stdout")
        incoming.put(error)
        reply = {"id": 1, "result": {}}
        incoming.put(reply)
        sent = []
        with self.assertRaises(EOFError) as raised:
            wait_for(incoming, sent.append, lambda m: m.get("id") == 1, timeout=1)
        self.assertIs(raised.exception, error)
        self.assertIs(incoming.get_nowait(), reply)
        self.assertEqual(sent, [])

    def test_absent_reply_reports_a_timeout(self):
        for timeout in [0, 0.001]:
            with self.subTest(timeout=timeout), self.assertRaisesRegex(
                TimeoutError, "Shader Tools did not respond"
            ):
                wait_for(queue.Queue(), self.fail, lambda _: True, timeout=timeout)


if __name__ == "__main__":
    unittest.main()
