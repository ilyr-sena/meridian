"""
Device Inspector and Onboarding State Machine for Meridian.

Evaluates deterministic state gates for every connected iPhone:
  1. USB Detection & Connection
  2. Pairing & Trust Verification ("Trust This Computer")
  3. Passcode & Lockscreen State
  4. iOS Version Check (iOS 27.x strictly required)
  5. Developer Mode Check & AMFI Programmatic Enablement/Reboot
  6. MeridianRunner App Presence & Sideload Verification
  7. Ready for Hub Sync & Streaming
"""
from __future__ import annotations

import asyncio
import enum
import logging
from dataclasses import dataclass
from typing import Optional

from pymobiledevice3.exceptions import (
    DeviceHasPasscodeSetError,
    NoDeviceConnectedError,
    PasswordRequiredError,
    PyMobileDevice3Exception,
)
from pymobiledevice3.lockdown import LockdownClient, create_using_usbmux
from pymobiledevice3.remote.core_device.app_service import AppServiceService
from pymobiledevice3.services.amfi import AmfiService

log = logging.getLogger(__name__)


class DeviceState(enum.Enum):
    DISCONNECTED = "disconnected"
    UNPAIRED = "unpaired"                # Needs "Trust This Computer"
    LOCKED = "locked"                    # Passcode / Face ID lockscreen active
    UNSUPPORTED_IOS = "unsupported_ios"  # Not running iOS 27
    DEVMODE_HIDDEN = "devmode_hidden"    # Developer mode menu hidden, needs AMFI reveal
    DEVMODE_DISABLED = "devmode_disabled"  # Developer mode toggle visible but OFF
    DEVMODE_PENDING_REBOOT = "devmode_reboot"  # Rebooting to apply dev mode
    NO_RUNNER_APP = "no_runner"          # MeridianRunner.app needs to be sideloaded
    READY = "ready"                      # Meets all criteria, ready to launch
    RUNNING = "running"                  # Streaming and tunnels active


@dataclass
class DeviceReport:
    udid: str
    name: str = "iPhone"
    model: str = "iPhone"
    product_type: str = "iPhone"
    os_version: str = "Unknown"
    is_ios_27: bool = False
    is_paired: bool = False
    is_locked: bool = False
    dev_mode_enabled: bool = False
    dev_mode_revealed: bool = False
    has_runner_app: bool = False
    runner_bundle_id: Optional[str] = None
    state: DeviceState = DeviceState.DISCONNECTED
    status_message: str = "Disconnected"
    user_action_step: str = ""

    def is_operational(self) -> bool:
        return self.state in (DeviceState.READY, DeviceState.RUNNING)


