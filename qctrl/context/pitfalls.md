# RCON Password Quoting Regression

**Problem**: RCON commands were failing with "Bad rcon_password" even though the password was correct.

**Root Cause**: Commit `00284c4e9` accidentally removed quotes around the password in the RCON command format:
- **Working**: `rcon "ace123" status`
- **Broken**: `rcon ace123 status`

**Why it matters**: The Quake 2 server's `Cmd_TokenizeString` expects the password to be quoted so `Cmd_Argv(1)` parses correctly. Without quotes, the server rejects the password.

**Fix**: Always use `format!("rcon \"{}\" {}", password, command)` in both UDP and TCP implementations.

**Prevention**: Added regression tests `test_rcon_command_format` and `test_rcon_password_no_quotes_in_format` in `crates/rcon/src/lib.rs`.

## Sources
- `crates/rcon/src/lib.rs:75` (RCON command construction)
- Commit `006635d16` (correct implementation)
- Commit `00284c4e9` (regression that removed quotes)
- Commit `50dbfe768` (fix with tests)

---

# Build Verification Before Committing

**Problem**: Claimed a feature was "done" without verifying the build, committing broken code.

**Rule**: NEVER claim "done" or commit unless:
1. `just fe-build` passes (TypeScript + Vite)
2. `just be-all` passes (Rust fmt, clippy, build)
3. Tests pass (`just be-test`)

**Prevention**: Always run the appropriate build command BEFORE committing.

---

# TypeScript Type Safety with Sets

**Problem**: `new Set(filteredMaps.map(...))` infers `Set<unknown>` instead of `Set<string>`.

**Fix**: Always use explicit type annotation: `new Set<string>(...)`

**Sources**:
- `frontend/src/components/AddMapDialog.tsx:194`

---

# JSX Structure Matching

**Problem**: Mismatched opening/closing tags in JSX cause cryptic TypeScript errors.

**Rule**: Always verify JSX structure:
- Each `<div>` has matching `</div>`
- Props are properly closed
- Nested structures are balanced

**Sources**:
- `frontend/src/components/AddMapDialog.tsx` (button group structure)
