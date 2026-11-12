// Minecraft's Vec3, AABB and the Mth and Math functions entity code uses,
// with Java's arithmetic: doubles and floats as IEEE values, Math.min and
// Math.max as Java defines them for NaN and signed zeros.

// Math.min(double, double).
#[inline]
pub fn jmin(a: f64, b: f64) -> f64 {
    if a != a {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

// Math.max(double, double).
#[inline]
pub fn jmax(a: f64, b: f64) -> f64 {
    if a != a {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}

// Mth.floor(double).
#[inline]
pub fn floor(d: f64) -> i32 {
    let i = d as i32;
    if d < i as f64 {
        i - 1
    } else {
        i
    }
}

// Mth.ceil(double).
#[inline]
pub fn ceil(d: f64) -> i32 {
    let i = d as i32;
    if d > i as f64 {
        i + 1
    } else {
        i
    }
}

// Mth.lfloor(double).
#[inline]
pub fn lfloor(d: f64) -> i64 {
    let l = d as i64;
    if d < l as f64 {
        l - 1
    } else {
        l
    }
}

// Mth.frac(double).
#[inline]
pub fn frac(d: f64) -> f64 {
    d - lfloor(d) as f64
}

// Mth.lerp(double, double, double).
#[inline]
pub fn lerp(delta: f64, start: f64, end: f64) -> f64 {
    start + delta * (end - start)
}

// Mth.sign(double).
#[inline]
pub fn sign(d: f64) -> i32 {
    if d == 0.0 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

// Mth.equal(double, double).
#[inline]
pub fn equal(x: f64, y: f64) -> bool {
    (y - x).abs() < 1.0E-5f32 as f64
}

// Mth.clamp(double, double, double).
#[inline]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else {
        jmin(value, max)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    #[inline]
    pub fn add(self, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(self.x + x, self.y + y, self.z + z)
    }

    #[inline]
    pub fn add_vec(self, v: Vec3) -> Vec3 {
        self.add(v.x, v.y, v.z)
    }

    // Vec3.subtract(x, y, z) is add(-x, -y, -z).
    #[inline]
    pub fn subtract(self, v: Vec3) -> Vec3 {
        self.add(-v.x, -v.y, -v.z)
    }

    #[inline]
    pub fn multiply(self, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(self.x * x, self.y * y, self.z * z)
    }

    #[inline]
    pub fn scale(self, factor: f64) -> Vec3 {
        self.multiply(factor, factor, factor)
    }

    #[inline]
    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    #[inline]
    pub fn length_sqr(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    #[inline]
    pub fn horizontal_distance_sqr(self) -> f64 {
        self.x * self.x + self.z * self.z
    }

    // Vec3.horizontalDistance.
    #[inline]
    pub fn horizontal_distance(self) -> f64 {
        (self.x * self.x + self.z * self.z).sqrt()
    }

    // Vec3.dot.
    #[inline]
    pub fn dot(self, v: Vec3) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    // Vec3.horizontal.
    #[inline]
    pub fn horizontal(self) -> Vec3 {
        Vec3::new(self.x, 0.0, self.z)
    }

    // Vec3.projectedOn.
    pub fn projected_on(self, v: Vec3) -> Vec3 {
        if v.length_sqr() == 0.0 {
            v
        } else {
            v.scale(self.dot(v)).scale(1.0 / v.length_sqr())
        }
    }

    // Vec3.distanceTo.
    #[inline]
    pub fn distance_to(self, other: Vec3) -> f64 {
        self.distance_to_sqr(other).sqrt()
    }

    pub fn normalize(self) -> Vec3 {
        let root = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if root < 1.0E-5f32 as f64 {
            Vec3::ZERO
        } else {
            Vec3::new(self.x / root, self.y / root, self.z / root)
        }
    }

    // this.distanceToSqr(other).
    #[inline]
    pub fn distance_to_sqr(self, other: Vec3) -> f64 {
        let d = other.x - self.x;
        let d1 = other.y - self.y;
        let d2 = other.z - self.z;
        d * d + d1 * d1 + d2 * d2
    }

    pub fn get(self, axis: usize) -> f64 {
        [self.x, self.y, self.z][axis]
    }

    pub fn with(self, axis: usize, length: f64) -> Vec3 {
        Vec3::new(if axis == 0 { length } else { self.x }, if axis == 1 { length } else { self.y }, if axis == 2 { length } else { self.z })
    }

    // Vec3.relative(direction, length) along an axis in its positive or
    // negative direction.
    pub fn relative(self, axis: usize, positive: bool, length: f64) -> Vec3 {
        let step = if positive { 1.0 } else { -1.0 };
        Vec3::new(
            self.x + length * if axis == 0 { step } else { 0.0 },
            self.y + length * if axis == 1 { step } else { 0.0 },
            self.z + length * if axis == 2 { step } else { 0.0 },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    // new AABB(x1, y1, z1, x2, y2, z2).
    #[inline]
    pub fn new(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Aabb {
        Aabb { min_x: jmin(x1, x2), min_y: jmin(y1, y2), min_z: jmin(z1, z2), max_x: jmax(x1, x2), max_y: jmax(y1, y2), max_z: jmax(z1, z2) }
    }

    #[inline]
    pub fn expand_towards(&self, x: f64, y: f64, z: f64) -> Aabb {
        let (mut d, mut d1, mut d2, mut d3, mut d4, mut d5) = (self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z);
        if x < 0.0 {
            d += x;
        } else if x > 0.0 {
            d3 += x;
        }
        if y < 0.0 {
            d1 += y;
        } else if y > 0.0 {
            d4 += y;
        }
        if z < 0.0 {
            d2 += z;
        } else if z > 0.0 {
            d5 += z;
        }
        Aabb::new(d, d1, d2, d3, d4, d5)
    }

    #[inline]
    pub fn inflate(&self, x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(self.min_x - x, self.min_y - y, self.min_z - z, self.max_x + x, self.max_y + y, self.max_z + z)
    }

    #[inline]
    pub fn deflate(&self, v: f64) -> Aabb {
        self.inflate(-v, -v, -v)
    }

    #[inline]
    pub fn mv(&self, x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(self.min_x + x, self.min_y + y, self.min_z + z, self.max_x + x, self.max_y + y, self.max_z + z)
    }

    // AABB.intersects(AABB).
    #[inline]
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min_x < o.max_x && self.max_x > o.min_x && self.min_y < o.max_y && self.max_y > o.min_y && self.min_z < o.max_z && self.max_z > o.min_z
    }

    #[inline]
    pub fn intersects_box(&self, x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> bool {
        self.min_x < x2 && self.max_x > x1 && self.min_y < y2 && self.max_y > y1 && self.min_z < z2 && self.max_z > z1
    }

    #[inline]
    pub fn contains(&self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x && x < self.max_x && y >= self.min_y && y < self.max_y && z >= self.min_z && z < self.max_z
    }

    pub fn xsize(&self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn ysize(&self) -> f64 {
        self.max_y - self.min_y
    }

    pub fn zsize(&self) -> f64 {
        self.max_z - self.min_z
    }

    pub fn center(&self) -> Vec3 {
        Vec3::new(lerp(0.5, self.min_x, self.max_x), lerp(0.5, self.min_y, self.max_y), lerp(0.5, self.min_z, self.max_z))
    }

    pub fn min(&self, axis: usize) -> f64 {
        [self.min_x, self.min_y, self.min_z][axis]
    }

    // AABB.collidedAlongVector(vector, boxes).
    pub fn collided_along_vector(&self, vector: Vec3, boxes: &[Aabb]) -> bool {
        let center = self.center();
        let end = center.add_vec(vector);
        for b in boxes {
            let grown = b.inflate(self.xsize() * 0.5 - 1.0E-7, self.ysize() * 0.5 - 1.0E-7, self.zsize() * 0.5 - 1.0E-7);
            if grown.contains(end.x, end.y, end.z) || grown.contains(center.x, center.y, center.z) {
                return true;
            }
            if clip_box([grown.min_x, grown.min_y, grown.min_z], [grown.max_x, grown.max_y, grown.max_z], center, end).is_some() {
                return true;
            }
        }
        false
    }

    pub fn max(&self, axis: usize) -> f64 {
        [self.max_x, self.max_y, self.max_z][axis]
    }
}

// AABB.clip(minX..., from, to): where the segment from `from` to `to` first
// meets the box, if it does (Direction is not needed: only the point).
pub fn clip_box(min: [f64; 3], max: [f64; 3], from: Vec3, to: Vec3) -> Option<Vec3> {
    let mut distance = 1.0f64;
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    let mut hit = false;
    // clipPoint(minDistance, ..., distanceSide, distanceOtherA, distanceOtherB, minSide, minA, maxA, minB, maxB, start...)
    let mut clip = |distance: &mut f64, side: f64, other_a: f64, other_b: f64, min_side: f64, min_a: f64, max_a: f64, min_b: f64, max_b: f64, start_side: f64, start_a: f64, start_b: f64| {
        let d = (min_side - start_side) / side;
        let d1 = start_a + d * other_a;
        let d2 = start_b + d * other_b;
        if 0.0 < d && d < *distance && min_a - 1.0E-7 < d1 && d1 < max_a + 1.0E-7 && min_b - 1.0E-7 < d2 && d2 < max_b + 1.0E-7 {
            *distance = d;
            hit = true;
        }
    };
    if dx > 1.0E-7 {
        clip(&mut distance, dx, dy, dz, min[0], min[1], max[1], min[2], max[2], from.x, from.y, from.z);
    } else if dx < -1.0E-7 {
        clip(&mut distance, dx, dy, dz, max[0], min[1], max[1], min[2], max[2], from.x, from.y, from.z);
    }
    if dy > 1.0E-7 {
        clip(&mut distance, dy, dz, dx, min[1], min[2], max[2], min[0], max[0], from.y, from.z, from.x);
    } else if dy < -1.0E-7 {
        clip(&mut distance, dy, dz, dx, max[1], min[2], max[2], min[0], max[0], from.y, from.z, from.x);
    }
    if dz > 1.0E-7 {
        clip(&mut distance, dz, dx, dy, min[2], min[0], max[0], min[1], max[1], from.z, from.x, from.y);
    } else if dz < -1.0E-7 {
        clip(&mut distance, dz, dx, dy, max[2], min[0], max[0], min[1], max[1], from.z, from.x, from.y);
    }
    if hit {
        Some(from.add(distance * dx, distance * dy, distance * dz))
    } else {
        None
    }
}
