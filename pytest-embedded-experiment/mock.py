#!/usr/bin/env python3
"""
    esp-emu --timeout 20s --chip esp32c3 \
        --firmware .esphome/build/mhz19/build/firmware.factory.bin \
        --uart1-tcp 127.0.0.1:5556 --exit-on "CO2:"
    python3 mhz19_mock.py 5556 850   # in a second terminal
"""
import socket
import sys
import time

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 5556
DEADLINE = time.time() + 60


# esp-emu is the TCP server; connect to it as the "sensor"
s = None
for _ in range(30):
    try:
        s = socket.create_connection(("127.0.0.1", PORT), timeout=2)
        break
    except OSError:
        if time.time() > DEADLINE:
            raise SystemExit(f"esp-emu not listening on 127.0.0.1:{PORT}")
        time.sleep(0.5)
if s is None:
    raise SystemExit(f"esp-emu not listening on 127.0.0.1:{PORT}")

print(f"connected to esp-emu UART1 on 127.0.0.1:{PORT}, reporting {PPM} ppm", flush=True)
s.settimeout(1)
buf = b""
try:
    while time.time() < DEADLINE:
        try:
            data = s.recv(64)
        except socket.timeout:
            continue
        if not data:
            break
        buf += data
        while len(buf) >= 9:
            frame, buf = buf[:9], buf[9:]
            if frame[0] != 0xFF:
                continue
            print("request: ", frame.hex(" "), flush=True)
            resp = make_response(frame)
            if resp:
                s.sendall(resp)
                print("response:", resp.hex(" "), flush=True)
except ConnectionError:
    pass
print("closing (emu gone or deadline)", flush=True)
