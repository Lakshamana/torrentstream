# TorrentStream 🚧

> **⚠️ Work In Progress**: This project is in very early development stages and is not yet ready for production use. Features are incomplete and the API may change significantly.

A high-performance BitTorrent client CLI written in Rust, designed for speed, safety, and simplicity.

## 🎯 Project Goals

TorrentStream aims to be a modern, efficient BitTorrent client that leverages Rust's performance and safety guarantees to provide:

- **Fast downloads** with concurrent piece downloading
- **Memory safety** without garbage collection overhead  
- **Cross-platform compatibility** (Linux, macOS, Windows)
- **Clean CLI interface** for automation and scripting
- **Robust error handling** and recovery mechanisms

## ✨ Features

### Currently Implemented
- [x] Torrent file parsing (.torrent files)
- [x] Tracker communication (HTTP/HTTPS)
- [x] Peer discovery and connection
- [x] BitTorrent protocol handshake
- [x] Piece-based downloading with integrity verification
- [x] Basic CLI interface
- [x] Concurrent downloads with connection pooling

### Planned Features
- [ ] DHT (Distributed Hash Table) support
- [ ] Magnet link support
- [ ] Upload/seeding capabilities
- [ ] Resume interrupted downloads
- [ ] Bandwidth limiting
- [ ] Web UI for monitoring
- [ ] Configuration file support
- [ ] Logging and statistics

## 🚀 Installation

### Prerequisites
- Rust 1.77+ (specified in `codecrafters.yml`)
- Git

### From Source
```bash
# Clone the repository
git clone https://github.com/yourusername/torrentstream.git
cd torrentstream

# Build the project
cargo build --release

# The binary will be available at target/release/bittorrent-starter-rust
```

### Development Build
```bash
# For development with debug symbols
cargo build

# Run directly with cargo
cargo run -- --help
```

## 📖 Usage

### Basic Download
```bash
# Download a torrent file to specified output directory
./target/release/bittorrent-starter-rust download -o /path/to/output file.torrent
```

### Command Line Options
```bash
torrentstream download [OPTIONS] <TORRENT_PATH>

OPTIONS:
    -o, --output <OUTPUT>    Output directory for downloaded files
    -h, --help              Print help information
    -V, --version           Print version information
```

### Examples
```bash
# Download Ubuntu ISO
torrentstream download -o ~/Downloads ubuntu-22.04.torrent

# Download to current directory
torrentstream download -o . sample.torrent
```

## 🏗️ Architecture

The project is structured with the following key components:

- **`src/main.rs`** - Entry point and CLI argument parsing
- **`src/types.rs`** - Core data structures (Torrent, Peer, Message, etc.)
- **`src/pool.rs`** - Connection pooling and peer management
- **`src/queue.rs`** - Download queue and piece scheduling
- **`src/constants.rs`** - Configuration constants and defaults

### Key Technologies
- **Async Runtime**: Tokio for concurrent operations
- **Serialization**: Serde with Bencode support for torrent files
- **HTTP Client**: Reqwest for tracker communication
- **CLI**: Clap for command-line argument parsing
- **Cryptography**: SHA-1 hashing for piece verification

## 🛠️ Development

### Running Tests
```bash
cargo test
```

### Code Formatting
```bash
cargo fmt
```

### Linting
```bash
cargo clippy
```

### Development Workflow
1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests and ensure they pass
5. Format code with `cargo fmt`
6. Submit a pull request

## 📋 Project Status

This project is part of a learning exercise and is **not production-ready**. Current limitations include:

- No seeding/uploading capability
- Limited error recovery
- No configuration options
- Basic logging only
- Single-file torrents only (no multi-file support yet)

## 🤝 Contributing

Contributions are welcome! Since this is a learning project, please:

1. Check existing issues before creating new ones
2. Follow Rust best practices and idioms
3. Add tests for new functionality
4. Update documentation as needed

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Built as part of the [CodeCrafters](https://codecrafters.io/) BitTorrent challenge
- Inspired by the BitTorrent protocol specification (BEP-0003)
- Thanks to the Rust community for excellent crates and documentation

## 📚 Resources

- [BitTorrent Protocol Specification](http://bittorrent.org/beps/bep_0003.html)
- [Rust Book](https://doc.rust-lang.org/book/)
- [Tokio Documentation](https://tokio.rs/)

---

**Note**: This README will be updated as the project evolves. Check back for the latest information on features and usage.
