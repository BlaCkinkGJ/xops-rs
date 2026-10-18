//! The state machine the whole game runs on, from the upstream `statemachine.cpp` and
//! `statemachine.h`.
//!
//! Upstream this is a file scope `StateMachine GameState` in `main.cpp` that the frame
//! loop reaches for. Here it is a value the owner passes, the same way the debug log is.
//!
//! Every screen has three states: create (prepare), now (run), destroy (dispose). One
//! call to [`StateMachine::next_state`] moves one step, and the three inputs (a mouse
//! click, BackSpace, F12) each set their flag and step once. The flags are what the
//! destroy states read: F12 restarts the screen that is being torn down, BackSpace leaves
//! it. A click always means "carry on".

use crate::core::ENABLE_DEBUGLOG;
use crate::debug::{DebugLog, LOG_CHECK};

/// Constants representing states, from the upstream `enum State`.
///
/// The discriminants are the upstream values, because the frame loop and the scene code
/// compare against them.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum State {
	/// \[error\] invalid state.
	STATE_NULL = 0,
	/// Opening screen, initialization / preparation.
	STATE_CREATE_OPENING = 1,
	/// Opening screen, running.
	STATE_NOW_OPENING = 2,
	/// Opening screen, termination / disposal.
	STATE_DESTROY_OPENING = 3,
	/// Menu screen, initialization / preparation.
	STATE_CREATE_MENU = 4,
	/// Menu screen, running.
	STATE_NOW_MENU = 5,
	/// Menu screen, termination / disposal.
	STATE_DESTROY_MENU = 6,
	/// Briefing screen, initialization / preparation.
	STATE_CREATE_BRIEFING = 7,
	/// Briefing screen, running.
	STATE_NOW_BRIEFING = 8,
	/// Briefing screen, termination / disposal.
	STATE_DESTROY_BRIEFING = 9,
	/// Main screen, initialization / preparation.
	STATE_CREATE_MAINGAME = 10,
	/// Main screen, running.
	STATE_NOW_MAINGAME = 11,
	/// Main screen, termination / disposal.
	STATE_DESTROY_MAINGAME = 12,
	/// Result display screen, initialization / preparation.
	STATE_CREATE_RESULT = 13,
	/// Result display screen, running.
	STATE_NOW_RESULT = 14,
	/// Result display screen, termination / disposal.
	STATE_DESTROY_RESULT = 15,
	/// Game exit.
	STATE_EXIT = 16,
}

/// `StateMachine`: manages the screen transitions of the entire game.
pub struct StateMachine {
	/// State transition mode.
	state_mode: i32,
	/// Current state.
	now_state: State,
	/// Whether the ESC key was pressed.
	back: bool,
	/// Whether the F12 key was pressed.
	f12: bool,
}

impl StateMachine {
	/// Constructor. The game starts on the opening screen, in the normal mode.
	pub fn new() -> Self {
		StateMachine {
			state_mode: 0,
			now_state: State::STATE_CREATE_OPENING,
			back: false,
			f12: false,
		}
	}

	/// `SetStateMode`: sets the state transition mode.
	///
	/// * `mode` normal: 0, briefing / main screen / result display only: 1, main screen
	///   only: 2.
	///
	/// Anything else falls back to mode 0, which is what the upstream `else` does. Upstream
	/// warns this must be called before the main loop starts, because the state jumps
	/// straight to the entry screen of the mode.
	pub fn set_state_mode(&mut self, mode: i32) {
		if mode == 1 {
			self.state_mode = mode;
			self.now_state = State::STATE_CREATE_BRIEFING;
		} else if mode == 2 {
			self.state_mode = mode;
			self.now_state = State::STATE_CREATE_MAINGAME;
		} else {
			self.state_mode = 0;
			self.now_state = State::STATE_CREATE_OPENING;
		}
	}

