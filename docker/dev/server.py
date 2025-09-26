import json
import os
import ssl
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlparse


class SceneServiceHandler(BaseHTTPRequestHandler):
    server_version = "LokanMockScene/0.1"

    def log_message(self, format, *args):  # noqa: A003 - matching BaseHTTPRequestHandler signature
        # Reduce noise in CI runs.
        return

    def do_GET(self):  # noqa: N802 - inherited API
        parsed = urlparse(self.path)
        if parsed.path == "/scene-svc/health":
            body = json.dumps({"status": "ok"}).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_error(404, "Not Found")

    def do_POST(self):  # noqa: N802 - inherited API
        parsed = urlparse(self.path)
        if parsed.path == "/scene-svc/scenes/apply":
            length = int(self.headers.get("Content-Length", "0"))
            if length:
                _ = self.rfile.read(length)
            body = json.dumps({"status": "accepted"}).encode("utf-8")
            self.send_response(202)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_error(404, "Not Found")


def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"Missing required environment variable: {name}")
    return value


def run() -> None:
    bind = os.environ.get("LOKAN_BIND", "0.0.0.0:9443")
    host, _, port_str = bind.partition(":")
    if not host or not port_str:
        raise RuntimeError("LOKAN_BIND must be in host:port format")
    port = int(port_str)

    server_cert = require_env("LOKAN_SERVER_CERT")
    server_key = require_env("LOKAN_SERVER_KEY")
    ca_cert = require_env("LOKAN_CA_CERT")

    context = ssl.create_default_context(ssl.Purpose.CLIENT_AUTH)
    context.load_cert_chain(certfile=server_cert, keyfile=server_key)
    context.load_verify_locations(cafile=ca_cert)
    context.verify_mode = ssl.CERT_REQUIRED

    httpd = HTTPServer((host, port), SceneServiceHandler)
    httpd.socket = context.wrap_socket(httpd.socket, server_side=True)

    print(f"Mock scene service listening on https://{bind}")
    httpd.serve_forever()


if __name__ == "__main__":
    run()
