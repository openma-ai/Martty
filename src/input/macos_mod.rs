//! Native macOS modifier detection via CoreGraphics (grok-build's
//! side-channel around the PTY).
//!
//! Terminals translate or swallow ⌘/⌥ chords before they reach a TUI;
//! `CGEventSourceFlagsState` reports the *physical* keyboard state
//! directly, without any special permissions, so a bare arrow that
//! arrives while ⌘ is held can be upgraded to `SUPER+arrow`.

// CGEventSourceFlagsState — stable public API since macOS 10.4.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
}

const HID_SYSTEM_STATE: i32 = 1;

// CGEventFlags bitmasks from <CoreGraphics/CGEventTypes.h>.
const MASK_ALTERNATE: u64 = 0x0008_0000; // Option
const MASK_COMMAND: u64 = 0x0010_0000;

/// One CG syscall, the two modifier bits we act on decoded.
pub fn snapshot() -> super::ModifierState {
    // SAFETY: integer in, integer out; no pointers cross the boundary.
    let f = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
    super::ModifierState {
        command: f & MASK_COMMAND != 0,
        option: f & MASK_ALTERNATE != 0,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_test_modifier_detection() {
        // Just proves the FFI link + call don't crash.
        let s = super::snapshot();
        let _ = (s.command, s.option);
    }
}
