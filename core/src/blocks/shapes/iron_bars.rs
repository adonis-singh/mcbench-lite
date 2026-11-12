// IronBarsBlock: iron bars, glass panes and copper bars (the weathering
// copper subclass adds random ticks).

use super::common::{cross_property, is_exception_for_connection, schedule_water_tick};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};
use crate::{shape_classes, shape_tag};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

// instanceof IronBarsBlock.
pub fn is_iron_bars(l: &Level, state: State) -> bool {
    shape_classes!(l, ["IronBarsBlock", "StainedGlassPaneBlock", "WeatheringCopperBarsBlock"])[l.block(state) as usize]
}

// IronBarsBlock.attachsTo.
fn attachs_to(l: &Level, state: State, solid_side: bool) -> bool {
    !is_exception_for_connection(l, state) && solid_side || is_iron_bars(l, state) || shape_tag!(l, "minecraft:walls")[l.block(state) as usize]
}

// IronBarsBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction.is_horizontal() {
        let attaches = attachs_to(l, neighbor, crate::shape_ops::is_face_sturdy(l, neighbor, neighbor_pos, direction.opposite(), Support::Full));
        l.reg.with_bool(state, cross_property(l, direction), attaches)
    } else {
        state
    }
}
