//! The composer's placeholder in a chat with no message yet (`web/app.js`
//! `IDEAS`, `EGGS`, `SEASONAL`, `pickIdea`): a rotating invitation instead
//! of a form label. One is picked per chat view, kept while the chat is on
//! screen, picked again when another chat is shown.

/// `IDEAS`: the 82 invitations (web/app.js 1588-1669), verbatim.
pub const IDEAS: [&str; 82] = [
    "Add a cat",
    "A cozy hobbit hole",
    "A lighthouse on a stormy cliff",
    "A giant rubber duck",
    "A wizard tower, slightly crooked",
    "A ramen stand at midnight",
    "A pirate ship run aground",
    "A treehouse village with rope bridges",
    "Saturn, with rings",
    "A snow globe with a cabin inside",
    "A dragon skeleton in the desert",
    "A floating sky island with a waterfall",
    "The Moai heads of Easter Island",
    "A medieval tavern with a secret basement",
    "A whale breaching out of the ocean",
    "A cathedral of stained glass",
    "A tiny red barn with fat sheep",
    "An overgrown greenhouse",
    "A castle with an excessive moat",
    "A UFO abducting a cow",
    "A giant chess board mid-game",
    "A creeper topiary garden",
    "A Japanese pagoda in autumn",
    "A desert oasis with palm trees",
    "A steampunk airship docked at a tower",
    "A volcano with a lava river",
    "A cherry blossom bridge over a koi pond",
    "An abandoned mineshaft entrance",
    "A cyberpunk alley with neon signs",
    "A sunken temple, half flooded",
    "A windmill on a hill of wheat",
    "A bonsai tree, but huge",
    "A space station orbiting nothing in particular",
    "An igloo village under the northern lights",
    "A lunar base with a rover",
    "A mushroom forest with glowing caps",
    "A Greek temple on a cliff",
    "A cozy library with a reading nook",
    "A coral reef with a shipwreck",
    "A haunted Victorian house",
    "A giant clock tower",
    "A beach hut with a surfboard",
    "A Viking longhouse",
    "A dam holding back a reservoir",
    "A roller coaster that loops",
    "A Mayan step pyramid in the jungle",
    "A fishing village on stilts",
    "A potted cactus the size of a building",
    "The Eiffel Tower",
    "The Colosseum",
    "The Golden Gate Bridge in fog",
    "A Dutch canal house row",
    "A Moroccan riad with a courtyard fountain",
    "A train station with a steam engine",
    "A hot air balloon over a valley",
    "A giant teapot cottage",
    "A burger the size of a house",
    "A bowl of fruit, enormous",
    "A sleeping fox curled up",
    "An axolotl, pixel art",
    "A parrot on a pirate hat",
    "A guitar lying on its side",
    "A grand piano",
    "A pixel art sunset",
    "A rocket on the launch pad",
    "A submarine surfacing",
    "A medieval market square",
    "An observatory with an open dome",
    "A desert canyon with a rope bridge",
    "A frozen waterfall",
    "A hedge maze with a fountain in the middle",
    "A tiny planet with one house on it",
    "A sandcastle, very serious",
    "A working 4-bit adder with lamps",
    "A piston door with a combination lock",
    "A redstone clock that ticks on a display",
    "A seven-segment display that counts up",
    "An automatic farm with hoppers",
    "A flying machine that goes and comes back",
    "A secret staircase behind a bookshelf",
    "A marble run made of rails",
    "A vending machine that dispenses snacks",
];

/// `EGGS`: the ten rarer ones (web/app.js 1674-1683).
pub const EGGS: [&str; 10] = [
    "Herobrine's summer house",
    "A redstone computer that runs Minecraft (good luck)",
    "The cake, which is not a lie",
    "A statue of the person reading this",
    "A to-scale replica of this text box",
    "Something that would confuse an archaeologist",
    "Whatever you were going to build anyway",
    "A monument to the first dirt hut",
    "A house that is definitely not haunted",
    "The Far Lands, but cozy",
];

/// `SEASONAL`: by month and day, the day's idea.
pub const SEASONAL: [((u32, u32), &str); 6] = [
    ((1, 1), "Fireworks over a city at midnight"),
    ((2, 14), "A heart made of roses in a garden"),
    ((3, 17), "A pot of gold at the end of a rainbow"),
    ((4, 1), "A perfectly normal house, upside down"),
    ((10, 31), "A haunted manor with a pumpkin patch"),
    ((12, 25), "A gingerbread village under snow"),
];

/// `pickIdea`: on a seasonal day (local `month` and `day`), with
/// probability one half the day's idea; otherwise with probability 1/40 a
/// random egg, else a random idea. `random` gives numbers in [0, 1).
pub fn pick_idea(month: u32, day: u32, mut random: impl FnMut() -> f64) -> &'static str {
    let seasonal = SEASONAL
        .iter()
        .find(|((m, d), _)| (*m, *d) == (month, day))
        .map(|(_, idea)| *idea);
    if let Some(idea) = seasonal
        && random() < 0.5
    {
        return idea;
    }
    let pool: &[&str] = if random() < 1. / 40. { &EGGS } else { &IDEAS };
    pool[((random() * pool.len() as f64) as usize).min(pool.len() - 1)]
}

/// A number in [0, 1) from the process's random hasher keys.
pub fn random() -> f64 {
    use std::hash::{BuildHasher as _, Hasher as _};
    let bits = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    (bits >> 11) as f64 / (1u64 << 53) as f64
}

/// The idea for a chat view shown now, by the local date.
pub fn idea_now() -> &'static str {
    use chrono::Datelike as _;
    let today = chrono::Local::now().date_naive();
    pick_idea(today.month(), today.day(), random)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idea_is_the_days_half_the_time_on_a_seasonal_day_else_an_egg_or_an_idea() {
        let mut numbers = [0.4].into_iter();
        assert_eq!(
            pick_idea(12, 25, || numbers.next().expect("a number")),
            "A gingerbread village under snow"
        );
        let mut numbers = [0.6, 0.5, 0.0].into_iter();
        assert_eq!(
            pick_idea(12, 25, || numbers.next().expect("a number")),
            "Add a cat"
        );
        let mut numbers = [0.01, 0.999].into_iter();
        assert_eq!(
            pick_idea(6, 1, || numbers.next().expect("a number")),
            "The Far Lands, but cozy"
        );
        let mut numbers = [0.5, 0.999].into_iter();
        assert_eq!(
            pick_idea(6, 1, || numbers.next().expect("a number")),
            "A vending machine that dispenses snacks"
        );
        assert_eq!((IDEAS.len(), EGGS.len()), (82, 10));
        let r = random();
        assert!((0. ..1.).contains(&r));
    }
}
