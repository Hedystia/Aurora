# Authored scenes and prefabs

`.aurora-scene` and `.aurora-prefab` are UTF-8 JSON documents with a numeric `version` of 1. Nodes appear in parent-first order and use zero-based indices. Omit a node's `parent` for a root. Prefabs have exactly one root. A sprite describes a PNG path and a local width and height. Paths resolve relative to the document that declares them. Omitting the whole transform gives the identity pose. Authored positions are in pixels, Y points up, rotation is in radians, and scale is unitless.

A scene can append prefab instances. Each instance can attach to an inline scene node. Track targets use the inline nodes followed by the nodes from each instance in authored order. Runtime handles are opaque and are reallocated after a scene change or pool checkout.

Use `SceneFile::capture` or `PrefabFile::capture` with a callback that resolves texture IDs to document paths, then call `save` to write the file. To edit an existing scene without losing its clips or prefab references, use `SceneFile::load`, edit the document, and save it. See the [gallery scene](../scripts/game-template/scenes/gallery.aurora-scene) and [marker prefab](../scripts/game-template/scenes/marker.aurora-prefab) for examples.

## Playback and pooling

A clip describes its sample rate, duration in samples, speed, wrap mode, property tracks, and optional markers. Each track targets one local transform channel and uses strictly increasing keys. Cubic interpolation eases between keys with zero tangents. An autoplay index selects the clip that starts on scene entry. The director can play another clip by index and pause or resume simulation without resetting playback.

A Rust `NodePool` borrows the owning scene and prefab. `get` checks out a reset subtree, `reset` restores an active instance's authored pose and sprites, and `put` detaches it. The pool rejects a return if the prefab instance's topology changed. A returned instance's handles are stale even if a later checkout reuses its ECS allocations. TypeScript currently controls the director and receives animation events. Prefab instantiation and pooling are available through authored scene references and the Rust APIs.
