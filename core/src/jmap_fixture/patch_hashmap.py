# Copies JDK 25's java.util.HashMap and adds counters (HashMap.CNT) for the
# tree-bin paths; the logic is untouched. Usage: patch_hashmap.py HashMap.java out.java
import sys

src = open(sys.argv[1]).read()


def sub(old, new):
    global src
    assert src.count(old) == 1, old
    src = src.replace(old, new)


sub("static final int MIN_TREEIFY_CAPACITY = 64;", "static final int MIN_TREEIFY_CAPACITY = 64;\n    public static final long[] CNT = new long[11];")
sub("d = (System.identityHashCode(a) <= System.identityHashCode(b) ?", "d = (CNT[0]++ >= 0 && System.identityHashCode(a) <= System.identityHashCode(b) ?")
sub("if (tab == null || (n = tab.length) < MIN_TREEIFY_CAPACITY)\n            resize();", "if (tab == null || (n = tab.length) < MIN_TREEIFY_CAPACITY)\n            { CNT[2]++; resize(); }")
sub("if ((tab[index] = hd) != null)\n                hd.treeify(tab);", "if ((tab[index] = hd) != null)\n                { CNT[1]++; hd.treeify(tab); }")
sub("tab[index] = loHead.untreeify(map);", "{ CNT[3]++; tab[index] = loHead.untreeify(map); }")
sub("tab[index + bit] = hiHead.untreeify(map);", "{ CNT[3]++; tab[index + bit] = hiHead.untreeify(map); }")
sub("loHead.treeify(tab);", "{ CNT[4]++; loHead.treeify(tab); }")
sub("hiHead.treeify(tab);", "{ CNT[4]++; hiHead.treeify(tab); }")
sub("boolean movable) {\n            int n;\n            if (tab == null || (n = tab.length) == 0)\n                return;", "boolean movable) {\n            int n; CNT[5]++;\n            if (tab == null || (n = tab.length) == 0)\n                return;")
sub("tab[index] = first.untreeify(map);  // too small", "CNT[6]++; tab[index] = first.untreeify(map);  // too small")
sub("TreeNode<K,V> r = p.red ? root : balanceDeletion(root, replacement);", "if (!p.red) CNT[8]++;\n            TreeNode<K,V> r = p.red ? root : balanceDeletion(root, replacement);")
sub("xp.next = x;", "xp.next = x; CNT[7]++;")
sub("static int tieBreakOrder(Object a, Object b) {\n            int d;", "static int tieBreakOrder(Object a, Object b) {\n            int d; CNT[10]++;")
sub("searched = true;", "searched = true; CNT[9]++;")
open(sys.argv[2], "w").write(src)
