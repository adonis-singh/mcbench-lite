// The order Minecraft draws translucent quads in (store.rs: octree_visits,
// sort_quads), against Minecraft's own structure: an octree built as
// net.minecraft.client.renderer.Octree builds one (Branch.add from the root,
// children created on demand, eight slots a branch) and walked as
// Octree.visitNodes walks it (a branch, then its slots in index order).

use mcbench_draw::assets::Level;
use mcbench_draw::store::{octree_visits, sort_quads};

const OVERWORLD: Level = Level { min_y: -64, height: 384 };

enum Node {
    Branch(Box<Branch>),
    Leaf([i32; 3]),
}

struct Branch {
    min: [i32; 3],
    max: [i32; 3],
    centre: [i32; 3],
    shifts: [usize; 3],
    camera_negative: [bool; 3],
    nodes: [Option<Node>; 8],
}

impl Branch {
    // Octree$Branch.<init>: its centre (min + span / 2), the camera's side
    // and the axis sorting (Octree$AxisSorting.getAxisSorting).
    fn new(min: [i32; 3], max: [i32; 3], camera_centre: [i32; 3]) -> Branch {
        let centre = [0, 1, 2].map(|k| min[k] + (max[k] - min[k] + 1) / 2);
        let d = [0, 1, 2].map(|k| camera_centre[k] - centre[k]);
        let (x, y, z) = (d[0].abs(), d[1].abs(), d[2].abs());
        // XYZ, XZY, YXZ, YZX, ZXY, ZYX: (xShift, yShift, zShift).
        let shifts = if x > y && x > z {
            if y > z { [4, 2, 1] } else { [4, 1, 2] }
        } else if y > x && y > z {
            if x > z { [2, 4, 1] } else { [1, 4, 2] }
        } else if x > y {
            [2, 1, 4]
        } else {
            [1, 2, 4]
        };
        Branch { min, max, centre, shifts, camera_negative: d.map(|v| v < 0), nodes: Default::default() }
    }

    fn add(&mut self, section: [i32; 3], camera_centre: [i32; 3]) {
        let negative = [0, 1, 2].map(|k| section[k] * 16 - self.centre[k] < 0);
        let index: usize = (0..3).filter(|&k| negative[k] != self.camera_negative[k]).map(|k| self.shifts[k]).sum();
        if self.max[0] - self.min[0] + 1 == 32 {
            self.nodes[index] = Some(Node::Leaf(section));
            return;
        }
        if self.nodes[index].is_none() {
            let min = [0, 1, 2].map(|k| if negative[k] { self.min[k] } else { self.centre[k] });
            let max = [0, 1, 2].map(|k| if negative[k] { self.centre[k] - 1 } else { self.max[k] });
            self.nodes[index] = Some(Node::Branch(Box::new(Branch::new(min, max, camera_centre))));
        }
        match self.nodes[index].as_mut().unwrap() {
            Node::Branch(b) => b.add(section, camera_centre),
            Node::Leaf(_) => unreachable!(),
        }
    }

    fn visit(&self, out: &mut Vec<[i32; 3]>) {
        for node in self.nodes.iter().flatten() {
            match node {
                Node::Branch(b) => b.visit(out),
                Node::Leaf(s) => out.push(*s),
            }
        }
    }
}

// Octree.<init>(cameraSection, renderDistance, sectionsY, minY).
fn minecraft_order(sections: &[[i32; 3]], camera_section: [i32; 3], distance: i32, level: Level) -> Vec<[i32; 3]> {
    let size = ((2 * distance + 1) as u32).next_power_of_two() as i32;
    let origin = camera_section.map(|v| v * 16);
    let min_x = origin[0] - distance * 16;
    let min_y = if size >= level.height / 16 { level.min_y } else { origin[1] - distance * 16 };
    let min_z = origin[2] - distance * 16;
    let min = [min_x, min_y, min_z];
    let max = min.map(|v| v + size * 16 - 1);
    let centre = origin.map(|v| v + 8);
    let mut root = Branch::new(min, max, centre);
    for s in sections {
        root.add(*s, centre);
    }
    let mut out = Vec::new();
    root.visit(&mut out);
    out
}

// A small deterministic generator.
struct Rng(u64);
impl Rng {
    fn next(&mut self, n: i32) -> i32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as i32
    }
}

#[test]
fn sections_are_visited_in_the_octrees_order() {
    let mut rng = Rng(7);
    for case in 0..200 {
        // Sections of a world within 12 chunks of the camera (Minecraft's
        // default render distance), the camera anywhere near it.
        let mut sections: Vec<[i32; 3]> = Vec::new();
        let camera = [rng.next(400) as f64 - 200.0 + 0.37, rng.next(300) as f64 - 40.0 + 0.61, rng.next(400) as f64 - 200.0 + 0.13];
        let cam_section = camera.map(|v| (v.floor() as i32).div_euclid(16));
        for _ in 0..1 + rng.next(60) {
            let s = [cam_section[0] + rng.next(25) - 12, rng.next(24) - 4, cam_section[2] + rng.next(25) - 12];
            if !sections.contains(&s) {
                sections.push(s);
            }
        }
        let keys = octree_visits(&sections, camera, OVERWORLD);
        let mut ours: Vec<usize> = (0..sections.len()).collect();
        ours.sort_by_key(|&i| keys[i]);
        let ours: Vec<[i32; 3]> = ours.into_iter().map(|i| sections[i]).collect();
        assert_eq!(ours, minecraft_order(&sections, cam_section, 12, OVERWORLD), "case {case}: camera {camera:?}");
    }
}

#[test]
fn a_world_reaching_past_the_render_distance_is_ordered_in_a_larger_octree() {
    let camera = [8.5, 70.0, 8.5];
    let sections = vec![[0, 4, 0], [20, 4, 0], [-30, 3, 5], [3, 2, -14]];
    let keys = octree_visits(&sections, camera, OVERWORLD);
    let mut ours: Vec<usize> = (0..sections.len()).collect();
    ours.sort_by_key(|&i| keys[i]);
    let ours: Vec<[i32; 3]> = ours.into_iter().map(|i| sections[i]).collect();
    assert_eq!(ours, minecraft_order(&sections, [0, 4, 0], 30, OVERWORLD));
}

#[test]
fn quads_are_sorted_by_the_centroid_of_their_first_and_third_corners() {
    // Two water tops of one section, their heights uneven: by the centroid
    // of corners 0 and 2 (MeshData.unpackQuadCentroids) the first is
    // farther from a camera above corner 0's side; by the mean of all four
    // corners it would be nearer.
    let quad = |x: f32, h: [f32; 4]| [[x, h[0], 0.0], [x, h[1], 1.0], [x + 1.0, h[2], 1.0], [x + 1.0, h[3], 0.0]];
    let centroid = |q: [[f32; 3]; 4]| [0, 1, 2].map(|k| (q[0][k] + q[2][k]) / 2.0);
    let a = quad(0.0, [0.1, 0.95, 0.1, 0.95]);
    let b = quad(0.0, [0.5, 0.5, 0.5, 0.5]);
    let camera = [0.5, 3.0, 0.5];
    assert_eq!(sort_quads(camera, &[centroid(a), centroid(b)]), vec![0, 1]);
    // Equally far: in the buffer's order.
    assert_eq!(sort_quads([0.5, 3.0, 0.5], &[[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [1.0, 0.0, 0.0]]), vec![0, 1, 2]);
}
