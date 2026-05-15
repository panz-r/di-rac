# Eclipse/OSGi Plugin Architecture — Condensed

**Declarative XML-based extension points (2001–present, 1500+ plugins).**

## Core Concept: Extension Points vs Extensions

| Role | Declares | Schema | Lifecycle |
|------|----------|--------|-----------|
| **Extension Point** | Host plugin — slot definition | `.exsd` XML Schema | Stable contract, versioned |
| **Extension** | Extender plugin — fills slot | XML in `plugin.xml` | Discovered at startup, loaded lazily |

## 5 Fundamental Patterns

1. **Formal Schema per Hook Point** — `.exsd` defines valid XML structure, enables editor support, validation, doc generation
2. **Configuration Element** — Universal parameter object (`IConfigurationElement`): `getAttribute()`, `getChildren()`, `createExecutableExtension("class")`
3. **Nested Loop Processing** — `for extension in extensions: for member in extension: process(member)`
4. **Lazy Instantiation via Virtual Proxy** — Don't create callback objects at activation. Create lightweight proxies that defer full instantiation until first use
5. **Interface Contract → Callback Implementation** — Extension point defines Java interface. Plugin provides concrete class. Host calls through interface only

## Key Lesson for Your DSL

**Declarative hook manifests enable discovery without code loading.** Eclipse's XML-based extension points let the system know what hooks exist without loading any plugin code. Your DSL should support a manifest that declares "I hook into `before_tool_call` for `bash`" — discoverable without executing any Wasm/Rust/Go code.
