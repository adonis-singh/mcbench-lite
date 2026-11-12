// BlueMap's ArrayTileModel: one record per triangle ("face"), every
// attribute in its own array, transforms applied to a window of faces.

pub const MAX_CAPACITY: usize = 1_000_000;

// A tile reached BlueMap's face limit: it is saved with the faces so far.
pub struct Full;

#[derive(Default)]
pub struct Tile {
    pub size: usize,
    pub position: Vec<f32>,
    pub uv: Vec<f32>,
    pub ao: Vec<f32>,
    pub color: Vec<f32>,
    pub sunlight: Vec<u8>,
    pub blocklight: Vec<u8>,
    pub material: Vec<i32>,
    // The faces' indices stably sorted by material (`sort`).
    pub order: Vec<u32>,
}

impl Tile {
    pub const fn new() -> Tile {
        Tile { size: 0, position: Vec::new(), uv: Vec::new(), ao: Vec::new(), color: Vec::new(), sunlight: Vec::new(), blocklight: Vec::new(), material: Vec::new(), order: Vec::new() }
    }

    // Empties the tile and keeps the memory its arrays grew to.
    pub fn clear(&mut self) {
        self.size = 0;
        self.position.clear();
        self.uv.clear();
        self.ao.clear();
        self.color.clear();
        self.sunlight.clear();
        self.blocklight.clear();
        self.material.clear();
        self.order.clear();
    }

    // ArrayTileModel.add: it throws once the faces would pass the limit.
    pub fn room(&self, count: usize) -> Result<(), Full> {
        if self.size + count > MAX_CAPACITY {
            return Err(Full);
        }
        Ok(())
    }

    // The two faces of a quad, (a, b, c) and (a, c, d): one material, color
    // and light for both, the quad's corners, uvs and ambient occlusion.
    #[allow(clippy::too_many_arguments)]
    pub fn quad(&mut self, c: [[f32; 3]; 4], uv: [f32; 8], color: [f32; 3], ao: [f32; 4], sunlight: u8, blocklight: u8, material: i32) {
        self.size += 2;
        for corner in [c[0], c[1], c[2], c[0], c[2], c[3]] {
            self.position.extend_from_slice(&corner);
        }
        self.uv.extend_from_slice(&[uv[0], uv[1], uv[2], uv[3], uv[4], uv[5], uv[0], uv[1], uv[4], uv[5], uv[6], uv[7]]);
        self.ao.extend_from_slice(&[ao[0], ao[1], ao[2], ao[0], ao[2], ao[3]]);
        self.color.extend_from_slice(&[color[0], color[1], color[2], color[0], color[1], color[2]]);
        self.sunlight.extend_from_slice(&[sunlight; 2]);
        self.blocklight.extend_from_slice(&[blocklight; 2]);
        self.material.extend_from_slice(&[material; 2]);
    }

    // ArrayTileModel.transform with a 4x4 matrix: each sum left to right,
    // no fused multiply-add.
    pub fn transform(&mut self, start: usize, m: &[f32; 12]) {
        for p in self.position[start * 9..self.size * 9].chunks_exact_mut(3) {
            let (x, y, z) = (p[0], p[1], p[2]);
            p[0] = m[0] * x + m[1] * y + m[2] * z + m[3];
            p[1] = m[4] * x + m[5] * y + m[6] * z + m[7];
            p[2] = m[8] * x + m[9] * y + m[10] * z + m[11];
        }
    }

    pub fn translate(&mut self, start: usize, dx: f32, dy: f32, dz: f32) {
        for p in self.position[start * 9..self.size * 9].chunks_exact_mut(3) {
            p[0] += dx;
            p[1] += dy;
            p[2] += dz;
        }
    }

    pub fn scale(&mut self, start: usize, s: f32) {
        for p in self.position[start * 9..self.size * 9].chunks_exact_mut(3) {
            p[0] *= s;
            p[1] *= s;
            p[2] *= s;
        }
    }

