# TypePython v1 Implementation Notes

**Status:** informative  
**Scope:** rollout guidance, reference architecture ideas, checklists, product boundary, follow-on directions, and security considerations  
**Numbering note:** original appendix lettering is preserved for stable reference.

This document is informative. It does not create additional conformance requirements.

Appendices intentionally included here:

- Appendix F through Appendix K

---

## 21. Appendices

The following appendices are informative. They describe reference implementation strategy, product boundary, and host-environment considerations, but they do not add new conformance obligations.

### Appendix F (Informative): Implementation Milestones

| Milestone | Focus                                               |
| --------- | --------------------------------------------------- |
| 0         | Workspace, config, CLI skeleton                     |
| 1         | Scanner, parser, binding, source map                |
| 2         | Declaration typing, module graph, summaries         |
| 3         | Core typing rules, emit, verify                     |
| 4         | Flow narrowing, sealed tracking, publication checks |
| 5         | DX v1 features (`watch`, `lsp`, `migrate`)          |
| 6         | Experimental v1 features                            |

### Appendix G (Informative): Reference Architecture Notes

This appendix describes a reference architecture style that has worked well for the TypePython project. It does not require exact crate names or a specific internal layout.

A reference implementation will often separate at least these logical phases:

1. scanning and parsing
2. binding and symbol creation
3. declaration typing and module graph construction
4. semantic elaboration
5. body type checking and narrowing
6. emit and stub generation
7. cache invalidation and summary persistence

Keeping semantic elaboration distinct from purely syntactic lowering is often useful because it helps the compiler:

- expand `TypedDict` transforms only after the source declaration surface is known
- compute sealed-hierarchy closure from the defining module's declarations
- preserve precise source-to-emit mappings for diagnostics and DX tooling

### Appendix H (Informative): Implementation Checklist

A team planning a Core v1 implementation should be able to answer all of the following from the normative documents:

1. What files are inputs and outputs?
2. How are new syntax forms parsed and lowered?
3. What is the default behavior for untyped imports?
4. What types exist beyond ordinary Python typing?
5. How do generics, overloads, and interfaces work?
6. How are diagnostics shaped and located?
7. How are `.pyi` files generated?
8. How does incremental invalidation work?
9. What parts of Python are explicitly out of scope or unsafe?
10. What behavior is required from the Core CLI, and which tools are DX v1 rather than Core v1?

A missing answer is a sign that additional design or specification work is needed; it is not itself a conformance ruling.

### Appendix I (Informative): v1 Product Boundary

A reasonable first Core v1 product can be considered complete when a user can:

1. write a package entirely in `.tpy`
2. import standard Python libraries and typed third-party libraries
3. get useful diagnostics during development
4. run `typepython build`
5. ship generated `.py` and `.pyi` without requiring TypePython at runtime
6. publish the generated package through normal Python packaging with `py.typed`
7. verify that the exported API surface is typed and consumable by downstream tools

DX v1 success additionally means that a team can opt into watch mode, LSP support, and migration reporting without changing Core v1 semantics.

That boundary, and not feature parity with all of TypeScript, defines v1 success.

### Appendix J (Informative): Priority Follow-On Directions

The following are the highest-value follow-on directions after Core v1 stabilizes. They are intentionally not part of Core v1 conformance.

1. **Diagnostic suppression, baselines, and per-rule severity control.**
   Large migrations need targeted escape hatches in addition to project profiles. A future version should standardize inline suppression syntax, baseline files, and per-rule severity configuration so teams can adopt TypePython incrementally without losing deterministic diagnostics.

2. **`TYPE_CHECKING`, version guards, and platform guards.**
   Real-world packages rely on conditional imports and environment-sensitive declaration surfaces. A future version should define how `if TYPE_CHECKING:`, Python-version checks, and platform predicates influence binding, public summaries, and emitted stubs.

3. **Non-callable decorator replacement semantics.**
   Core v1 covers deterministic callable-to-callable decorator transforms. A future version should define how decorators that replace a declaration with a non-callable object, a rewritten class surface, or runtime-computed metadata participate in checking and `.pyi` emission without reintroducing plugin-specific behavior.

4. **A first-class record or shape model.**
   This is the prerequisite for extending utility transforms beyond `TypedDict`. Before transforms such as `Partial` or `Pick` are allowed on classes, interfaces, or protocols, the language needs an explicit notion of field presence, field mutability, and constructor participation that is not overloaded onto nominal classes.

5. **Native modern emit for newer targets.**
   Once the target range extends beyond 3.12, the emitter can consider `type` statements and newer native syntax more aggressively. That work should remain target-aware and deterministic: modern emit must be a controlled alternate projection of the same declaration surface, not a semantic fork.

6. **External checker compatibility verification.**
   Long-term, `verify` should be able to validate that emitted surfaces are consumable not just by TypePython but also by major external type checkers. This should build on the existing authoritative `.pyi` model rather than replacing it with TypePython-specific declaration artifacts.

7. **Testing helpers such as `reveal_type`, `assert_type`, and `assert_never`.**
   These are high-value developer tools but do not need to block Core v1. They should integrate cleanly with the diagnostic system and be specified as checker-only utilities with no runtime obligations beyond what standard Python already provides.

8. **Workspace or project references.**
   Useful for very large repos, but not required for the first shippable v1. A later version should define dependency boundaries, reusable summaries, and rebuild ordering between projects before standardizing a multi-project graph.

### Appendix K (Informative): Security and Host Environment Considerations

TypePython interacts with the host filesystem, Python interpreters, installed packages, and published build artifacts. Implementations SHOULD document which operations inspect or execute host-controlled content and under what trust assumptions.

At minimum:

- implementations SHOULD distinguish purely static operations from operations that invoke a Python interpreter or inspect runtime import surfaces
- implementations MUST NOT claim that publication or interoperability checks are hermetic unless every interpreter, package source, and artifact input is controlled and declared
- behavior that depends on host interpreter version, filesystem semantics, locale, or platform path rules is host-defined unless a normative document states otherwise
- verification workflows that inspect built wheels, source distributions, or installed packages SHOULD surface enough context for users to identify which external artifact influenced the result
- implementations SHOULD document whether symlink resolution, case-folding filesystems, and interpreter path discovery can affect build or verification outcomes

Security-sensitive deployment environments MAY require implementations to disable runtime-assisted verification features, restrict interpreter discovery, or run verification in a sandboxed environment.
