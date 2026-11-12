// LegacyRandomSource (RandomSource.create(seed)), the level's random.

pub struct Random {
    seed: i64,
}

const MULTIPLIER: i64 = 25214903917;
const MASK: i64 = (1 << 48) - 1;

impl Random {
    pub fn new(seed: i64) -> Random {
        Random { seed: (seed ^ MULTIPLIER) & MASK }
    }

    // LegacyRandomSource's seed field.
    pub fn state(&self) -> i64 {
        self.seed
    }

    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULTIPLIER).wrapping_add(11)) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let i = self.next(31);
            let r = i % bound;
            if i.wrapping_sub(r).wrapping_add(bound - 1) >= 0 {
                return r;
            }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        let hi = self.next(32) as i64;
        let lo = self.next(32) as i64;
        (hi << 32).wrapping_add(lo)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * 5.9604645E-8f32
    }

    pub fn next_double(&mut self) -> f64 {
        let a = self.next(26) as i64;
        let b = self.next(27) as i64;
        (((a << 27) + b) as f32 * 1.110223E-16f32) as f64
    }

    // RandomSource.triangle(center, maxDeviation).
    pub fn triangle(&mut self, center: f64, max_deviation: f64) -> f64 {
        let a = self.next_double();
        let b = self.next_double();
        center + max_deviation * (a - b)
    }
}

#[cfg(test)]
mod tests {
    use super::Random;

    // java.util.Random(0) draws the same sequence as LegacyRandomSource(0).
    #[test]
    fn matches_java() {
        let mut r = Random::new(0);
        assert_eq!(r.next_int(), -1155484576);
        assert_eq!(r.next_int(), -723955400);
        let mut r = Random::new(42);
        assert_eq!(r.next_int_bound(10), 0);
        assert_eq!(r.next_int_bound(10), 3);
        let mut r = Random::new(0);
        assert_eq!(r.next_long(), -4962768465676381896);
    }
}
