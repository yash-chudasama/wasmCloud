![Crates.io](https://img.shields.io/crates/v/wasmcloud-control-interface)
[![Documentation](https://img.shields.io/badge/Docs-Documentation-blue)](https://wasmcloud.dev)
[![Rustdocs](https://docs.rs/wasmcloud-control-interface/badge.svg)](https://docs.rs/wasmcloud-control-interface)

# wasmCloud Control Interface

This library is a convenient API for interacting with the lattice control interface. This is a Rust crate that implements the [lattice control protocol](https://wasmcloud.dev/reference/lattice-protocols/control-interface/) as described in the wasmCloud reference documentation.

The lattice control interface provides a way for clients to interact with the lattice to issue control commands and queries. This interface is a message broker protocol that supports functionality like starting and stopping actors and providers, declaring link definitions, monitoring lattice events, holding auctions to determine scheduling compatibility, and much more.

## ⚠️ Temporary location

This repository is a temporary location for this crate, which used to be
in
github.com/wasmcloud/wasmcloud/crates/wasmcloud-control-interface.

The plan is to move this crate back into the wasmcloud host repo once
version 0.50 is stabilized and merged.
