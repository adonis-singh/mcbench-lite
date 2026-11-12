// TorchBlock: BaseTorchBlock's updateShape and canSurvive.

use crate::blocks::{torch, Behaviour, NONE};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(torch::update_shape), can_survive: Some(torch::can_survive), ..NONE };
