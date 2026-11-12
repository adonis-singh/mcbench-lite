// CropBlock (wheat, carrots, potatoes, beetroots, torchflower): canSurvive
// needs light 8 at the crop and farmland below. entityInside acts on
// ravagers only.

use super::stem;
use crate::blocks::vegetation;
use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(vegetation::update_shape), can_survive: Some(can_survive), ..NONE };

// CropBlock.hasSufficientLight: level.getRawBrightness(pos, 0) >= 8.
pub fn has_sufficient_light(l: &mut Level, pos: Pos) -> bool {
    l.raw_brightness(pos, 0) >= 8
}

// CropBlock.canSurvive: hasSufficientLight(level, pos) && super.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    has_sufficient_light(l, pos) && vegetation::can_survive(l, pos, stem::may_place_on)
}
