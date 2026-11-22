# Jumpy Bird

Jumpy Bird is an original 2D sample set against an aurora. Open `scripts/game-template/jumpy` as an Aurora project and press Play. Space, Up, and touch start a run, flap, and retry. In Game view, click to send a touch or focus the view for Space.

The TypeScript component owns gravity, flaps, scrolling, pillar placement and recycling, scoring, sound cues, and the ready and retry states. Three ground segments scroll from their left anchors. Four instances of the pillar prefab supply the reusable node slots. The scene also contains a ping-pong wing clip, contact areas, and font-rasterized score digits backed by sprite frames. The score persists in the editor's project cache. A standalone run can opt into persistence with `--profile PATH`.

## Native player

```bash
cargo build --locked --workspace
script="$(tools/build-script.sh native jumpy)"
target/debug/aurora-player \
  --scene scripts/game-template/jumpy/main.aurora-scene \
  --script-node 5 --script "$script" --profile .aurora/jumpy-best
```

The scene and prefab are editable in Aurora. The checked-in initial scene was assembled as a scene document, and the TypeScript component was authored as source. The Gameplay inspector exposes contact areas and sprite-frame banks. Select the root node to edit the scene sound bank. The game uses the listed frame and sound indices.

## Golden test

Run `tools/golden_test.sh` to build the static-only game script and play a fixed input trace for exactly 600 simulation frames. The trace starts with touch, then uses Space, scores, and reaches game over. The runner uses a temporary asset cache, an isolated silent mixer, and no saved high score. It checks the result before comparing the SHA-256 of the serialized entity state with the committed Insta snapshot.

For an intentional change, review a headless run and regenerate the baseline with `tools/golden_test.sh --bless`. Run the regular check twice after blessing.
