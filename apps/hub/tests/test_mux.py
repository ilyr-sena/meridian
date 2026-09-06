import plistlib
import socket
import struct
import sys
import threading

from meridian_py.mux import MuxClient
from meridian_py.models import Device


def test_list_devices_against_fake_mux(tmp_path):
    if sys.platform == "win32":
        # On Windows, use TCP loopback since AF_UNIX is unavailable
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind(("127.0.0.1", 0))
        port = srv.getsockname()[1]
        srv.listen(1)

        def serve():
            conn, _ = srv.accept()
            conn.recv(4096)
            body = plistlib.dumps({
                "DeviceList": [{
                    "Properties": {
                        "SerialNumber": "00008110-000C694914F3801E",
                        "DeviceID": 1,
                        "ProductID": 0x12A8,
                        "ConnectionType": "USB",
                    }
                }]
            }, fmt=plistlib.FMT_XML)
            conn.sendall(struct.pack("<IIII", len(body) + 16, 1, 8, 7) + body)
            conn.close()
            srv.close()

        t = threading.Thread(target=serve, daemon=True)
        t.start()
        devices = MuxClient(f"127.0.0.1:{port}").list_devices()
        assert len(devices) == 1
        assert devices[0].udid == "00008110-000C694914F3801E"
        t.join(timeout=2)
    else:
        sock_path = str(tmp_path / "mux.sock")
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.bind(sock_path)
        srv.listen(1)

        def serve():
            conn, _ = srv.accept()
            conn.recv(4096)
            body = plistlib.dumps({
                "DeviceList": [{
                    "Properties": {
                        "SerialNumber": "00008110-000C694914F3801E",
                        "DeviceID": 1,
                        "ProductID": 0x12A8,
                        "ConnectionType": "USB",
                    }
                }]
            }, fmt=plistlib.FMT_XML)
            conn.sendall(struct.pack("<IIII", len(body) + 16, 1, 8, 7) + body)
            conn.close()
            srv.close()

        t = threading.Thread(target=serve, daemon=True)
        t.start()
        devices = MuxClient(sock_path).list_devices()
        assert len(devices) == 1
        assert devices[0].udid == "00008110-000C694914F3801E"
        t.join(timeout=2)
