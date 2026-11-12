// BaseCoralFanBlock and BaseCoralPlantBlock (the dead fans and plants; the
// same code, BaseCoralPlantTypeBlock's).

use super::coral;
use crate::blocks::{Behaviour, NONE};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(coral::base_update_shape), can_survive: Some(coral::can_survive), ..NONE };
