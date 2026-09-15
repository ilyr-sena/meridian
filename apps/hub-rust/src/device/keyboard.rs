//! Pure Rust keyboard input over CoreDevice HID.
//!
//! Keys are delivered as `IndigoKeyboardButtonEvent`s (HID Keyboard/Keypad
//! page `0x07`, usage page implicit) on the SAME cached RemoteXPC session
//! hardware buttons use — no WDA round-trip, no session lookup. One one-way
//! XPC event per key state change: single-digit-millisecond latency.
//!
//! The mapping is scancode-based (`KeyboardEvent.code` positions on the
//! reference US layout). The device interprets each usage with ITS OWN
//! configured keyboard layout, so any distribution the phone has (Spanish,
//! German, ...) types exactly what a real USB keyboard would type on it.
//!
//! Characters needing a modifier (uppercase, symbols) are wrapped:
//! modifier down -> key down -> key up -> modifier up. Held modifiers and
//! OS-side key repeat are supported through [`CoreDeviceKeyboard::down`] /
//! [`CoreDeviceKeyboard::up`] (raw state, no auto-release).

use std::time::Duration;

use idevice::services::core_device::ButtonState;
use tracing::{debug, info};

use super::actions::{create_hid_session, hid_cache, HidSession};

/// HID Keyboard/Keypad (`0x07`) usages for `KeyboardEvent.code` positions.
///
/// These are POSITIONS on the reference (US) layout — the device maps them
/// through its own layout at HID-interpretation time.
pub fn usage_for_code(code: &str) -> Option<u64> {
    let u = match code {
        // Letters: KeyA..KeyZ -> 0x04..0x1D
        "KeyA" => 0x04,
        "KeyB" => 0x05,
        "KeyC" => 0x06,
        "KeyD" => 0x07,
        "KeyE" => 0x08,
        "KeyF" => 0x09,
        "KeyG" => 0x0A,
        "KeyH" => 0x0B,
        "KeyI" => 0x0C,
        "KeyJ" => 0x0D,
        "KeyK" => 0x0E,
        "KeyL" => 0x0F,
        "KeyM" => 0x10,
        "KeyN" => 0x11,
        "KeyO" => 0x12,
        "KeyP" => 0x13,
        "KeyQ" => 0x14,
        "KeyR" => 0x15,
        "KeyS" => 0x16,
        "KeyT" => 0x17,
        "KeyU" => 0x18,
        "KeyV" => 0x19,
        "KeyW" => 0x1A,
        "KeyX" => 0x1B,
        "KeyY" => 0x1C,
        "KeyZ" => 0x1D,

        // Digits row: Digit1..Digit9, Digit0 -> 0x1E..0x27
        "Digit1" => 0x1E,
        "Digit2" => 0x1F,
        "Digit3" => 0x20,
        "Digit4" => 0x21,
        "Digit5" => 0x22,
        "Digit6" => 0x23,
        "Digit7" => 0x24,
        "Digit8" => 0x25,
        "Digit9" => 0x26,
        "Digit0" => 0x27,

        // Control / editing keys
        "Enter" | "NumpadEnter" => 0x28,
        "Escape" => 0x29,
        "Backspace" => 0x2A,
        "Tab" => 0x2B,
        "Space" => 0x2C,
        "Minus" => 0x2D,
        "Equal" => 0x2E,
        "BracketLeft" => 0x2F,
        "BracketRight" => 0x30,
        "Backslash" => 0x31,
        "Semicolon" => 0x33,
        "Quote" => 0x34,
        "Backquote" => 0x35,
        "Comma" => 0x36,
        "Period" => 0x37,
        "Slash" => 0x38,
        "CapsLock" => 0x39,

        // Function row
        "F1" => 0x3A,
        "F2" => 0x3B,
        "F3" => 0x3C,
        "F4" => 0x3D,
        "F5" => 0x3E,
        "F6" => 0x3F,
        "F7" => 0x40,
        "F8" => 0x41,
        "F9" => 0x42,
        "F10" => 0x43,
        "F11" => 0x44,
        "F12" => 0x45,

        // Navigation
        "PrintScreen" => 0x46,
        "ScrollLock" => 0x47,
        "Pause" => 0x48,
        "Insert" => 0x49,
        "Home" => 0x4A,
        "PageUp" => 0x4B,
        "Delete" => 0x4C,
        "End" => 0x4D,
        "PageDown" => 0x4E,
        "ArrowRight" => 0x4F,
        "ArrowLeft" => 0x50,
        "ArrowDown" => 0x51,
        "ArrowUp" => 0x52,

        // Numpad
        "NumLock" => 0x53,
        "NumpadDivide" => 0x54,
        "NumpadMultiply" => 0x55,
        "NumpadSubtract" => 0x56,
        "NumpadAdd" => 0x57,
        "Numpad1" => 0x59,
        "Numpad2" => 0x5A,
        "Numpad3" => 0x5B,
        "Numpad4" => 0x5C,
        "Numpad5" => 0x5D,
        "Numpad6" => 0x5E,
        "Numpad7" => 0x5F,
        "Numpad8" => 0x60,
        "Numpad9" => 0x61,
        "Numpad0" => 0x62,
        "NumpadDecimal" => 0x63,
        "NumpadEqual" => 0x67,

        // Modifiers
        "ControlLeft" => 0xE0,
        "ShiftLeft" => 0xE1,
        "AltLeft" => 0xE2,
        "MetaLeft" => 0xE3,
        "ControlRight" => 0xE4,
        "ShiftRight" => 0xE5,
        "AltRight" => 0xE6,
        "MetaRight" => 0xE7,

        // International keys (HID usage-table entries)
        "IntlBackslash" => 0x64, // Non-US \ and |
        "IntlRo" => 0x87,
        "IntlYen" => 0x89,

        _ => return None,
    };
    Some(u)
}

