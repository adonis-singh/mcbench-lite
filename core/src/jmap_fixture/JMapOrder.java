import java.util.*;

// Makes the scenarios of jmap_order.txt: sequences of puts, removes and
// replaces on a java.util.HashMap<Pos, Integer> and puts on a
// java.util.HashSet<Pos>, with the iteration order a JVM gave each.
//
// A Pos hashes and compares as net.minecraft.core.BlockPos does: Vec3 is the
// Vec3i (implements Comparable<Vec3>, hashCode (y + z * 31) * 31 + x) and Pos
// extends it declaring no interface, so HashMap.comparableClassFor is null
// for it. Mode S keys are all PosA, as the engine's are all BlockPos: two
// keys of one hash then order by System.identityHashCode. Mode M keys are
// PosA when z is even and PosB when it is odd: two keys of one hash and
// different classes order by class name, which is reproducible, and only
// two of one class and one hash are an identity tie.
//
// Run on the stock JDK it makes the fixture without the tie flag (a tie is
// then unknown); run with the java.util.HashMap that patch_hashmap.py makes
// (--patch-module java.base=...) it counts the identityHashCode orderings
// (HashMap.CNT[0]) per scenario and the tree-bin paths taken.
//
// Arguments: seed count out.txt [dump]
public class JMapOrder {
    static class Vec3 implements Comparable<Vec3> {
        final int x, y, z;

        Vec3(int x, int y, int z) {
            this.x = x;
            this.y = y;
            this.z = z;
        }

        @Override
        public int hashCode() {
            return (y + z * 31) * 31 + x;
        }

        @Override
        public boolean equals(Object o) {
            if (this == o) return true;
            return o instanceof Vec3 v && x == v.x && y == v.y && z == v.z;
        }

        @Override
        public int compareTo(Vec3 o) {
            if (y != o.y) return Integer.compare(y, o.y);
            if (z != o.z) return Integer.compare(z, o.z);
            return Integer.compare(x, o.x);
        }
    }

    static class PosA extends Vec3 {
        PosA(int x, int y, int z) {
            super(x, y, z);
        }
    }

    static class PosB extends Vec3 {
        PosB(int x, int y, int z) {
            super(x, y, z);
        }
    }

    static Vec3 key(boolean mixed, int x, int y, int z) {
        return mixed && (z & 1) != 0 ? new PosB(x, y, z) : new PosA(x, y, z);
    }

    static int spread(int x, int y, int z) {
        int h = (y + z * 31) * 31 + x;
        return h ^ (h >>> 16);
    }

    static SplittableRandom rng;
    static long[] cnt() {
        try {
            return (long[]) HashMap.class.getField("CNT").get(null);
        } catch (ReflectiveOperationException e) {
            return null;
        }
    }

    // Ops are {type, x, y, z}: 0 put, 1 remove, 2 replace.
    static List<int[]> ops;
    static LinkedHashSet<List<Integer>> present;

    static void put(int x, int y, int z) {
        ops.add(new int[] {0, x, y, z});
        present.add(List.of(x, y, z));
    }

    static void remove(int x, int y, int z) {
        ops.add(new int[] {1, x, y, z});
        present.remove(List.of(x, y, z));
    }

    static void replace(int x, int y, int z) {
        ops.add(new int[] {2, x, y, z});
    }

    // A removal of a key the map holds (mostly) or does not.
    static void removeSome() {
        if (!present.isEmpty() && rng.nextInt(10) != 0) {
            List<Integer> any = new ArrayList<>(present).get(rng.nextInt(present.size()));
            remove(any.get(0), any.get(1), any.get(2));
        } else {
            remove(rng.nextInt(7) - 3, rng.nextInt(5) - 2, rng.nextInt(7) - 3);
        }
    }

    static void replaceSome() {
        if (!present.isEmpty() && rng.nextInt(4) != 0) {
            List<Integer> any = new ArrayList<>(present).get(rng.nextInt(present.size()));
            replace(any.get(0), any.get(1), any.get(2));
        } else {
            replace(rng.nextInt(7) - 3, rng.nextInt(5) - 2, rng.nextInt(7) - 3);
        }
    }

    static void churn(int removes, int replaces) {
        for (int i = 0; i < removes; i++) removeSome();
        for (int i = 0; i < replaces; i++) replaceSome();
    }

    // Keys whose spread hash has the low `bits` bits of `residue`.
    static List<int[]> family(int bits, int residue, int want, int span, int cx, int cy, int cz) {
        LinkedHashSet<List<Integer>> seen = new LinkedHashSet<>();
        List<int[]> out = new ArrayList<>();
        int mask = (1 << bits) - 1;
        for (long tries = 0; out.size() < want && tries < 4_000_000L; tries++) {
            int x = cx + rng.nextInt(2 * span + 1) - span;
            int y = cy + rng.nextInt(2 * span + 1) - span;
            int z = cz + rng.nextInt(2 * span + 1) - span;
            if ((spread(x, y, z) & mask) == residue && seen.add(List.of(x, y, z))) out.add(new int[] {x, y, z});
        }
        return out;
    }

