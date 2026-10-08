# WBook

A tiny and beautiful txt to epub converter, with toc and metadata combined,
written in Rust and Typescript.

## Features

- PreProcess and PostProcess support
- Send to Kindle support
- Customizable TOC, Metadata and Content in each chapter
- Simple but powerful template engine, based on DJANGO2 template engine

## Requirements

- Rust 1.70+
- LLVM 11+
- NodeJS 18+
- PNPM 6.0+

## Installation

Download the package for your platform from
[Releases](https://github.com/greenhat616/WBook/releases). The Windows
`*_portable.zip` needs no installation and keeps its settings in a `data`
folder next to `wbook.exe`.

## Development

```bash
git clone
cd wbook
pnpm install
pnpm tauri dev
```

## Build

```bash
pnpm tauri build
```

## License

Licensed under LGPL-3.0 License. See [LICENSE](LICENSE) for more information.