/// Logical modifier name -> keyboard usage.
fn modifier_usage(name: &str) -> Option<u64> {
    let u = match name.to_ascii_lowercase().as_str() {
        "shift" | "shiftleft" => 0xE1,
        "shiftright" => 0xE5,
        "ctrl" | "control" | "controlleft" => 0xE0,
        "controlright" => 0xE4,
        "alt" | "altleft" => 0xE2,
        "altgr" | "altright" => 0xE6,
        "meta" | "cmd" | "metaleft" => 0xE3,
        "metaright" => 0xE7,
        _ => return None,
    };
    Some(u)
}

pub struct CoreDeviceKeyboard;

impl CoreDeviceKeyboard {
    /// Press and release a key by `KeyboardEvent.code` position.
    ///
    /// `modifier` (optional) is pressed down before the key and released
    /// after — for uppercase/symbol characters typed from virtual keys.
    /// For physical typing prefer [`down`]/[`up`] streams.
    pub async fn press(
        udid: String,
        device_id: u32,
        code: &str,
        modifier: Option<&str>,
    ) -> anyhow::Result<()> {
        let usage = usage_for_code(code)
            .ok_or_else(|| anyhow::anyhow!("No HID usage for key code: {code}"))?;
        let modifier = modifier.map(str::to_string);
        with_session(&udid, device_id, move |session| Box::pin(async move {
            if let Some(mod_name) = modifier {
                let m = modifier_usage(&mod_name)
                    .ok_or_else(|| anyhow::anyhow!("Unknown keyboard modifier: {mod_name}"))?;
                session.hid.send_keyboard(m, ButtonState::Down).await?;
                let r = type_key_press(session, usage).await;
                let _ = session.hid.send_keyboard(m, ButtonState::Up).await;
                r?;
                return Ok(());
            }
            type_key_press(session, usage).await
        }))
        .await
    }

    /// Raw key-down by code (no auto-release). Enables held modifiers and
    /// OS-side key repeat; pair with [`CoreDeviceKeyboard::up`].
    pub async fn down(udid: String, device_id: u32, code: &str) -> anyhow::Result<()> {
        let usage = usage_for_code(code)
            .ok_or_else(|| anyhow::anyhow!("No HID usage for key code: {code}"))?;
        with_session(&udid, device_id, move |session| Box::pin(async move {
            session
                .hid
                .send_keyboard(usage, ButtonState::Down)
                .await
                .map_err(anyhow::Error::from)
        }))
        .await
    }

    /// Raw key-up by code, releasing a key held via [`down`].
    pub async fn up(udid: String, device_id: u32, code: &str) -> anyhow::Result<()> {
        let usage = usage_for_code(code)
            .ok_or_else(|| anyhow::anyhow!("No HID usage for key code: {code}"))?;
        with_session(&udid, device_id, move |session| Box::pin(async move {
            session
                .hid
                .send_keyboard(usage, ButtonState::Up)
                .await
                .map_err(anyhow::Error::from)
        }))
        .await
    }

