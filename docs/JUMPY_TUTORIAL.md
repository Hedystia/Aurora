# Build Jumpy Bird in the editor

This walkthrough creates the Jumpy Bird sample through the Aurora editor UI.
The finished project is in `scripts/game-template/jumpy`; keep it open beside
the tutorial as a reference. The steps use the sample artwork and sounds so the
only thing you author is the scene graph, prefab, animation, and component
binding.

The tutorial takes about 20 minutes. Save the scene with **Ctrl+S** after each
major section.

## 1. Create a 2D project

1. Run `cargo run --locked --bin aurora-editor`.
2. On the Projects screen, choose **New Project**.
3. Enter `JumpyTutorial` as the **Project Name**, choose a parent folder, and
   select **2D** as the starting scene type.
4. Choose **Create & Edit**. Aurora creates `assets/`, `main.scene`, and the
   TypeScript SDK files.

Do not choose the **Jumpy Bird** starter for this walkthrough; that starter
creates the completed sample before the first step.

## 2. Import the sample assets

Open `scripts/game-template/jumpy/assets` in the system file browser and drag
these files into the editor's **FileSystem** panel:

- `sky.png`, `ground.png`, `jumpy.png`, `wing.png`, `ready.png`, `over.png`
- `pillar.png`, `score.png`, `best.png`, and `0.png` through `9.png`
- `swoosh.wav`, `point.wav`, `hit.wav`, `die.wav`

Drag the PNGs and WAVs into the `assets` folder, not onto the scene. Once the
files are imported, the FileSystem asset list shows the project-relative paths
used by the nodes below.

Now set the design resolution. Choose **Project → Display Settings**, set
**Width** to `640` and **Height** to `384`, select **Contain**, and confirm.
Jumpy Bird is authored in a 640×384 pixel frame.

## 3. Build the scene tree

Use **Node Library → Built-in** and drag a **Sprite** tile into the **Scene
Tree**, or select the Canvas and click the tile. Rename each new node with
**F2**. Drop the matching PNG from FileSystem onto the node to set its sprite.
Set **Position**, **Size**, and **Draw Order** in the Inspector.

Create these nodes under the Canvas, in this order. The order matters because
the script refers to node indices.

| Index | Name | Parent | Position | Size | Draw order | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | Canvas | — | — | — | — | Scene root created with the project |
| 1 | Night sky | Canvas | 0, 0 | 640, 384 | 0 | `assets/sky.png` |
| 2 | Ground 1 | Canvas | -320, -184 | 256, 24 | 1 | `assets/ground.png`, anchor `0, 0.5` |
| 3 | Ground 2 | Canvas | -64, -184 | 256, 24 | 1 | `assets/ground.png`, anchor `0, 0.5` |
| 4 | Ground 3 | Canvas | 192, -184 | 256, 24 | 1 | `assets/ground.png`, anchor `0, 0.5` |
| 5 | Jumpy | Canvas | -145, 0 | 36, 30 | 2 | `assets/jumpy.png` |
| 6 | Wing | Jumpy | -6, -3 | 15, 11 | 2 | `assets/wing.png`; child of Jumpy |
| 7 | Start screen | Canvas | 0, 48 | 352, 100 | 20 | `assets/ready.png` |
| 8 | Game over screen | Canvas | 0, 48 | 352, 100 | 20 | `assets/over.png`, **Active** off |
| 9 | Score digit 1 | Canvas | -250, 142 | 23, 28 | 10 | `assets/0.png` |
| 10 | Score digit 2 | Canvas | -227, 142 | 23, 28 | 10 | `assets/0.png` |
| 11 | Score digit 3 | Canvas | -204, 142 | 23, 28 | 10 | `assets/0.png` |
| 12 | Best digit 1 | Canvas | 203, 142 | 23, 28 | 10 | `assets/0.png` |
| 13 | Best digit 2 | Canvas | 226, 142 | 23, 28 | 10 | `assets/0.png` |
| 14 | Best digit 3 | Canvas | 249, 142 | 23, 28 | 10 | `assets/0.png` |
| 15 | Score label | Canvas | -227, 169 | 75, 18 | 10 | `assets/score.png` |
| 16 | Best label | Canvas | 226, 169 | 75, 18 | 10 | `assets/best.png` |
| 17 | Ground sensor | Canvas | 0, -192 | — | — | Empty node; sensor added in step 6 |

