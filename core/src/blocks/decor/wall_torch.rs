// WallTorchBlock: its own canSurvive (a sturdy face behind) and updateShape.

use crate::blocks::{wall_torch, Behaviour, NONE};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(wall_torch::update_shape), can_survive: Some(wall_torch::can_survive), ..NONE };