    // ArrayTileModel.sort: the faces stably sorted by material. BlueMap
    // sorts face indices with fastutil's stable merge sort and then moves
    // the faces in place; a stable sort's order is unique and the move is a
    // permutation, so the faces' indices in a stable sort's order are the
    // faces in BlueMap's (the test below holds the two to it). The faces
    // stay where they are; `order` says which face is where.
    pub fn sort(&mut self) {
        self.order.clear();
        self.order.extend(0..self.size as u32);
        let material = &self.material;
        self.order.sort_by_key(|&face| material[face as usize]);
    }
}

#[cfg(test)]
mod tests {
    // BlueMap's own sort, literally: MergeSort.mergeSortInt (from fastutil)
    // over face indices, then ArrayTileModel.sort's in-place move.
    fn bluemap_sort(material: &mut Vec<i32>, payload: &mut Vec<u32>) {
        let size = material.len();
        let mut sorted: Vec<i32> = (0..size as i32).collect();
        let mut support = sorted.clone();
        let keys = material.clone();
        merge_sort(&mut sorted, 0, size, &|a, b| keys[a as usize].cmp(&keys[b as usize]), &mut support);
        for i in 0..size {
            let mut s = sorted[i] as usize;
            while s < i {
                s = sorted[s] as usize;
            }
            material.swap(i, s);
            payload.swap(i, s);
        }
    }

    type Compare<'a> = dyn Fn(i32, i32) -> std::cmp::Ordering + 'a;

    fn merge_sort(a: &mut [i32], from: usize, to: usize, comp: &Compare, supp: &mut [i32]) {
        let len = to - from;
        if len < 16 {
            insertion_sort(a, from, to, comp);
            return;
        }
        let mid = (from + to) >> 1;
        merge_sort(supp, from, mid, comp, a);
        merge_sort(supp, mid, to, comp, a);
        if comp(supp[mid - 1], supp[mid]).is_le() {
            a[from..to].copy_from_slice(&supp[from..to]);
            return;
        }
        let (mut p, mut q) = (from, mid);
        for item in a.iter_mut().take(to).skip(from) {
            if q >= to || (p < mid && comp(supp[p], supp[q]).is_le()) {
                *item = supp[p];
                p += 1;
            } else {
                *item = supp[q];
                q += 1;
            }
        }
    }

    fn insertion_sort(a: &mut [i32], from: usize, to: usize, comp: &Compare) {
        let mut i = from;
        loop {
            i += 1;
            if i >= to {
                break;
            }
            let t = a[i];
            let mut j = i;
            let mut u = a[j - 1];
            while comp(t, u).is_lt() {
                a[j] = u;
                if from == j - 1 {
                    j -= 1;
                    break;
                }
                j -= 1;
                u = a[j - 1];
            }
            a[j] = t;
        }
    }

    #[test]
    fn sort_matches_bluemap() {
        let mut state = 12345u64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) as u32
        };
        for round in 0..400 {
            let size = [0, 1, 2, 15, 16, 17, 100, 1000, 20000][round % 9] + (next() % 50) as usize;
            let kinds = 1 + next() % [2, 5, 40, 2000][round % 4];
            let mut material: Vec<i32> = (0..size).map(|_| (next() % kinds) as i32).collect();
            let mut payload: Vec<u32> = (0..size as u32).collect();
            let mut tile = super::Tile::default();
            tile.size = size;
            tile.material = material.clone();
            tile.blocklight = (0..size).map(|face| face as u8).collect();
            tile.position = vec![0.0; size * 9];
            for (face, value) in tile.position.chunks_exact_mut(9).enumerate() {
                value[0] = face as f32;
            }
            tile.sort();
            bluemap_sort(&mut material, &mut payload);
            let sorted = |values: &Vec<i32>| tile.order.iter().map(|&face| values[face as usize]).collect::<Vec<i32>>();
            assert_eq!(sorted(&tile.material), material);
            let moved: Vec<u32> = tile.order.iter().map(|&face| tile.position[face as usize * 9] as u32).collect();
            assert_eq!(moved, payload);
            assert!(tile.order.iter().zip(&payload).all(|(&face, &p)| tile.blocklight[face as usize] == p as u8));
        }
    }
}
