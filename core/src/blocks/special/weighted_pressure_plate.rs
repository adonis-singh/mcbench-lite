// WeightedPressurePlateBlock: the signal counts the entities on the plate
// against its maximum weight (15 for the light plate, 150 for the heavy one).

use crate::level::Level;
use crate::property;
use crate::registry::State;

fn power(l: &Level) -> u16 {
    property!(l, "power", ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15"])
}

pub fn signal_for_state(l: &Level, state: State) -> i32 {
    l.reg.get_int(state, power(l))
}

pub fn set_signal_for_state(l: &Level, state: State, strength: i32) -> State {
    l.reg.with_int(state, power(l), strength)
}

// getSignalStrength, with `count` entities that no event cancels.
pub fn signal_strength(count: i32, max_weight: i32) -> i32 {
    if count > 0 {
        let f = count.min(max_weight) as f32 / max_weight as f32;
        // Mth.ceil(f * 15.0F).
        let value = f * 15.0f32;
        let i = value as i32;
        if value > i as f32 {
            i + 1
        } else {
            i
        }
    } else {
        0
    }
}
