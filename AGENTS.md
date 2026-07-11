# AGENTS.md

## Build and test

- Run tests in release mode

## Engineering rules

- Don't write code only to let tests pass, implement correct semantics and correct algorithms.
- Do not optimize for minimal patches if the current structure is wrong.
- Large refactors are acceptable when they improve correctness.
- Do not add fixture-specific hacks.
- Keep code concise: if one line is clearer than two, use one line.
- Prefer simple, direct, high-performance code.
- Avoid redundant code, unnecessary abstractions, repeated work, and unused paths.
- If unused or unnecessary code is discovered while working, remove or simplify it.
- Do not keep compatibility shims unless they preserve required semantics.
- Everytime you finish the change, check if the existing code is well organized and maintenable and readable, refector when needed
- Everytime you fix a bug or implement a new feature, document the code and mention why you make this change, mention if the fix/implementation did fix the real issue or just a temp solution
- No minimal change, no need to capatible with old code, code must designed in long term develop perspective (maintainable, readable, organized, clear code logic)
- one file can't contains too much code, split them
- When optimize performance, you must write benchmark for this secenario with criterion framework.

## Testing policy

- Every bug fix must include a regression test.
- Every new feature must include semantic tests and edge cases.
- If a suspected bug is found while coding, add a focused test first, then fix it.
- Temporary diagnostic tests are allowed but must be removed before finishing unless they become permanent regression tests.
- All tests must run in single thread

## Debug

- use renderer and `render_with_options` to capture debug info, you may need to write a temp rust function yourself to debug

## Completion policy

After each feature or bug fix:

1. Run focused release tests.
2. Run broader relevant release tests.
3. Review the code for new bugs or semantic gaps.
4. If a new likely bug is found, add a test, fix it, and repeat.
5. Run formatting and lint checks:
   - `cargo fmt`
   - `cargo clippy --release`
6. If it is svg related change, run svg full tests to make sure there is no regression
7. In svg, resvg reference png are not 100% correct, small pixels difference is acceptable, ask developer to confirm before consider the change failed.
8. Every change related to rendering, must run full svg tests and examples, if there are diff in pngs, make sure the change make sense and reviewed by human
9. Review your changes, do they make sense
10. After benchmark running, individual performance regression must not greater than 2%