    /// Type a run of characters through the device's layout — each char is
    /// mapped to its scancode position (+ shift when required on the
    /// reference layout). ASCII-only; arbitrary Unicode goes through the WDA
    /// paste path.
    pub async fn type_ascii(udid: String, device_id: u32, text: &str) -> anyhow::Result<()> {
        let text = text.to_string();
        with_session(&udid, device_id, move |session| Box::pin(async move {
            for ch in text.chars() {
                match ch {
                    '\r' => continue,
                    '\n' => type_key_press(session, 0x28).await?,
                    '\t' => type_key_press(session, 0x2B).await?,
                    _ => {
                        let Some((usage, shift)) = char_usage(ch) else {
                            debug!("no scancode for char {ch:?}; skipped");
                            continue;
                        };
                        if shift {
                            session
                                .hid
                                .send_keyboard(0xE1, ButtonState::Down)
                                .await?;
                            let r = type_key_press(session, usage).await;
                            let _ = session.hid.send_keyboard(0xE1, ButtonState::Up).await;
                            r?;
                        } else {
                            type_key_press(session, usage).await?;
                        }
                    }
                }
            }
            Ok(())
        }))
        .await
    }
}

async fn type_key_press(session: &mut HidSession, usage: u64) -> Result<(), anyhow::Error> {
    session.hid.send_keyboard(usage, ButtonState::Down).await?;
    // Short hold so the phone's keyboard pipeline registers the tap.
    tokio::time::sleep(Duration::from_millis(12)).await;
    session.hid.send_keyboard(usage, ButtonState::Up).await?;
    Ok(())
}

/// Reference (US) layout character -> (usage, needs_shift).
fn char_usage(ch: char) -> Option<(u64, bool)> {
    match ch {
        'a'..='z' => Some((0x04 + (ch as u64 - 'a' as u64), false)),
        'A'..='Z' => Some((0x04 + (ch as u64 - 'A' as u64), true)),
        '1'..='9' => Some((0x1E + (ch as u64 - '1' as u64), false)),
        '0' => Some((0x27, false)),
        ' ' => Some((0x2C, false)),
        '-' => Some((0x2D, false)),
        '_' => Some((0x2D, true)),
        '=' => Some((0x2E, false)),
        '+' => Some((0x2E, true)),
        '[' => Some((0x2F, false)),
        '{' => Some((0x2F, true)),
        ']' => Some((0x30, false)),
        '}' => Some((0x30, true)),
        '\\' => Some((0x31, false)),
        '|' => Some((0x31, true)),
        ';' => Some((0x33, false)),
        ':' => Some((0x33, true)),
        '\'' => Some((0x34, false)),
        '"' => Some((0x34, true)),
        '`' => Some((0x35, false)),
        '~' => Some((0x35, true)),
        ',' => Some((0x36, false)),
        '<' => Some((0x36, true)),
        '.' => Some((0x37, false)),
        '>' => Some((0x37, true)),
        '/' => Some((0x38, false)),
        '?' => Some((0x38, true)),
        '!' => Some((0x1E, true)),
        '@' => Some((0x1F, true)),
        '#' => Some((0x20, true)),
        '$' => Some((0x21, true)),
        '%' => Some((0x22, true)),
        '^' => Some((0x23, true)),
        '&' => Some((0x24, true)),
        '*' => Some((0x25, true)),
        '(' => Some((0x26, true)),
        ')' => Some((0x27, true)),
        _ => None,
    }
}

/// Run `f` against the device's cached HID session, recreating it once if it
/// went stale (lock/unlock, USB re-enum). The map lock is held for the
/// duration of `f`, serializing HID events on the device's single connection.
async fn with_session<T, F>(udid: &str, device_id: u32, f: F) -> anyhow::Result<T>
where
    F: for<'a> FnOnce(
        &'a mut HidSession,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<T, anyhow::Error>> + Send + 'a>,
    >,
{
    let mut f = Some(f);
    let cache = hid_cache();
    let mut map = cache.lock().await;

    let session = match map.get_mut(udid) {
        Some(s) => s,
        None => {
            info!("Opening CoreDevice HID keyboard session for {udid}...");
            let s = create_hid_session(udid, device_id).await?;
            map.insert(udid.to_string(), s);
            map.get_mut(udid)
                .ok_or_else(|| anyhow::anyhow!("HID session missing"))?
        }
    };

    match (f.take().expect("session closure"))(session).await {
        Ok(v) => Ok(v),
        Err(e) => {
            debug!("HID session stale for {udid} ({e}); reconnecting");
            let fresh = create_hid_session(udid, device_id).await?;
            map.insert(udid.to_string(), fresh);
            let session = map
                .get_mut(udid)
                .ok_or_else(|| anyhow::anyhow!("HID session missing"))?;
            (f.take().expect("session closure"))(session).await
        }
    }
}
