# StarDust - Outline

StarDust is a library for data layout and external function management. Specifically, StarDust aims to achieve the following two tasks:

- Explicit declaration on data layout, including:
  - How is the data stored in memory?
  - How is the data retrieved?
  - How is the data represented?
- Importing external function, including:
  - Code from other language, e.g., C, Python, Haskell, LLVM, LEAN directly without translating them into Rust syntax beforehand.
  - Provide a platform to pass data in a uniform way across functions in various language, i.e. no pre-processing/post-processing is needed between them.

The "Explicit declaration" serves as a THIN layer from StarDust to OS kernel, and the "Import external function" serves as a THIN layer from StarDust to others. 

We use Rust to implement StarDust in this development.

The executable contract is [waves.md](waves.md) (do not implement from this outline). Done waves: [waves-done.md](waves-done.md).

---

StarDust also supports OOP structure with advanced data and function management. User may set how the object is stored or use multi-language during the implementation of one single class.