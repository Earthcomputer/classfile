# classfile

Yet another Rust classfile parsing library. Yes, another one. No this is not a commitment, it's a bit of fun for now. I've found that the other libraries don't quite meet my needs

Goals:
- Be able to handle *any* class file, and access any data in it, include class files with Java strings unrepresentable as UTF-8 in a way that remainds ergonomic.
- Be able to read, transform, and write class files
- Use the visitor pattern to remain zero-copy where possible.
