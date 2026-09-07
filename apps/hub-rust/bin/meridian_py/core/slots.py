"""
Dynamic connection-order port slot allocator.

Assigns isolated port blocks to connected iPhones strictly in the order
they connect via USB to the host computer:
  Slot 0: WDA=8100, Bridge=9001, Stream=9200, Tunnel=49151
  Slot 1: WDA=8101, Bridge=9002, Stream=9201, Tunnel=49152
  Slot N: WDA=8100+N, Bridge=9001+N, Stream=9200+N, Tunnel=49151+N
"""
from __future__ import annotations

import logging
import threading
import time
from dataclasses import dataclass
from typing import Dict, List, Optional

log = logging.getLogger(__name__)

BASE_WDA_PORT = 8100
BASE_BRIDGE_PORT = 9001
BASE_STREAM_PORT = 9200
BASE_TUNNEL_PORT = 49151
MAX_SLOTS = 32


@dataclass(frozen=True)
class DevicePorts:
    slot: int
    wda: int
    bridge: int
    stream: int
    tunnel: int
    udid: str
    connected_at: float

    def to_dict(self) -> dict:
        return {
            "wda": self.wda,
            "bridge": self.bridge,
            "stream": self.stream,
        }

    def __str__(self) -> str:
        return f"Slot #{self.slot} [WDA:{self.wda} Bridge:{self.bridge} Stream:{self.stream} Tunnel:{self.tunnel}]"


class SlotManager:
    """Thread-safe allocator managing port slots strictly in connection order."""

    def __init__(self, max_slots: int = MAX_SLOTS):
        self.max_slots = max_slots
        self._lock = threading.Lock()
        # slot_idx -> DevicePorts
        self._slots: Dict[int, DevicePorts] = {}
        # udid -> slot_idx
        self._udid_to_slot: Dict[str, int] = {}
        # Order counter
        self._connection_seq = 0

    def acquire(self, udid: str) -> DevicePorts:
        """
        Allocate the next available slot for a device based on connection order.
        If the device is already assigned a slot, return its existing ports.
        """
        with self._lock:
            if udid in self._udid_to_slot:
                slot = self._udid_to_slot[udid]
                return self._slots[slot]

            # Find the lowest available slot index
            for idx in range(self.max_slots):
                if idx not in self._slots:
                    ports = DevicePorts(
                        slot=idx,
                        wda=BASE_WDA_PORT + idx,
                        bridge=BASE_BRIDGE_PORT + idx,
                        stream=BASE_STREAM_PORT + idx,
                        tunnel=BASE_TUNNEL_PORT + idx,
                        udid=udid,
                        connected_at=time.time(),
                    )
                    self._slots[idx] = ports
                    self._udid_to_slot[udid] = idx
                    self._connection_seq += 1
                    log.info("⚡ Device %s assigned %s (connection #%d)", udid, ports, self._connection_seq)
                    return ports

            raise RuntimeError(f"All {self.max_slots} device slots are exhausted!")

    def release(self, udid: str) -> Optional[DevicePorts]:
        """Release slot assigned to a device upon disconnection."""
        with self._lock:
            if udid not in self._udid_to_slot:
                return None
            slot = self._udid_to_slot.pop(udid)
            ports = self._slots.pop(slot, None)
            if ports:
                log.info("🔌 Device %s released %s", udid, ports)
            return ports

    def get(self, udid: str) -> Optional[DevicePorts]:
        """Get the ports assigned to a UDID if currently active."""
        with self._lock:
            slot = self._udid_to_slot.get(udid)
            if slot is not None:
                return self._slots.get(slot)
            return None

    def get_by_slot(self, slot: int) -> Optional[DevicePorts]:
        """Get the ports assigned to a slot index."""
        with self._lock:
            return self._slots.get(slot)

    def list_active(self) -> List[DevicePorts]:
        """List all active port allocations sorted by slot index."""
        with self._lock:
            return [self._slots[k] for k in sorted(self._slots.keys())]


# Global singleton instance for the Meridian application
GLOBAL_SLOTS = SlotManager()