    static void shuffle(List<int[]> l) {
        for (int i = l.size() - 1; i > 0; i--) Collections.swap(l, i, rng.nextInt(i + 1));
    }

    static void randomBox() {
        int bx = rng.nextInt(2000) - 1000, by = rng.nextInt(200), bz = rng.nextInt(2000) - 1000;
        int n = 1 + rng.nextInt(40);
        for (int i = 0; i < n; i++) put(bx + rng.nextInt(7) - 3, by + rng.nextInt(5) - 2, bz + rng.nextInt(7) - 3);
        churn(rng.nextInt(6), rng.nextInt(6));
    }

    // A chunk's worth of positions, as the light engine's pending set gathers.
    static void chunk() {
        int cx = (rng.nextInt(4000) - 2000) * 16, cz = (rng.nextInt(4000) - 2000) * 16;
        int n = 30 + rng.nextInt(rng.nextBoolean() ? 400 : 2500);
        int ymin = rng.nextBoolean() ? -64 : rng.nextInt(200), yspan = 1 + rng.nextInt(384);
        for (int i = 0; i < n; i++) put(cx + rng.nextInt(16), ymin + rng.nextInt(yspan), cz + rng.nextInt(16));
    }

    // Many keys on the low bits of one bucket, padded and churned.
    static void familyScenario() {
        int bits = 6 + rng.nextInt(5);
        int residue = rng.nextInt(1 << bits);
        int f = 9 + rng.nextInt(rng.nextInt(4) == 0 ? 110 : 40);
        List<int[]> fam = family(bits, residue, f, 300, rng.nextInt(2000) - 1000, rng.nextInt(200), rng.nextInt(2000) - 1000);
        List<int[]> pad = new ArrayList<>();
        int p = rng.nextInt(4) == 0 ? rng.nextInt(500) : rng.nextInt(60);
        for (int i = 0; i < p; i++) pad.add(new int[] {rng.nextInt(600) - 300, rng.nextInt(200), rng.nextInt(600) - 300});
        List<int[]> all = new ArrayList<>(fam);
        all.addAll(pad);
        shuffle(all);
        boolean padFirst = rng.nextInt(3) == 0;
        if (padFirst) {
            for (int[] k : pad) put(k[0], k[1], k[2]);
            List<int[]> famCopy = new ArrayList<>(fam);
            shuffle(famCopy);
            all = famCopy;
        }
        int i = 0;
        for (int[] k : all) {
            put(k[0], k[1], k[2]);
            if (rng.nextInt(6) == 0 && i > 3) removeSome();
            if (rng.nextInt(12) == 0) replaceSome();
            i++;
        }
        // Remove down, perhaps past the untreeify threshold.
        int removes = rng.nextInt(present.size() + 1);
        for (int j = 0; j < removes; j++) {
            removeSome();
            if (rng.nextInt(5) == 0 && !fam.isEmpty()) {
                int[] k = fam.get(rng.nextInt(fam.size()));
                put(k[0], k[1], k[2]);
            }
        }
    }

    // Keys on a lattice with a power-of-two (or three times one) stride in
    // one dimension, and in a second one half the time.
    static void stride() {
        int d0 = rng.nextInt(3), d1 = (d0 + 1 + rng.nextInt(2)) % 3;
        boolean two = rng.nextBoolean();
        int s0 = (rng.nextInt(3) == 0 ? 3 : 1) << (3 + rng.nextInt(9));
        int s1 = (rng.nextInt(3) == 0 ? 3 : 1) << rng.nextInt(10);
        int n = 9 + rng.nextInt(rng.nextInt(3) == 0 ? 400 : 60);
        int[] base = {rng.nextInt(200) - 100, rng.nextInt(100), rng.nextInt(200) - 100};
        List<int[]> keys = new ArrayList<>();
        for (int i = 0; i < n; i++) {
            int[] k = base.clone();
            k[d0] += s0 * i;
            if (two) k[d1] += s1 * rng.nextInt(12);
            keys.add(k);
        }
        shuffle(keys);
        for (int[] k : keys) {
            put(k[0], k[1], k[2]);
            if (rng.nextInt(7) == 0) removeSome();
        }
        churn(rng.nextInt(present.size() / 2 + 1), rng.nextInt(5));
    }