For the three ground segments, set **Anchor** to `0, 0.5` so the script can
scroll them from their left edge. Leave the other sprites at the default
`0.5, 0.5` anchor. **Draw order** is the **Draw Order** field in the
Inspector's sprite appearance section. Higher values draw on top.

The scene should now show the sky, three ground pieces, the bird with a wing,
the two overlays, and the six score digits.

## 4. Create the pillar prefab

1. Add an **Empty Node** under Canvas and rename it `Pillar pair`. Set its
   position to `355, 0` and turn **Active** off.
2. Add a **Sprite** child named `Pillar upper`, set its position to `0, 170`,
   size to `52, 230`, and assign `assets/pillar.png`.
3. Add a second Sprite child named `Pillar lower`, set its position to
   `0, -170`, size to `52, 230`, and assign the same `assets/pillar.png`.
4. Select `Pillar pair`, open its context menu in the Scene Tree, and choose
   **Save as Prefab**. Save it as `pillars.prefab` in the project root.

The prefab root is inactive so a freshly placed instance starts hidden. The
script activates and recycles instances through `NodePool`.

## 5. Place four pillar instances

Drag `pillars.prefab` from **Node Library → Prefabs** into the Scene Tree four
times, under Canvas, after `Ground sensor`. Leave all four instances at their
prefab position and do not reorder them.

Each instance adds three scene indices:

| Instance | Root | Upper child | Lower child |
| --- | --- | --- | --- |
| First | 18 | 19 | 20 |
| Second | 21 | 22 | 23 |
| Third | 24 | 25 | 26 |
| Fourth | 27 | 28 | 29 |

The script's pool is `new NodePool([18, 21, 24, 27])`, so those four roots must
keep these indices. If the scene tree gains or loses a node before them, update
the pool or rebuild the instances in this order.

## 6. Add contact sensors

Select each node and enable **Gameplay → Contact Sensor**. Set the **Sensor
Size** as follows:

| Node | Sensor size |
| --- | --- |
| Jumpy (5) | 24, 22 |
| Ground sensor (17) | 640, 26 |
| Every `Pillar upper` and `Pillar lower` child | 52, 230 |

For the pillar instances, expand each prefab in the Scene Tree and select both
children. The sample uses sensors on indices 19, 20, 22, 23, 25, 26, 28, and
29. The bird's contact with any sensor ends the run.

The **Ground sensor** is an empty node with no sprite. Its large sensor gives
the bird a floor even though the visual ground scrolls beneath it.

## 7. Register the score frames

The score and best-score rows use sprite-frame banks rather than separate
objects per number. Select the Canvas, open **Gameplay → Sprite Frames**, and
add a frame bank for each digit node:

- Score digit 1, Score digit 2, Score digit 3
- Best digit 1, Best digit 2, Best digit 3

For each of the six banks, drag `0.png`, `1.png`, … `9.png` into the bank in
that order. The script calls `scene.frame(start, digit)`, where frame index `0`
is `0.png` and index `9` is `9.png`.

You can select the six digit nodes together and add the same ordered bank to
each one if the Inspector applies the drop to the whole selection; otherwise
repeat the ten-file drop per node.

## 8. Register the sound bank

With Canvas selected, open **Gameplay → Scene Audio Clips** and drag the sound
files in this exact order:

1. `swoosh.wav` — flap and run start
2. `point.wav` — score
3. `hit.wav` — collision
4. `die.wav` — game-over cue

The script uses the indices `0` through `3` with `scene.sound(index)`. Dropping
the files in a different order changes which cue plays.

## 9. Animate the wing

1. Select `Wing`.
2. Open the **Animation** panel. If prompted, choose **Add Animation
   Component**.
