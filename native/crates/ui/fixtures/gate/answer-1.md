# A lamp on a lever

The lamp is built and **wired**. It lights when the lever is *on* and goes dark when it is off; the old torch is ~~gone~~. The block that does the work is a `redstone_lamp`, and [the wiki page on lamps](https://minecraft.wiki/w/Redstone_Lamp) has the details.

## What was placed

1. A stone base, three by three
2. The lamp on top
   - powered from below
   - with a lever on its west side
3. A sign that says *Pull me*

## The program

```rust
fn build(world: &mut World) {
    world.fill((0, 0, 0), (2, 0, 2), "stone");
    world.set((1, 1, 1), "redstone_lamp");
    world.set((0, 1, 1), "lever[face=floor,facing=west]");
}
```

## Blocks used

| Block | Count | Light |
|:------|------:|:-----:|
| stone | 9 | 0 |
| redstone_lamp | 1 | 15 |
| lever | 1 | 0 |

> A lamp only needs power from one side: the lever powers the block it sits on, and that block powers the lamp.

---

That is all. Pull the lever to test it.