    // A family and the keys whose full hash equals a member's: z + 1 with
    // y - 31 (a Pos of the other class in mode M) and, rarely, x + 31 with
    // y - 1 (the same class).
    static void ties() {
        int bits = 6 + rng.nextInt(4);
        int residue = rng.nextInt(1 << bits);
        int f = 9 + rng.nextInt(30);
        List<int[]> fam = family(bits, residue, f, 200, rng.nextInt(2000) - 1000, 40 + rng.nextInt(100), rng.nextInt(2000) - 1000);
        List<int[]> all = new ArrayList<>(fam);
        for (int[] k : fam) {
            int r = rng.nextInt(10);
            if (r < 3) all.add(new int[] {k[0], k[1] - 31, k[2] + 1});
            else if (r == 3) all.add(new int[] {k[0], k[1] - 62, k[2] + 2});
            else if (r == 4 && rng.nextInt(3) == 0) all.add(new int[] {k[0] + 31, k[1] - 1, k[2]});
        }
        shuffle(all);
        int pads = rng.nextInt(40);
        for (int i = 0; i < pads; i++) all.add(rng.nextInt(all.size() + 1), new int[] {rng.nextInt(600) - 300, rng.nextInt(200), rng.nextInt(600) - 300});
        for (int[] k : all) {
            put(k[0], k[1], k[2]);
            if (rng.nextInt(6) == 0) removeSome();
        }
        int removes = rng.nextInt(present.size() + 1);
        for (int j = 0; j < removes; j++) removeSome();
        churn(0, rng.nextInt(4));
    }

    static long fnv(String s) {
        long h = 0xcbf29ce484222325L;
        for (int i = 0; i < s.length(); i++) {
            h ^= s.charAt(i);
            h *= 0x100000001b3L;
        }
        return h;
    }

    public static void main(String[] args) throws Exception {
        long seed = Long.parseLong(args[0]);
        int count = Integer.parseInt(args[1]);
        boolean dump = args.length > 3;
        rng = new SplittableRandom(seed);
        long[] cnt = cnt();
        long[] total = new long[11];
        int scenarios = 0, ties = 0, classTies = 0;
        long[] byKind = new long[6];
        try (java.io.PrintWriter out = new java.io.PrintWriter(args[2])) {
            for (int s = 0; s < count; s++) {
                ops = new ArrayList<>();
                present = new LinkedHashSet<>();
                int kind = switch (rng.nextInt(20)) {
                    case 0, 1, 2 -> 0;
                    case 3 -> 1;
                    case 4, 5, 6, 7, 8, 9, 10, 11 -> 2;
                    case 12, 13, 14, 15 -> 3;
                    default -> 4;
                };
                byKind[kind]++;
                switch (kind) {
                    case 0 -> randomBox();
                    case 1 -> chunk();
                    case 2 -> familyScenario();
                    case 3 -> stride();
                    default -> ties();
                }
                boolean mixed = rng.nextBoolean();
                long[] before = cnt == null ? null : cnt.clone();
                HashMap<Vec3, Integer> map = new HashMap<>();
                HashSet<Vec3> set = new HashSet<>();
                int i = 0;
                for (int[] op : ops) {
                    Vec3 k = key(mixed, op[1], op[2], op[3]);
                    switch (op[0]) {
                        case 0 -> {
                            map.put(k, i);
                            set.add(k);
                        }
                        case 1 -> map.remove(k);
                        default -> map.replace(k, i);
                    }
                    i++;
                }
                boolean tie = false;
                if (cnt != null) {
                    for (int c = 0; c < 11; c++) total[c] += cnt[c] - before[c];
                    tie = cnt[0] != before[0];
                    if (!tie && cnt[10] != before[10]) classTies++;
                }
                StringBuilder m = new StringBuilder(), st = new StringBuilder();
                for (Map.Entry<Vec3, Integer> e : map.entrySet()) {
                    Vec3 v = e.getKey();
                    m.append(v.x).append(',').append(v.y).append(',').append(v.z).append(':').append(e.getValue()).append(';');
                }
                for (Vec3 v : set) st.append(v.x).append(',').append(v.y).append(',').append(v.z).append(';');
                StringBuilder line = new StringBuilder();
                line.append(mixed ? 'M' : 'S');
                for (int[] op : ops) line.append(' ').append("+-=".charAt(op[0])).append(op[1]).append(',').append(op[2]).append(',').append(op[3]);
                line.append(" | ").append(cnt == null ? "?" : tie ? "1" : "0").append(' ').append(map.size()).append(' ')
                    .append(Long.toHexString(fnv(m.toString()))).append(' ').append(set.size()).append(' ').append(Long.toHexString(fnv(st.toString())));
                if (dump) line.append(" | ").append(m).append(" | ").append(st);
                out.println(line);
                scenarios++;
                if (tie) ties++;
            }
        }
        System.err.println("scenarios " + scenarios + " identityTieScenarios " + ties + " classTieScenarios " + classTies + " kinds " + Arrays.toString(byKind));
        if (cnt != null) {
            System.err.println("identityTieBreaks " + total[0] + " treeify " + total[1] + " treeifyResize " + total[2] + " splitUntreeify " + total[3]
                + " splitTreeify " + total[4] + " removeTreeNode " + total[5] + " removeUntreeify " + total[6] + " treeInserts " + total[7]
                + " balanceDeletion " + total[8] + " putSearched " + total[9] + " tieBreakCalls " + total[10]);
        }
    }
}
