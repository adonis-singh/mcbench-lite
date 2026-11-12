// The damage sources the entities of this level take: DamageSource and its
// damage type tags, as far as the level makes them.

// A damage source by its damage type. Explosion is the explosion damage type
// (no entity causes it: the explosions here are primed TNT's and a TNT
// minecart's); the others are what fire, lava and the blocks entities touch
// do (no entity causes them either).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dmg {
    // DamageTypes.LAVA.
    Lava,
    // DamageTypes.ON_FIRE.
    OnFire,
    // DamageTypes.EXPLOSION.
    Explosion,
    // DamageTypes.OUT_OF_WORLD (the void).
    OutOfWorld,
    // DamageTypes.IN_FIRE (fire and soul fire).
    InFire,
    // DamageTypes.CAMPFIRE.
    Campfire,
    // DamageTypes.CACTUS.
    Cactus,
    // DamageTypes.SWEET_BERRY_BUSH.
    SweetBerryBush,
    // DamageTypes.HOT_FLOOR (magma).
    HotFloor,
    // DamageTypes.FREEZE (powder snow).
    Freeze,
}

impl Dmg {
    // DamageTypeTags.BYPASSES_INVULNERABILITY (out_of_world and generic_kill).
    pub fn bypasses_invulnerability(self) -> bool {
        matches!(self, Dmg::OutOfWorld)
    }

    // DamageTypeTags.IS_FIRE.
    pub fn is_fire(self) -> bool {
        matches!(self, Dmg::Lava | Dmg::OnFire | Dmg::InFire | Dmg::Campfire | Dmg::HotFloor)
    }

    // DamageTypeTags.IGNITES_ARMOR_STANDS (in_fire and campfire).
    pub fn ignites_armor_stands(self) -> bool {
        matches!(self, Dmg::InFire | Dmg::Campfire)
    }

    // DamageTypeTags.IS_EXPLOSION.
    pub fn is_explosion(self) -> bool {
        matches!(self, Dmg::Explosion)
    }
}