class DeviceInspector:
    """Inspects iPhones and executes onboarding gates."""

    @staticmethod
    async def inspect(udid: str, is_running: bool = False) -> DeviceReport:
        report = DeviceReport(udid=udid)

        # 1. Connect via Lockdown to inspect trust, passcode, and basic metadata
        try:
            lockdown: LockdownClient = await create_using_usbmux(serial=udid)
        except (NoDeviceConnectedError, ConnectionError, OSError):
            report.state = DeviceState.DISCONNECTED
            report.status_message = "Device not detected over USB"
            report.user_action_step = "Connect your iPhone via a verified USB cable."
            return report
        except PasswordRequiredError:
            report.state = DeviceState.UNPAIRED
            report.status_message = "Trust pairing required"
            report.user_action_step = "Unlock your iPhone and tap 'Trust This Computer'."
            return report
        except Exception as e:
            report.state = DeviceState.UNPAIRED
            report.status_message = f"Pairing error: {e}"
            report.user_action_step = "Unlock device and accept trust dialog."
            return report

        # Extract basic device info
        report.name = lockdown.short_info.get("DeviceName") or "iPhone"
        report.product_type = lockdown.short_info.get("ProductType") or "iPhone"
        report.os_version = str(lockdown.short_info.get("ProductVersion") or "")
        report.model = lockdown.short_info.get("DeviceClass") or "iPhone"
        report.is_paired = True
        report.is_ios_27 = report.os_version.startswith("27.") or report.os_version == "27"

        # 2. Check Passcode / Lockscreen state
        try:
            is_pwd_protected = await lockdown.get_value(key="PasswordProtected")
            report.is_locked = bool(is_pwd_protected)
            if report.is_locked:
                report.state = DeviceState.LOCKED
                report.status_message = "Device is locked with passcode"
                report.user_action_step = "Unlock your iPhone with Passcode or Face ID to allow communication."
                return report
        except Exception:
            pass

        # 3. iOS 27 Version Gate
        # Only iOS 27 is supported.
        if not report.is_ios_27:
            report.state = DeviceState.UNSUPPORTED_IOS
            report.status_message = f"Unsupported iOS version: {report.os_version}"
            report.user_action_step = f"Meridian requires iOS 27.x. Device is running iOS {report.os_version}."
            return report

        # 4. Developer Mode Gate
        dev_mode_on = False
        try:
            val = await lockdown.get_value(domain="com.apple.security.mac.amfi", key="DeveloperModeStatus")
            dev_mode_on = bool(val)
        except Exception:
            try:
                from ..remote.bridge import _rsd_for_device
                rsd = await _rsd_for_device(udid)
                dev_mode_on = await rsd.get_developer_mode_status()
            except Exception as e:
                log.debug("could not query developer mode via RSD: %s", e)

        report.dev_mode_enabled = bool(dev_mode_on)

        if not report.dev_mode_enabled:
            # Check AMFI service to reveal toggle if missing
            try:
                amfi = AmfiService(lockdown)
                await amfi.reveal_developer_mode_option_in_ui()
                report.dev_mode_revealed = True
                report.state = DeviceState.DEVMODE_DISABLED
                report.status_message = "Developer Mode is disabled"
                report.user_action_step = (
                    "Open Settings → Privacy & Security → Developer Mode on your iPhone and toggle it ON. "
                    "The device will prompt to restart."
                )
            except Exception as e:
                report.state = DeviceState.DEVMODE_HIDDEN
                report.status_message = f"Developer Mode inactive ({e})"
                report.user_action_step = "Click 'Enable Developer Mode' in Meridian to trigger activation."
            return report

        # 5. MeridianRunner App Presence Gate
        try:
            from pymobiledevice3.services.installation_proxy import InstallationProxyService
            inst = InstallationProxyService(lockdown)
            apps = await inst.get_apps()
            for bid in apps.keys():
                bid_str = str(bid)
                if bid_str.startswith("dev.ius.meridian.runner") or "meridian.runner" in bid_str.lower():
                    report.has_runner_app = True
                    report.runner_bundle_id = bid_str
                    break
        except Exception as e:
            log.debug("could not query installed apps via InstallationProxy: %s", e)

        if not report.has_runner_app:
            try:
                from ..remote.bridge import _rsd_for_device
                rsd = await _rsd_for_device(udid)
                async with AppServiceService(rsd) as app_service:
                    apps = await app_service.list_apps()
                    for a in apps:
                        bid = str(a.get("bundleIdentifier") or "")
                        if bid.startswith("dev.ius.meridian.runner") or "meridian.runner" in bid.lower():
                            report.has_runner_app = True
                            report.runner_bundle_id = bid
                            break
            except Exception as e:
                log.debug("could not query installed apps via CoreDevice: %s", e)

        if not report.has_runner_app:
            report.state = DeviceState.NO_RUNNER_APP
            report.status_message = "MeridianRunner not installed"
            report.user_action_step = (
                "Click 'Sideload MeridianRunner' to automatically sign and sideload the runner app "
                "using your Apple ID."
            )
            return report

        # All criteria met!
        if is_running:
            report.state = DeviceState.RUNNING
            report.status_message = "Running & Streaming"
            report.user_action_step = "Device is active and streaming."
        else:
            report.state = DeviceState.READY
            report.status_message = "Ready for Meridian Hub"
            report.user_action_step = "Click 'Start' to begin remote control & streaming."

        return report

    @staticmethod
    async def reveal_developer_mode(udid: str) -> bool:
        """Programmatically reveal the Developer Mode menu in iOS Settings."""
        try:
            lockdown = await create_using_usbmux(serial=udid)
            amfi = AmfiService(lockdown)
            await amfi.reveal_developer_mode_option_in_ui()
            log.info("✓ AMFI revealed Developer Mode option in device UI for %s", udid)
            return True
        except Exception as e:
            log.warning("failed to reveal developer mode for %s: %s", udid, e)
            return False

    @staticmethod
    async def trigger_developer_mode_enable(udid: str) -> bool:
        """Trigger developer mode activation and reboot on the device."""
        try:
            lockdown = await create_using_usbmux(serial=udid)
            amfi = AmfiService(lockdown)
            await amfi.enable_developer_mode(enable_post_restart=False)
            log.info("✓ AMFI scheduled Developer Mode enable and reboot for %s", udid)
            return True
        except DeviceHasPasscodeSetError:
            log.warning("cannot enable developer mode while passcode is set without user toggle")
            return False
        except Exception as e:
            log.warning("failed to trigger developer mode enable: %s", e)
            return False

    @staticmethod
    async def confirm_developer_mode_post_restart(udid: str) -> bool:
        """Confirm the developer mode prompt after device reboot."""
        try:
            lockdown = await create_using_usbmux(serial=udid)
            amfi = AmfiService(lockdown)
            await amfi.enable_developer_mode_post_restart()
            log.info("✓ AMFI confirmed Developer Mode post-restart for %s", udid)
            return True
        except Exception as e:
            log.warning("failed to confirm developer mode post-restart: %s", e)
            return False
