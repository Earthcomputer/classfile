# classfile

Yet another Rust classfile parsing library. Yes, another one. No this is not a commitment, it's a bit of fun for now. I've found that the other libraries don't quite meet my needs

The design of this API is heavily inspired by the ASM Java library.

Goals:
- Be able to read, transform, and write class files.
- Low-copy reading of class files.
- Lazy reading of class files, by calling methods directly on `ClassReader` and `ClassReader.events()`.
- Use the event pattern to stream events from a reader, possibly through transformers, and into a writer without necessarily needing to know which events exist in advance
  - Unfortunately the provider traits are quite nasty until we have associated type defaults. I couldn't think of a better way to do this, if you know of one please let me know. For now, additions to these traits should not be considered breaking.
- Be able to handle *any* class file, and access any data in it, include class files with Java strings unrepresentable as UTF-8 in a way that remainds ergonomic.
