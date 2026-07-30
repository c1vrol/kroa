# Getting started

This guide takes you from installation to your first working Kroa program.

## 1. Install tools

You need two tools:

1. **Rust** — used to build the Kroa compiler  
2. **Clang (LLVM)** — used to turn LLVM IR into a native executable

### Install Rust

Follow https://rustup.rs/ and then verify:

```bash
rustc --version
cargo --version
```

### Install Clang

Install LLVM so that `clang` is available in your terminal:

```bash
clang --version
```

On Windows, add `C:\Program Files\LLVM\bin` to your `PATH` if needed.

## 2. Build Kroa

From the project root:

```bash
cargo build --release
```

Run the compiler with:

```bash
./target/release/kroa run examples/hello.kroa
```
## 3. Write a tiny program

Create `hello.kroa`:

```kroa
fn main() -> i64:
    let message_number = 40
    print_i64(message_number + 2)
    return 0
```

What this means:

- `fn main() -> i64` defines the program entry point. It returns an integer exit-style value.
- `let` creates an immutable variable (you cannot change it later unless you use `let mut`).
- `print_i64(...)` prints an integer.
- Indentation (spaces only) defines the function body. Tabs are rejected.

## 4. Compile and run

```bash
kroa run hello.kroa
```

You should see:

```text
42
```

## 5. Useful next steps

- Read the [Language guide](language-guide.md) for variables, control flow, and functions.
- Check the [Reference](reference.md) for a short summary of syntax and commands.
- If something fails, open [Troubleshooting](troubleshooting.md).

## Notes about performance

Kroa compiles ahead of time. That means your program becomes a normal executable, similar in spirit to C.  
Exact speed depends on the program. Numeric code can be very fast; safety checks and abstractions may add cost when they cannot be optimized away.
