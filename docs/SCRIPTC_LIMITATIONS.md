# scriptc compatibility

Aurora pins scriptc 0.4.16. The compiler emits native executables, relocatable program objects and WASI modules. Native objects require a matching scriptc runtime pack. Native FFI uses an explicit C ABI manifest. [CLI](https://scriptc.dev/docs/cli) · [Native objects](https://scriptc.dev/docs/native-objects) · [FFI](https://scriptc.dev/docs/ffi)

Aurora's native sample build uses the same TypeScript syntax parser as the editor. It erases supported authoring annotations in staged source while preserving source line and byte offsets, then feeds the result to `scriptc --tier static-only`. Inspector metadata and wire bindings are generated separately. User source files retain their decorators. Use explicit, statically typed APIs in native scripts. [Compiler limitations](https://scriptc.dev/docs/limitations)

Native sample entries call `launch` with one component. A custom entry can call `launchMany` with instances ordered to match the player's repeated `--script-node` options. Run `aurora-codegen --lower INPUT.ts OUTPUT.ts` to stage decorated sources for a custom build. [Aurora scripting](SCRIPTING.md)

## Observed static-only support

The sample builds and exported bundles compile the Aurora SDK itself with scriptc's static-only tier. That source exercises class inheritance, field initializers, getters, private fields, `Map`/`Set`, array iteration and higher-order methods, destructuring in `for...of`, template strings, optional chaining, nullish coalescing, and `Number.isInteger`. Those constructs have been verified through `hello`, `bird`, `scene`, `jumpy`, and a two-scene native export.

Game code is limited to statically resolvable modules. Dynamic `import()`, `eval`, `new Function`, Node built-ins, and runtime package resolution are not part of the exported program. Development launchers bundle through Bun or esbuild, so they can accept more syntax than the native build. A construct that only works in development must be removed or replaced before native export.

The compiler is invoked as `scriptc build ENTRY --tier static-only --ffi ffi.json`. Aurora stages project sources, erases its authoring decorators while preserving byte and line offsets, and registers every component factory at build time.

scriptc 0.4.16 does not lower the library type `Function`, including as a `Map` key. The SDK therefore keeps component constructors in two parallel arrays and compares them by identity instead of using `Map<Function, string>`. Passing a class constructor to `addComponent` still works, because the generated entry registers that same class value.

Verified by compiling and running standalone probes with `scriptc 0.4.16 --tier static-only`: `async`/`await`, `Promise`, `setTimeout`, and a real network `fetch()` all compile and run. Generators and regular expressions remain unverified.

Aurora's wire loop is frame-locked and its native receive blocks on the socket. Promise continuations do not run across a blocking native receive, so the loop yields to microtasks once between frames. This lets a promise callback observe results on a later simulation tick. It does not extend the host's 250 ms reply deadline: a lifecycle callback that returns a Promise is not awaited by the host, and network work that needs to gate gameplay must be resolved through a host-mediated protocol with an explicit completion event. No such bridge exists yet, so a network `fetch` inside a component callback is fire-and-forget from the player's point of view.
