# Strategic Todo Implementation Ledger

This ledger makes the `docs/strategic-todo.md` completion history reviewable. In this roadmap, a
"thing" is a cohesive implementation unit: one behavior/design/test/documentation slice that can be
reviewed and reverted independently. Nested checklist bullets and acceptance criteria are evidence
inside those units rather than separate empty commits.

## Commit discipline

- Implementation commits carry the code or executable tests for a roadmap unit.
- Documentation commits mark the matching roadmap section and update user/spec evidence after the
  implementation exists.
- Generated evidence commits keep generated reports reproducible with their generator changes.
- No empty commits are used as checklist placeholders.

## Roadmap unit mapping

| Roadmap area | Cohesive implementation commits |
| --- | --- |
| Framework shape and decorator transform system | `150a718` `c9f8bc4` `208ffbf` `c82004a` `2f70da3` `f571a2b` `b960356` `18c421f` `03ef3e4` `cebe4a0` `04a4880` `7d729ac` `2e468fe` |
| First-class record and shape model | `da385fa` `44de52b` `ec93c5c` `c8664b3` `2e7ba13` |
| Trustworthiness and test infrastructure | `2018dee` `09270c7` `1ecedbc` `91c79b1` `2debf13` `d94da67` `f903c43` `221289b` `89fc838` |
| Checker portability and compatibility audit | `4b31b14` `5232593` `5d66531` `99c46f0` `ca096a3` `f250525` |
| Python annotation runtime compatibility | `0c76ad4` `4d6749c` `c69940b` |
| Public API surface diff | `dd4d5b4` `3552fd9` `6cecab1` `6f2b692` `18050fa` `39637ed` `fba46f5` |
| Typed dependency and stub health supply chain | `1c11aa2` `634bfce` `2632e44` |
| Pydantic and FastAPI integration | `34dca48` `827c76f` `9e9eff0` `13fcfaf` `22649a8` `84c471d` `3d58818` |
| Migration and publication workflow | `0f84fbc` `8834bc2` `68ede5b` `4cbb991` `efd1939` `b002cb4` |
| LSP migration UX | `58ad13e` `6b5d5b3` `70cd8f8` `35dc659` `c5a0a90` `ea8ea62` `763fa55` `b4cc250` `f68aac8` |
| Framework adapter authoring and validation | `0fc1a5c` `645a973` `c1e3ea3` `5c5a8a7` `14920da` `8e98ffa` `10af63c` `85fc10e` `d30ad56` |
| Rust-like result/effect pattern and deferred exception strategy | `8f46f55` |
| Boundary validator generation | `f06d896` `9324625` `9d68484` `9be9a2a` `91c8e64` `1abbfb5` `99df0a1` |
| Boundary validator docs and evidence | `285f793` `d770035` `a449579` `1a777ec` `f903c43` `221289b` `89fc838` |
| Must-use, must-await, and must-close diagnostics | `8769bb7` `fc144aa` `c34aef8` |
| Sync/async dual emit | `a582573` `be0a170` `43150a7` `29987dc` `3d869aa` `a5ee11c` `6668b98` |
| Deferred roadmap items explicitly scoped out of implementation | `526456e` `ca26359` `7b28bb8` |
| Roadmap status reconciliation and final closure | `dbd5e8f` `4b49978` `d7aca28` `e61096b` `b58f5f1` `cf42435` `0387947` |

## Verification evidence

The latest closure pass is validated by the tracked commands in `docs/conformance-report.md` under
"Current Validation Evidence", plus the feature-specific evidence rows for LSP, migration reports,
and selected boundary validators.