	/// `NextState`: transitions to the next state.
	///
	/// The three modes have three separate tables rather than one table with the
	/// differences factored out, because that is the upstream shape and the differences
	/// (where BackSpace leads, whether the menu exists) are the whole behaviour.
	pub fn next_state(&mut self, log: &mut DebugLog) {
		if self.state_mode == 0 {
			// normal
			self.now_state = match self.now_state {
				State::STATE_CREATE_OPENING => State::STATE_NOW_OPENING,
				State::STATE_NOW_OPENING => State::STATE_DESTROY_OPENING,
				State::STATE_DESTROY_OPENING => {
					if self.f12 {
						State::STATE_CREATE_OPENING
					} else {
						State::STATE_CREATE_MENU
					}
				}
				State::STATE_CREATE_MENU => State::STATE_NOW_MENU,
				State::STATE_NOW_MENU => State::STATE_DESTROY_MENU,
				State::STATE_DESTROY_MENU => {
					if self.f12 {
						State::STATE_CREATE_MENU
					} else if self.back {
						State::STATE_EXIT
					} else {
						State::STATE_CREATE_BRIEFING
					}
				}
				State::STATE_CREATE_BRIEFING => State::STATE_NOW_BRIEFING,
				State::STATE_NOW_BRIEFING => State::STATE_DESTROY_BRIEFING,
				State::STATE_DESTROY_BRIEFING => {
					if self.f12 {
						State::STATE_CREATE_BRIEFING
					} else if self.back {
						State::STATE_CREATE_MENU
					} else {
						State::STATE_CREATE_MAINGAME
					}
				}
				State::STATE_CREATE_MAINGAME => State::STATE_NOW_MAINGAME,
				State::STATE_NOW_MAINGAME => State::STATE_DESTROY_MAINGAME,
				State::STATE_DESTROY_MAINGAME => {
					if self.f12 {
						State::STATE_CREATE_MAINGAME
					} else if self.back {
						State::STATE_CREATE_MENU
					} else {
						State::STATE_CREATE_RESULT
					}
				}
				State::STATE_CREATE_RESULT => State::STATE_NOW_RESULT,
				State::STATE_NOW_RESULT => State::STATE_DESTROY_RESULT,
				State::STATE_DESTROY_RESULT => {
					if self.f12 {
						State::STATE_CREATE_RESULT
					} else {
						State::STATE_CREATE_MENU
					}
				}
				State::STATE_EXIT => State::STATE_EXIT,
				State::STATE_NULL => State::STATE_NULL,
			};
		} else if self.state_mode == 1 {
			// briefing / main screen / result display only
			self.now_state = match self.now_state {
				State::STATE_CREATE_BRIEFING => State::STATE_NOW_BRIEFING,
				State::STATE_NOW_BRIEFING => State::STATE_DESTROY_BRIEFING,
				State::STATE_DESTROY_BRIEFING => {
					if self.f12 {
						State::STATE_CREATE_BRIEFING
					} else if self.back {
						State::STATE_EXIT
					} else {
						State::STATE_CREATE_MAINGAME
					}
				}
				State::STATE_CREATE_MAINGAME => State::STATE_NOW_MAINGAME,
				State::STATE_NOW_MAINGAME => State::STATE_DESTROY_MAINGAME,
				State::STATE_DESTROY_MAINGAME => {
					if self.f12 {
						State::STATE_CREATE_MAINGAME
					} else if self.back {
						State::STATE_CREATE_BRIEFING
					} else {
						State::STATE_CREATE_RESULT
					}
				}
				State::STATE_CREATE_RESULT => State::STATE_NOW_RESULT,
				State::STATE_NOW_RESULT => State::STATE_DESTROY_RESULT,
				State::STATE_DESTROY_RESULT => {
					if self.f12 {
						State::STATE_CREATE_RESULT
					} else {
						State::STATE_CREATE_BRIEFING
					}
				}
				State::STATE_EXIT => State::STATE_EXIT,
				_ => State::STATE_NULL,
			};
		} else if self.state_mode == 2 {
			// main screen only
			self.now_state = match self.now_state {
				State::STATE_CREATE_MAINGAME => State::STATE_NOW_MAINGAME,
				State::STATE_NOW_MAINGAME => State::STATE_DESTROY_MAINGAME,
				// Both a click and F12 restart the mission, and BackSpace is the only way
				// out of mode 2. The upstream spells the two restart arms out.
				State::STATE_DESTROY_MAINGAME => {
					if self.f12 {
						State::STATE_CREATE_MAINGAME
					} else if self.back {
						State::STATE_EXIT
					} else {
						State::STATE_CREATE_MAINGAME
					}
				}
				State::STATE_EXIT => State::STATE_EXIT,
				_ => State::STATE_NULL,
			};
		} else {
			self.now_state = State::STATE_NULL;
		}

		if ENABLE_DEBUGLOG {
			// One line per screen entered, as upstream. The now and destroy states are
			// silent because the create state already named the screen.
			match self.now_state {
				State::STATE_CREATE_OPENING => {
					log.write_log(LOG_CHECK, b"State machine", b"Opening");
				}
				State::STATE_CREATE_MENU => {
					log.write_log(LOG_CHECK, b"State machine", b"Main menu");
				}
				State::STATE_CREATE_BRIEFING => {
					log.write_log(LOG_CHECK, b"State machine", b"Briefing");
				}
				State::STATE_CREATE_MAINGAME => {
					log.write_log(LOG_CHECK, b"State machine", b"Play mission");
				}
				State::STATE_CREATE_RESULT => {
					log.write_log(LOG_CHECK, b"State machine", b"Result");
				}
				_ => {}
			}
		}
	}

