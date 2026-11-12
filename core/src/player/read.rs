// What the player reads of the level, without changing it: the player is a
// client, and the level it walks in is the server's. A chunk the level does
// not hold is read as the empty chunk the engine's void world makes when
// anything asks for it (air); a position outside the build height is void air.

use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::{Fluid, FluidId, State, BLOCKS_MOTION, EMPTY_FLUID};
use crate::world::World;

// Level.getBlockState.
#[inline]
pub fn state(l: &Level, pos: Pos) -> State {
    if World::outside(pos) {
        l.known.void_air
    } else if !l.world.has_chunk(pos.x() >> 4, pos.z() >> 4) {
        l.known.air
    } else {
        l.world.get(pos)
    }
}

// Level.getFluidState.
#[inline]
pub fn fluid_state(l: &Level, pos: Pos) -> FluidId {
    if World::outside(pos) {
        return EMPTY_FLUID;
    }
    l.fluid_of(state(l, pos))
}

// FluidState.getOwnHeight.
pub fn own_height(l: &Level, fluid: FluidId) -> f32 {
    l.reg.fluids[fluid as usize].amount as f32 / 9.0f32
}

// FluidState.getHeight(level, pos): a fluid with the same fluid above fills
// the block (FlowingFluid.getHeight; the empty fluid has no height).
pub fn fluid_height(l: &Level, fluid: FluidId, pos: Pos) -> f32 {
    let this = l.fluid_type(fluid);
    if this == Fluid::Empty {
        return 0.0;
    }
    let above = fluid_state(l, pos.above());
    if this.is_same(l.fluid_type(above)) {
        1.0
    } else {
        own_height(l, fluid)
    }
}

// FlowingFluid.affectsFlow.
fn affects_flow(l: &Level, this: Fluid, fluid: FluidId) -> bool {
    fluid == EMPTY_FLUID || l.fluid_type(fluid).is_same(this)
}

// FlowingFluid.isSolidFace.
fn is_solid_face(l: &Level, this: Fluid, pos: Pos, side: Dir) -> bool {
    let state = state(l, pos);
    let fluid = fluid_state(l, pos);
    if l.fluid_type(fluid).is_same(this) {
        return false;
    }
    let block = &l.reg.blocks[l.block(state) as usize];
    let ice = block.class == "IceBlock" || block.supers.iter().any(|s| s == "IceBlock");
    side == Dir::Up || (!ice && crate::shape_ops::is_face_sturdy(l, state, pos, side, crate::registry::Support::Full))
}

// FlowingFluid.getFlow: the direction the fluid at `pos` pushes.
pub fn fluid_flow(l: &Level, pos: Pos, fluid: FluidId) -> Vec3 {
    let this = l.fluid_type(fluid);
    let (mut d, mut d1) = (0.0f64, 0.0f64);
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        let neighbor = fluid_state(l, at);
        if affects_flow(l, this, neighbor) {
            let mut neighbor_height = own_height(l, neighbor);
            let mut f = 0.0f32;
            if neighbor_height == 0.0 {
                let neighbor_state = state(l, at);
                if l.reg.flags(neighbor_state) & BLOCKS_MOTION == 0 {
                    let below = fluid_state(l, at.below());
                    if affects_flow(l, this, below) {
                        neighbor_height = own_height(l, below);
                        if neighbor_height > 0.0 {
                            f = own_height(l, fluid) - (neighbor_height - 0.8888889f32);
                        }
                    }
                }
            } else if neighbor_height > 0.0 {
                f = own_height(l, fluid) - neighbor_height;
            }
            if f != 0.0 {
                let (dx, _, dz) = dir.offset();
                d += (dx as f32 * f) as f64;
                d1 += (dz as f32 * f) as f64;
            }
        }
    }
    let mut vec = Vec3::new(d, 0.0, d1);
    if l.reg.fluids[fluid as usize].falling {
        for dir in HORIZONTAL {
            let at = pos.relative(dir);
            if is_solid_face(l, this, at, dir) || is_solid_face(l, this, at.above(), dir) {
                vec = vec.normalize().add(0.0, -6.0, 0.0);
                break;
            }
        }
    }
    vec.normalize()
}

// Level.getFluidState(pos).is(FluidTags.WATER).
pub fn fluid_state_is_water(l: &Level, pos: Pos) -> bool {
    l.fluid_type(fluid_state(l, pos)).is_water()
}
