use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Whether the main playback screen currently owns terminal input.
#[derive(Clone, Default, Debug)]
pub struct InputGate(Arc<AtomicBool>);

impl InputGate {
    pub fn is_playback_active(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn activate_playback(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Hand terminal input to a screen running inside the player task.
    pub fn hand_off_to_screen(&self) {
        self.0.store(false, Ordering::Release);
    }

    pub fn pause(&self) -> InputGuard {
        let previous = self.0.swap(false, Ordering::AcqRel);
        InputGuard {
            gate: self.clone(),
            previous,
        }
    }

    /// The main loop already paused input before sending a screen command.
    pub fn resume_after_screen(&self) -> InputGuard {
        InputGuard {
            gate: self.clone(),
            previous: true,
        }
    }
}

pub struct InputGuard {
    gate: InputGate,
    previous: bool,
}

impl Drop for InputGuard {
    fn drop(&mut self) {
        self.gate.0.store(self.previous, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_pause_restores_previous_state() {
        let gate = InputGate::default();
        gate.activate_playback();
        {
            let _outer = gate.pause();
            assert!(!gate.is_playback_active());
            {
                let _inner = gate.pause();
            }
            assert!(!gate.is_playback_active());
        }
        assert!(gate.is_playback_active());
    }

    #[test]
    fn screen_guard_restores_input_on_error_path() {
        let gate = InputGate::default();
        gate.activate_playback();
        gate.hand_off_to_screen();
        assert!(!gate.is_playback_active());
        {
            let _screen = gate.resume_after_screen();
            assert!(!gate.is_playback_active());
        }
        assert!(gate.is_playback_active());
    }

    #[test]
    fn startup_pause_does_not_activate_playback() {
        let gate = InputGate::default();
        {
            let _selection = gate.pause();
        }
        assert!(!gate.is_playback_active());
    }
}