	/// `PushMouseButton`: received a mouse click.
	pub fn push_mouse_button(&mut self, log: &mut DebugLog) {
		self.back = false;
		self.f12 = false;
		self.next_state(log);
	}

	/// `PushBackSpaceKey`: received the BackSpace key.
	pub fn push_back_space_key(&mut self, log: &mut DebugLog) {
		self.back = true;
		self.f12 = false;
		self.next_state(log);
	}

	/// `PushF12Key`: received the F12 key.
	pub fn push_f12_key(&mut self, log: &mut DebugLog) {
		self.back = false;
		self.f12 = true;
		self.next_state(log);
	}

	/// `GetState`: returns the current state.
	pub fn get_state(&self) -> State {
		self.now_state
	}
}

impl Default for StateMachine {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A machine sitting on `state`, with the given input flags already latched.
	fn at(mode: i32, state: State, back: bool, f12: bool) -> StateMachine {
		let mut machine = StateMachine::new();
		machine.set_state_mode(mode);
		machine.state_mode = mode;
		machine.now_state = state;
		machine.back = back;
		machine.f12 = f12;
		machine
	}

	/// Steps once with only the log to write to, no input flag latched.
	fn step(machine: &mut StateMachine) -> State {
		let mut log = DebugLog::new();
		machine.next_state(&mut log);
		machine.get_state()
	}

	#[test]
	fn starts_on_the_opening_screen() {
		let machine = StateMachine::new();
		assert_eq!(machine.get_state(), State::STATE_CREATE_OPENING);
		assert_eq!(machine.state_mode, 0);
		assert!(!machine.back);
		assert!(!machine.f12);
	}

	#[test]
	fn set_state_mode_picks_the_entry_screen() {
		let mut machine = StateMachine::new();
		machine.set_state_mode(1);
		assert_eq!(machine.get_state(), State::STATE_CREATE_BRIEFING);
		machine.set_state_mode(2);
		assert_eq!(machine.get_state(), State::STATE_CREATE_MAINGAME);
	}

	#[test]
	fn set_state_mode_falls_back_to_normal() {
		for mode in [0, 3, -1, 100] {
			let mut machine = at(2, State::STATE_CREATE_MAINGAME, false, false);
			machine.set_state_mode(mode);
			assert_eq!(machine.state_mode, 0);
			assert_eq!(machine.get_state(), State::STATE_CREATE_OPENING);
		}
	}

