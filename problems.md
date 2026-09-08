# Problems

## Open

### MSVC shared-lib fixtures

The in-tree I1 C and I4 LLVM fixtures compile with gcc/clang (`-shared`). MSVC/`link.exe` is detected and rejected; there is no MSVC artifact path.

**Solution:**
