// The shape and attachment blocks: slabs, stairs, trapdoors, doors, fence
// gates, fences, walls, bars and panes, signs, ladders, carved pumpkins,
// banners and coral fans. A module per Java class (the weathering copper
// subclasses add only random ticks, which this port never runs, and share
// their parent's); common.rs has the code they share.

pub mod banner;
pub mod base_coral_fan;
pub mod base_coral_wall_fan;
pub mod carved_pumpkin;
pub mod ceiling_hanging_sign;
pub mod common;
pub mod coral;
pub mod coral_fan;
pub mod coral_wall_fan;
pub mod door;
pub mod fence;
pub mod fence_gate;
pub mod iron_bars;
pub mod ladder;
pub mod sign;
pub mod slab;
pub mod stained_glass_pane;
pub mod stairs;
pub mod standing_sign;
pub mod trapdoor;
pub mod wall;
pub mod wall_banner;
pub mod wall_hanging_sign;
pub mod wall_sign;