3. Create a clip named `Wingbeat`.
4. Set **Sample** to `60`, **Duration** to `10`, **Speed** to `1`, and the wrap
   mode to **Ping-pong**.
5. Add the `Wing` node's **Scale Y** property, then add two keys:
   - frame `0`: value `1`
   - frame `10`: value `0.36`
6. Set both keys to **Cubic** interpolation. Enable **Autoplay** so the clip
   starts when the scene opens.

Press the play transport in the Animation panel to preview the flap. The wing
is a child of Jumpy, so its local scale animation follows the bird without
changing the bird's own scale.

## 10. Create the Jumpy component

1. In FileSystem, choose **New Asset → New TypeScript Component** and name it
   `Jumpy`.
2. Replace the generated file with the sample component from
   [`scripts/game-template/jumpy/assets/Jumpy.ts`](../scripts/game-template/jumpy/assets/Jumpy.ts).
3. Save the file.

The component declares six `@property({ type: NodeRef })` fields, owns the
gravity/flap state, scrolls the ground, recycles pillar instances, updates the
digit banks, plays sound cues, and persists the best score. Its `NodePool`
contains the four prefab roots listed in step 5.

## 11. Attach and bind the component

1. Select `Jumpy` in the Scene Tree.
2. Choose **Inspector → Add Component → Jumpy**. You can also drag
   `assets/Jumpy.ts` onto the node.
3. Fill the six node-reference fields by dragging nodes from the Scene Tree
   into each slot:

| Field | Node |
| --- | --- |
| `birdNode` | Jumpy |
| `startScreen` | Start screen |
| `endScreen` | Game over screen |
| `firstGround` | Ground 1 |
| `scoreDigits` | Score digit 1 |
| `bestDigits` | Best digit 1 |

The fields are contiguous by design: the script uses `firstGround + i`,
`scoreDigits + i`, and `bestDigits + i` for the three ground segments and the
three digits in each row. Keep the six digit nodes in the order shown in the
scene table.

Save the scene again.

## 12. Play and verify

Press **F5** or choose **▶ Play**. The Game view should show the ready overlay.
Press **Space** or **Up**, click the Game view, or tap the window to start.

Verify these behaviours:

- The bird rises on each flap and falls under gravity.
- The ground scrolls continuously from left to right.
- Pillars approach, the score increments when a pair passes the bird, and a
  collision shows the game-over overlay.
- Pressing again after game over clears the pillars, resets the score, and
  starts a new run.
- The best score updates and survives a Play/Stop cycle in the editor cache.
- The wing keeps folding and unfolding through the ping-pong clip.

Press **F8** to stop the embedded player.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| The bird does not appear over the background | `Jumpy`'s sprite and draw order are set, and its parent is Canvas. |
| Nothing scrolls | The three ground nodes are indices 2–4 in order and `firstGround` points at Ground 1. |
| Pillars never appear | Four prefab instances exist under Canvas and `NodePool` still uses `18, 21, 24, 27`. |
| The score shows the wrong digits | Each digit bank lists `0.png` through `9.png`, in order, and `scoreDigits` points at Score digit 1. |
| The game ends immediately | The bird starts overlapping a sensor, or the Ground sensor is placed above the bird. The ground sensor belongs at `0, -192` with size `640, 26`. |
| No sound plays | The four clips are in the Canvas sound bank in the documented order. Preview sound also requires a working audio output device. |
| The wing does not animate | The Wingbeat clip autoplays and targets `Wing`'s Scale Y channel. |

## Reference and golden replay

The committed project at `scripts/game-template/jumpy` is the reference
assembly. Its scene file records the same nodes, prefab instances, frame
banks, sound bank, and animation clip described above.

For the deterministic replay, build the static-only script and run the golden
test:

```bash
tools/build-script.sh native jumpy
tools/golden_test.sh
```

The golden test plays a fixed 600-frame input trace and compares the serialized
scene state with the committed snapshot. Do not use `--bless` unless you have
reviewed an intentional gameplay change; the tutorial should reproduce the
committed hash without regenerating the baseline.
