# Contributing to parallel-bz2

Thank you for your interest in contributing to `parallel-bz2`! We welcome contributions of all forms, including code, documentation, bug reports, and feature requests.

## Getting Started

1. Fork the repository and create your branch from `main`
2. Make your changes with clear, descriptive commit messages
3. Ensure your code follows the project's style
4. Run tests: `cargo test`
5. Run clippy: `cargo clippy --all-targets --all-features`
6. Format your code: `cargo fmt`
7. Submit a pull request

## Development Setup

1. Install Rust using [rustup](https://rustup.rs/)
2. Clone the repository:
   ```bash
   git clone https://github.com/parallel-bz2/parallel-bz2.git
   cd parallel-bz2
   ```
3. Build the project:
   ```bash
   cargo build
   ```
4. Run all tests:
   ```bash
   cargo test
   ```

## Testing

Before submitting your changes, please ensure all tests pass:
```bash
cargo test
```

If you're making performance-related changes, please run the benchmarks:
```bash
cargo bench
```

## Code Style

- Follow the [Rust Style Guide](https://doc.rust-lang.org/1.0.0/style/)
- Use `cargo fmt` to format your code
- Use `cargo clippy` to catch common mistakes and improve your code
- Write documentation for all public items using rustdoc

## Issues

- Feel free to open an issue for questions, bug reports, or feature requests
- For bugs, please include a minimal reproduction case
- For features, please explain the use case

## Pull Requests

- Keep pull requests focused and atomic
- Include documentation updates when appropriate
- Add tests for new functionality
- Ensure CI passes before requesting a review

## Questions?

If you have any questions, feel free to open an issue with the "question" label.