	#[test]
	fn normal_mode_walks_every_screen_with_the_mouse() {
		// create -> now -> destroy, and the next screen, with no flag set.
		let chain = [
			State::STATE_CREATE_OPENING,
			State::STATE_NOW_OPENING,
			State::STATE_DESTROY_OPENING,
			State::STATE_CREATE_MENU,
			State::STATE_NOW_MENU,
			State::STATE_DESTROY_MENU,
			State::STATE_CREATE_BRIEFING,
			State::STATE_NOW_BRIEFING,
			State::STATE_DESTROY_BRIEFING,
			State::STATE_CREATE_MAINGAME,
			State::STATE_NOW_MAINGAME,
			State::STATE_DESTROY_MAINGAME,
			State::STATE_CREATE_RESULT,
			State::STATE_NOW_RESULT,
			State::STATE_DESTROY_RESULT,
			State::STATE_CREATE_MENU,
		];
		let mut machine = at(0, chain[0], false, false);
		for pair in chain.windows(2) {
			assert_eq!(step(&mut machine), pair[1], "from {:?}", pair[0]);
		}
	}

	#[test]
	fn the_input_flags_do_not_move_create_or_now_states() {
		// BackSpace latched inside a running screen still only disposes it, because only
		// the destroy states read the flags.
		let cases = [
			(State::STATE_CREATE_OPENING, State::STATE_NOW_OPENING),
			(State::STATE_NOW_OPENING, State::STATE_DESTROY_OPENING),
			(State::STATE_CREATE_MENU, State::STATE_NOW_MENU),
			(State::STATE_NOW_MENU, State::STATE_DESTROY_MENU),
			(State::STATE_CREATE_BRIEFING, State::STATE_NOW_BRIEFING),
			(State::STATE_NOW_BRIEFING, State::STATE_DESTROY_BRIEFING),
			(State::STATE_CREATE_MAINGAME, State::STATE_NOW_MAINGAME),
			(State::STATE_NOW_MAINGAME, State::STATE_DESTROY_MAINGAME),
			(State::STATE_CREATE_RESULT, State::STATE_NOW_RESULT),
			(State::STATE_NOW_RESULT, State::STATE_DESTROY_RESULT),
		];
		let mut log = DebugLog::new();
		for (from, to) in cases {
			let mut machine = at(0, from, false, false);
			machine.push_back_space_key(&mut log);
			assert_eq!(machine.get_state(), to, "BackSpace from {:?}", from);
		}
	}

	#[test]
	fn f12_restarts_the_screen_being_torn_down() {
		let cases = [
			(State::STATE_DESTROY_OPENING, State::STATE_CREATE_OPENING),
			(State::STATE_DESTROY_MENU, State::STATE_CREATE_MENU),
			(State::STATE_DESTROY_BRIEFING, State::STATE_CREATE_BRIEFING),
			(State::STATE_DESTROY_MAINGAME, State::STATE_CREATE_MAINGAME),
			(State::STATE_DESTROY_RESULT, State::STATE_CREATE_RESULT),
		];
		for (from, to) in cases {
			let mut machine = at(0, from, false, false);
			let mut log = DebugLog::new();
			machine.push_f12_key(&mut log);
			assert_eq!(machine.get_state(), to, "F12 from {:?}", from);
		}
	}

	#[test]
	fn back_space_leaves_the_screen() {
		// The menu is the root of mode 0, so BackSpace there ends the game.
		let mut machine = at(0, State::STATE_DESTROY_MENU, false, false);
		let mut log = DebugLog::new();
		machine.push_back_space_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_EXIT);

