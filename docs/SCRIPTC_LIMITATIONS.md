# scriptc compatibility

Aurora pins scriptc 0.4.16. The compiler emits native executables, relocatable program objects and WASI modules. Native objects require a matching scriptc runtime pack. Native FFI uses an explicit C ABI manifest. [CLI](https://scriptc.dev/docs/cli) · [Native objects](https://scriptc.dev/docs/native-objects) · [FFI](https://scriptc.dev/docs/ffi)

The static engine SDK will use explicitly typed objects and registration metadata. The compiler supports some boxed dynamic operations and decorators but rejects legacy property decorators, with restrictions on reflective mutation and computed members. Use explicit registration in compiled sources and lower supported decorator syntax to equivalent metadata during the development build. Verify release examples with `--tier static-only`. [Compiler limitations](https://scriptc.dev/docs/limitations)