		// Elsewhere it goes back one screen.
		let mut machine = at(0, State::STATE_DESTROY_BRIEFING, false, false);
		machine.push_back_space_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_CREATE_MENU);

		let mut machine = at(0, State::STATE_DESTROY_MAINGAME, false, false);
		machine.push_back_space_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_CREATE_MENU);

		// The result screen has no back, only F12 and the menu.
		let mut machine = at(0, State::STATE_DESTROY_RESULT, false, false);
		machine.push_back_space_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_CREATE_MENU);
	}

	#[test]
	fn a_click_clears_a_latched_back_or_f12() {
		let mut log = DebugLog::new();
		let mut machine = at(0, State::STATE_DESTROY_MENU, true, true);
		machine.push_mouse_button(&mut log);
		assert!(!machine.back);
		assert!(!machine.f12);
		assert_eq!(machine.get_state(), State::STATE_CREATE_BRIEFING);
	}

	#[test]
	fn mode_1_walks_briefing_mission_result() {
		let chain = [
			State::STATE_CREATE_BRIEFING,
			State::STATE_NOW_BRIEFING,
			State::STATE_DESTROY_BRIEFING,
			State::STATE_CREATE_MAINGAME,
			State::STATE_NOW_MAINGAME,
			State::STATE_DESTROY_MAINGAME,
			State::STATE_CREATE_RESULT,
			State::STATE_NOW_RESULT,
			State::STATE_DESTROY_RESULT,
			State::STATE_CREATE_BRIEFING,
		];
		let mut machine = at(1, chain[0], false, false);
		for pair in chain.windows(2) {
			assert_eq!(step(&mut machine), pair[1], "from {:?}", pair[0]);
		}

		// BackSpace from the briefing's destroy state leaves the game in mode 1.
		let mut machine = at(1, State::STATE_DESTROY_BRIEFING, false, false);
		let mut log = DebugLog::new();
		machine.push_back_space_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_EXIT);

		// F12 still restarts, and the result screen goes back to the briefing.
		let mut machine = at(1, State::STATE_DESTROY_RESULT, false, false);
		machine.push_f12_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_CREATE_RESULT);
		let mut machine = at(1, State::STATE_DESTROY_MAINGAME, false, false);
		machine.push_back_space_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_CREATE_BRIEFING);
	}

	#[test]
	fn mode_2_restarts_the_mission_and_exits_on_back() {
		let mut log = DebugLog::new();

		// A click and F12 both restart, because mode 2 has nowhere else to go.
		for f12 in [false, true] {
			let mut machine = at(2, State::STATE_DESTROY_MAINGAME, false, false);
			if f12 {
				machine.push_f12_key(&mut log);
			} else {
				machine.push_mouse_button(&mut log);
			}
			assert_eq!(machine.get_state(), State::STATE_CREATE_MAINGAME);
		}

		let mut machine = at(2, State::STATE_DESTROY_MAINGAME, false, false);
		machine.push_back_space_key(&mut log);
		assert_eq!(machine.get_state(), State::STATE_EXIT);

		let mut machine = at(2, State::STATE_CREATE_MAINGAME, false, false);
		assert_eq!(step(&mut machine), State::STATE_NOW_MAINGAME);
	}

	#[test]
	fn states_outside_the_mode_are_refused() {
		// Mode 1 has no opening or menu, mode 2 has only the mission, so a state that
		// belongs to another mode lands on the invalid state rather than being guessed.
		assert_eq!(
			step(&mut at(1, State::STATE_CREATE_OPENING, false, false)),
			State::STATE_NULL
		);
		assert_eq!(
			step(&mut at(1, State::STATE_CREATE_MENU, false, false)),
			State::STATE_NULL
		);
		assert_eq!(
			step(&mut at(2, State::STATE_CREATE_BRIEFING, false, false)),
			State::STATE_NULL
		);
		assert_eq!(
			step(&mut at(2, State::STATE_DESTROY_MENU, false, false)),
			State::STATE_NULL
		);

		// There is no mode 3.
		assert_eq!(
			step(&mut at(3, State::STATE_CREATE_OPENING, false, false)),
			State::STATE_NULL
		);
		assert_eq!(
			step(&mut at(-1, State::STATE_NOW_MAINGAME, false, false)),
			State::STATE_NULL
		);

		// And the null state stays null rather than recovering on its own.
		assert_eq!(
			step(&mut at(0, State::STATE_NULL, false, false)),
			State::STATE_NULL
		);
	}

	#[test]
	fn exit_is_terminal() {
		for mode in [0, 1, 2] {
			let mut machine = at(mode, State::STATE_EXIT, false, false);
			assert_eq!(step(&mut machine), State::STATE_EXIT);
			assert_eq!(step(&mut machine), State::STATE_EXIT);
		}
	}
}
